//! XQ 風格看盤 demo（Rust + GPUI 原生繪圖），接 WryFeedHost（127.0.0.1:47631）行情。
//!
//! 版面：上＝走勢圖、左下＝報價表、右下＝資訊格（不嵌 WebView），分隔線可拖曳。
//! 效能：行情在背景執行緒解碼＋合併（xq-feed / bridge），UI 每 frame 最多套用一次；
//! 報價表／走勢圖／資訊格是各自的 cached view，只有 notify 的那塊會重畫。

mod bridge;
mod chart;
mod names;
mod series;
mod table;
mod theme;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, MouseButton, MouseMoveEvent, SharedString,
    StyleRefinement, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowOptions,
};

use bridge::{Conn, FeedBridge};
use chart::{ChartEvent, ChartView};
use series::Period;
use table::{QuoteTable, TableEvent};
use theme::*;

const TITLE_H: f32 = 26.0;
const STATUS_H: f32 = 22.0;
const SPLIT: f32 = 5.0;

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Horizontal,
    Vertical,
}

#[derive(Default, Clone)]
struct Stats {
    fps: u32,
    rss_mb: f64,
    frames_in: u64,
    quote_rows_in: u64,
    applies: u32,
    apply_us_avg: u64,
    chart_paints: u64,
    chart_paint_us_avg: u64,
}

struct InfoPane {
    symbol: SharedString,
    name: SharedString,
    price: f64,
    change: f64,
    volume: i64,
    stats: Stats,
}

impl Render for InfoPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let has = self.price > 0.0;
        let color = dir_color(self.change);
        let kv = |k: &'static str, v: String, c: gpui::Rgba| {
            div()
                .flex()
                .flex_row()
                .justify_between()
                .h(px(20.))
                .child(div().text_color(dim()).child(k))
                .child(div().font_family(MONO_FONT).text_color(c).child(v))
        };
        let s = &self.stats;
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(panel())
            .text_size(px(12.))
            .text_color(text())
            .child(
                div()
                    .h(px(26.))
                    .px_2()
                    .flex()
                    .items_center()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child("資訊"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(220.))
                            .child(div().text_size(px(15.)).pb_1().child(format!("{} {}", self.symbol, self.name)))
                            .child(kv("成交", if has { format!("{:.2}", self.price) } else { "--".into() }, color))
                            .child(kv("漲跌", if has { fmt_change(self.change) } else { "--".into() }, color))
                            .child(kv("幅度", if has { format!("{:+.2}%", pct(self.price, self.change)) } else { "--".into() }, color))
                            .child(kv("昨收", if has { format!("{:.2}", self.price - self.change) } else { "--".into() }, text()))
                            .child(kv("總量", if has { fmt_int(self.volume) } else { "--".into() }, text())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(240.))
                            .child(div().text_size(px(13.)).pb_1().text_color(dim()).child("效能（每秒取樣）"))
                            .child(kv("引擎封包", format!("{}/s", s.frames_in), text()))
                            .child(kv("報價筆數（未合併）", format!("{}/s", fmt_int(s.quote_rows_in as i64)), text()))
                            .child(kv("UI 套用（合併後）", format!("{}/s", s.applies), text()))
                            .child(kv("套用平均耗時", format!("{} µs", s.apply_us_avg), text()))
                            .child(kv("走勢圖重畫", format!("{}/s · {} µs", s.chart_paints, s.chart_paint_us_avg), text())),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .text_color(dim())
                    .text_size(px(11.))
                    .child("右下網頁格：GPUI 版不內嵌 WebView（gpui-wry 的子視窗不受 GPUI 裁切），此格先做資訊／效能面板。"),
            )
    }
}

struct Terminal {
    bridge: FeedBridge,
    table: Entity<QuoteTable>,
    chart: Entity<ChartView>,
    info: Entity<InfoPane>,
    conn: Conn,
    top_frac: f32,
    left_w: f32,
    drag: Option<Drag>,
    frames: u32,
    applies: u32,
    apply_us: u64,
    last_frames_in: u64,
    last_rows_in: u64,
    last_paints: u64,
    last_paint_us: u64,
    stats: Stats,
    metrics_log: bool,
    no_cache: bool,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Terminal {
    fn new(cx: &mut Context<Self>) -> Self {
        let (bridge, mut wake_rx) = FeedBridge::start();
        let table = cx.new(|_| QuoteTable::new(names::WATCH));
        let (sym, name) = names::WATCH[0];
        let chart = cx.new(|_| ChartView::new(sym, name, Period::Intraday));
        let info = cx.new(|_| InfoPane {
            symbol: sym.into(),
            name: name.into(),
            price: 0.0,
            change: 0.0,
            volume: 0,
            stats: Stats::default(),
        });

        let subs = vec![
            cx.subscribe(&table, |this: &mut Self, _, ev: &TableEvent, cx| match ev {
                TableEvent::Select(sym) => this.switch_chart(sym.to_string(), None, cx),
            }),
            cx.subscribe(&chart, |this: &mut Self, _, ev: &ChartEvent, cx| match ev {
                ChartEvent::Period(p) => {
                    let sym = this.chart.read(cx).symbol().to_string();
                    this.switch_chart(sym, Some(*p), cx);
                }
            }),
        ];

        // 行情：背景喚醒 → 套用一次 → 睡到下一個 frame（≤ ~60 次/秒）
        let feed_task = cx.spawn(async move |this, cx| {
            while wake_rx.next().await.is_some() {
                if this.update(cx, |t, cx| t.apply_feed(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
        });
        // 每秒取樣 FPS / RSS（不跟 tick 綁）
        let stats_task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |t, cx| t.sample(cx)).is_err() {
                break;
            }
        });

        let mut t = Terminal {
            bridge,
            table,
            chart,
            info,
            conn: Conn::Connecting,
            top_frac: 0.56,
            left_w: 470.0,
            drag: None,
            frames: 0,
            applies: 0,
            apply_us: 0,
            last_frames_in: 0,
            last_rows_in: 0,
            last_paints: 0,
            last_paint_us: 0,
            stats: Stats::default(),
            metrics_log: std::env::var_os("XQ_METRICS").is_some(),
            no_cache: std::env::var_os("XQ_NO_VIEW_CACHE").is_some(),
            _tasks: vec![feed_task, stats_task],
            _subs: subs,
        };
        t.resubscribe(cx);
        t
    }

    fn resubscribe(&mut self, cx: &mut Context<Self>) {
        let symbols = self.table.read(cx).symbols();
        let chart = self.chart.read(cx);
        let sub = xq_feed::Subscribe { symbols, chart: chart.symbol().to_string(), period: chart.period().code().into() };
        self.bridge.subscribe(sub);
    }

    fn switch_chart(&mut self, symbol: String, period: Option<Period>, cx: &mut Context<Self>) {
        let (name, quote) = match self.table.read(cx).row(&symbol) {
            Some(r) => (r.name.to_string(), (r.price > 0.0).then_some((r.price, r.change, r.volume))),
            None => (symbol.clone(), None),
        };
        let period = period.unwrap_or_else(|| self.chart.read(cx).period());
        self.chart.update(cx, |c, cx| c.load(&symbol, &name, period, quote, cx));
        self.info.update(cx, |i, cx| {
            i.symbol = symbol.clone().into();
            i.name = name.clone().into();
            let (p, c, v) = quote.unwrap_or((0.0, 0.0, 0));
            i.price = p;
            i.change = c;
            i.volume = v;
            cx.notify();
        });
        self.resubscribe(cx);
    }

    fn apply_feed(&mut self, cx: &mut Context<Self>) {
        let t0 = Instant::now();
        let ib = self.bridge.take();
        if let Some(c) = ib.conn {
            self.conn = c;
            if c == Conn::Up {
                // 重連後 xq-feed 會自動重送最新訂閱；這裡不用再送
            }
            cx.notify();
        }
        if !ib.quotes.is_empty() {
            if self.conn != Conn::Ready {
                self.conn = Conn::Ready;
                cx.notify();
            }
            self.table.update(cx, |t, cx| t.apply(&ib.order, &ib.quotes, cx));
            let sym = self.chart.read(cx).symbol().to_string();
            if let Some(q) = ib.quotes.get(&sym) {
                let name = if q.name.is_empty() || q.name == q.symbol {
                    self.table.read(cx).row(&sym).map(|r| r.name.to_string()).unwrap_or_default()
                } else {
                    q.name.clone()
                };
                self.chart.update(cx, |c, cx| c.apply_quote(q.price, q.change, q.volume, &name, cx));
                self.info.update(cx, |i, cx| {
                    i.price = q.price;
                    i.change = q.change;
                    i.volume = q.volume;
                    cx.notify();
                });
            }
        }
        if let Some((sym, prev, bars)) = &ib.intraday {
            self.chart.update(cx, |c, cx| c.set_intraday(sym, *prev, bars, cx));
        }
        for (sym, bar) in &ib.minutes {
            self.chart.update(cx, |c, cx| c.apply_minute(sym, bar, cx));
        }
        self.applies += 1;
        self.apply_us += t0.elapsed().as_micros() as u64;
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let frames_in = self.bridge.counters.frames.load(Ordering::Relaxed);
        let rows_in = self.bridge.counters.quote_rows.load(Ordering::Relaxed);
        let (paints, paint_us) = {
            let c = self.chart.read(cx);
            (c.paints.get(), c.paint_us.get())
        };
        let dp = paints - self.last_paints;
        self.stats = Stats {
            fps: self.frames,
            rss_mb: rss_mb(),
            frames_in: frames_in - self.last_frames_in,
            quote_rows_in: rows_in - self.last_rows_in,
            applies: self.applies,
            apply_us_avg: if self.applies > 0 { self.apply_us / self.applies as u64 } else { 0 },
            chart_paints: dp,
            chart_paint_us_avg: if dp > 0 { (paint_us - self.last_paint_us) / dp } else { 0 },
        };
        if self.metrics_log {
            let s = &self.stats;
            eprintln!(
                "[metrics] fps={} rss_mb={:.1} feed_frames/s={} quote_rows/s={} applies/s={} apply_us_avg={} chart_paints/s={} chart_paint_us_avg={} conn={:?}",
                s.fps, s.rss_mb, s.frames_in, s.quote_rows_in, s.applies, s.apply_us_avg, s.chart_paints, s.chart_paint_us_avg, self.conn
            );
        }
        self.last_frames_in = frames_in;
        self.last_rows_in = rows_in;
        self.last_paints = paints;
        self.last_paint_us = paint_us;
        self.frames = 0;
        self.applies = 0;
        self.apply_us = 0;
        let stats = self.stats.clone();
        self.info.update(cx, |i, cx| {
            i.stats = stats;
            cx.notify();
        });
        cx.notify();
    }

    fn on_drag(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.drag else { return };
        if !ev.dragging() {
            self.drag = None;
            return;
        }
        let vp = window.viewport_size();
        match d {
            Drag::Horizontal => {
                let avail = f32::from(vp.height) - TITLE_H - STATUS_H - SPLIT;
                let y = f32::from(ev.position.y) - TITLE_H;
                self.top_frac = (y / avail).clamp(0.15, 0.85);
            }
            Drag::Vertical => {
                self.left_w = f32::from(ev.position.x).clamp(200.0, f32::from(vp.width) - 200.0);
            }
        }
        cx.notify();
    }
}

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

impl Render for Terminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 根 view 每個 frame 都會 render（子 view 是 cached），所以這裡數 FPS
        self.frames += 1;
        let vp = window.viewport_size();
        let avail = f32::from(vp.height) - TITLE_H - STATUS_H - SPLIT;
        let top_h = (avail * self.top_frac).floor();
        let (conn_text, conn_color) = match self.conn {
            Conn::Connecting => ("連線中…", dim()),
            Conn::Up => ("已連線，等待行情", accent()),
            Conn::Ready => ("行情連線", down()),
            Conn::Down => ("行情中斷，重連中", up()),
        };
        let addr = xq_feed::feed_addr();
        let s = &self.stats;
        let dragging = self.drag;
        let no_cache = self.no_cache;
        let full = || StyleRefinement::default().size_full();
        let view = |v: gpui::AnyView| -> gpui::AnyElement {
            if no_cache { v.into_any_element() } else { v.cached(full()).into_any_element() }
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg())
            .font_family(UI_FONT)
            .text_size(px(12.))
            .text_color(text())
            .on_mouse_move(cx.listener(Self::on_drag))
            .on_any_mouse_down(|ev, _, _| {
                if std::env::var_os("XQ_DEBUG").is_some() {
                    eprintln!("[debug] mouse down {:?} at {:?}", ev.button, ev.position);
                }
            })
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.drag.take().is_some() {
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(TITLE_H))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child(div().text_color(up()).child("XQ"))
                    .child("風格測試 · GPUI 原生版")
                    .child(div().text_color(dim()).child("同一個 WryFeedHost 行情（demo-ticks 為合成測試資料）")),
            )
            .child(div().h(px(top_h)).w_full().child(view(self.chart.clone().into())))
            .child(
                div()
                    .id("hsplit")
                    .h(px(SPLIT))
                    .w_full()
                    .cursor_row_resize()
                    .bg(if dragging == Some(Drag::Horizontal) { accent() } else { border() })
                    .hover(|s| s.bg(accent()))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.drag = Some(Drag::Horizontal);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .child(div().w(px(self.left_w)).h_full().child(view(self.table.clone().into())))
                    .child(
                        div()
                            .id("vsplit")
                            .w(px(SPLIT))
                            .h_full()
                            .cursor_col_resize()
                            .bg(if dragging == Some(Drag::Vertical) { accent() } else { border() })
                            .hover(|s| s.bg(accent()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.drag = Some(Drag::Vertical);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1().h_full().child(view(self.info.clone().into()))),
            )
            .child(
                div()
                    .h(px(STATUS_H))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .px_3()
                    .bg(header())
                    .border_t_1()
                    .border_color(border())
                    .text_size(px(11.))
                    .child(div().text_color(conn_color).child(format!("● {conn_text} {addr}")))
                    .child(div().flex_1())
                    .child(div().font_family(MONO_FONT).child(format!("FPS {}", s.fps)))
                    .child(div().font_family(MONO_FONT).child(format!("RSS {:.1} MB", s.rss_mb)))
                    .child(div().font_family(MONO_FONT).text_color(dim()).child(format!(
                        "封包 {}/s → 套用 {}/s",
                        s.frames_in, s.applies
                    ))),
            )
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1240.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("XQ 風格測試（GPUI）".into()), ..Default::default() }),
                ..Default::default()
            },
            |_window, cx| cx.new(Terminal::new),
        )
        .expect("open main window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
