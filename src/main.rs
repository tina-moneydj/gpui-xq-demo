//! XQ 風格看盤 demo（Rust + GPUI 原生繪圖），接 WryFeedHost（127.0.0.1:47631）行情。
//!
//! 版面：上＝走勢圖、左下＝報價表、右下＝資訊格（不嵌 WebView），分隔線可拖曳。
//! 效能：行情在背景執行緒解碼＋合併（xq-feed / bridge），UI 每 frame 最多套用一次；
//! 報價表／走勢圖／資訊格是各自的 cached view，只有 notify 的那塊會重畫。

mod bridge;
mod chart;
mod drawings;
mod gfx;
mod indicators;
mod model;
mod names;
mod series;
mod store;
mod table;
mod theme;
mod tickbench;

use std::sync::atomic::{AtomicU64, Ordering};
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

/// 根 view 畫過的 frame 總數（走勢圖壓測算 FPS 用）
pub static FRAMES: AtomicU64 = AtomicU64::new(0);

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
    /// XQ_BENCH=1：第一個 frame／第一筆報價畫出後各在 stderr 印一行 `BENCH ...`（bench 腳本打時間戳）
    bench: bool,
    bench_frame_logged: bool,
    bench_quote_logged: bool,
    /// 壓測（XQ_GROUP_STRESS=1）：累計 frame 數、捲動時 frame 間隔 > 32ms 的次數／總毫秒
    total_frames: u64,
    track_jank: bool,
    last_frame: Option<Instant>,
    jank_n: u64,
    jank_ms: f64,
    /// XQ_TICK_BENCH=1：高頻進價壓測量測（預設 None，熱路徑不多做事）
    tick: Option<tickbench::TickBench>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Terminal {
    fn new(cx: &mut Context<Self>) -> Self {
        let (bridge, mut wake_rx) = FeedBridge::start();
        let table = cx.new(|_| QuoteTable::new(names::WATCH));
        let (sym, name) = names::WATCH[0];
        // XQ_PERIOD=D：啟動就開日線（基準測試用，和 wry 版同畫面）；預設分時
        let period = std::env::var("XQ_PERIOD").ok().and_then(|v| Period::from_code(&v)).unwrap_or(Period::Intraday);
        let chart = cx.new(|cx| ChartView::new(sym, name, period, cx));
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
                TableEvent::Select(sym) => {
                    if let Some(tb) = this.tick.as_mut() {
                        tb.on_switch(sym);
                    }
                    this.switch_chart(sym.to_string(), None, cx)
                }
            }),
            cx.subscribe(&chart, |this: &mut Self, _, ev: &ChartEvent, cx| match ev {
                ChartEvent::Period(p) => {
                    let sym = this.chart.read(cx).symbol();
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
            // XQ_TOP_FRAC：上方走勢圖佔比（重圖表壓測時調到與 wry 版同樣大的圖面）
            top_frac: std::env::var("XQ_TOP_FRAC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.56),
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
            bench: std::env::var_os("XQ_BENCH").is_some_and(|v| v != "0"),
            bench_frame_logged: false,
            bench_quote_logged: false,
            total_frames: 0,
            track_jank: false,
            last_frame: None,
            jank_n: 0,
            jank_ms: 0.0,
            tick: xq_feed::tick_bench_enabled().then(tickbench::TickBench::default),
            _tasks: vec![feed_task, stats_task],
            _subs: subs,
        };
        if t.tick.is_some() {
            // 高頻進價壓測：一開始就開 5 萬合成列（Z 代號進索引，engine 報價可對到）
            t.table.update(cx, |tb, cx| tb.enable_stress_indexed(cx));
        }
        t.resubscribe(cx);
        if let Some(task) = Self::spawn_stress(cx) {
            t._tasks.push(task);
        }
        t
    }

    /// XQ_GROUP_STRESS=1：與 wry 版 `__benchGroupWatchlist` 同流程——1.2 秒後開 5 萬合成列，
    /// 先只跳價（XQ_STRESS_TICK_HZ 筆/秒，30 Hz 分批，預設 3000）XQ_STRESS_TICK_MS，
    /// 再邊跳價邊上下正弦捲動 XQ_STRESS_SCROLL_MS（預設各 2000ms）。結果用 `GROUPPERF|{json}` 印到 stderr。
    fn spawn_stress(cx: &mut Context<Self>) -> Option<Task<()>> {
        if !std::env::var_os("XQ_GROUP_STRESS").is_some_and(|v| v != "0") {
            return None;
        }
        let num = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).unwrap_or(d);
        let tick_ms = num("XQ_STRESS_TICK_MS", 2000);
        let scroll_ms = num("XQ_STRESS_SCROLL_MS", 2000);
        let hz = num("XQ_STRESS_TICK_HZ", 3000);
        let batch = ((hz as f64) / 30.0).round().max(1.0) as usize;
        Some(cx.spawn(async move |this, cx| {
            type Snap = (u64, u64, u64, u64, usize);
            fn snap(t: &mut Terminal, cx: &mut Context<Terminal>) -> Snap {
                let tb = t.table.read(cx);
                (t.total_frames, tb.paints, tb.paint_us, tb.cells, tb.len())
            }
            fn stats(a: Snap, b: Snap, secs: f64) -> String {
                let paints = b.1 - a.1;
                let avg_ms = if paints > 0 { (b.2 - a.2) as f64 / paints as f64 / 1000.0 } else { 0.0 };
                let cells = if paints > 0 { (b.3 - a.3) / paints } else { 0 };
                format!(
                    "\"fps\":{:.1},\"paints\":{},\"avgPaintMs\":{:.3},\"avgCellsPerPaint\":{},\"cellUpdates\":{}",
                    (b.0 - a.0) as f64 / secs, paints, avg_ms, cells, b.3 - a.3
                )
            }
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            if this.update(cx, |t, cx| t.table.update(cx, |tb, cx| { if !tb.stress { tb.toggle_stress(cx) } })).is_err() {
                return;
            }
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
            let Ok(a) = this.update(cx, snap) else { return };
            eprintln!("GROUPPERF|{{\"phase\":\"tick-start\",\"rows\":{},\"cols\":6,\"tickHz\":{hz}}}", a.4);
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_millis(tick_ms) {
                let _ = this.update(cx, |t, cx| t.table.update(cx, |tb, cx| tb.synth_ticks(batch, &mut rng, cx)));
                cx.background_executor().timer(Duration::from_millis(33)).await;
            }
            let secs = t0.elapsed().as_secs_f64();
            let Ok(b) = this.update(cx, snap) else { return };
            eprintln!("GROUPPERF|{{\"phase\":\"tick-end\",\"elapsedMs\":{:.0},{}}}", secs * 1000.0, stats(a, b, secs));

            let _ = this.update(cx, |t, _| {
                t.track_jank = true;
                t.last_frame = None;
                t.jank_n = 0;
                t.jank_ms = 0.0;
            });
            let s0 = Instant::now();
            let mut last_tick = Instant::now();
            while s0.elapsed() < Duration::from_millis(scroll_ms) {
                let f = s0.elapsed().as_secs_f64() / (scroll_ms as f64 / 1000.0);
                let frac = (f * std::f64::consts::PI * 4.0).sin() * 0.5 + 0.5;
                let tick = last_tick.elapsed() >= Duration::from_millis(33);
                if tick {
                    last_tick = Instant::now();
                }
                let _ = this.update(cx, |t, cx| {
                    t.table.update(cx, |tb, cx| {
                        tb.scroll_frac(frac, cx);
                        if tick {
                            tb.synth_ticks(batch, &mut rng, cx);
                        }
                    })
                });
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
            let secs = s0.elapsed().as_secs_f64();
            let Ok(c) = this.update(cx, |t, cx| {
                t.track_jank = false;
                (snap(t, cx), t.jank_n, t.jank_ms)
            }) else {
                return;
            };
            let (c, jn, jms) = c;
            eprintln!(
                "GROUPPERF|{{\"phase\":\"done\",\"rows\":{},\"cols\":6,\"tickFps\":{:.1},\"scrollFps\":{:.1},\"scrollJankCount\":{jn},\"scrollJankAvgMs\":{:.1},{}}}",
                c.4,
                (b.0 - a.0) as f64 / (tick_ms as f64 / 1000.0),
                (c.0 - b.0) as f64 / secs,
                if jn > 0 { jms / jn as f64 } else { 0.0 },
                stats(b, c, secs)
            );
        }))
    }

    fn resubscribe(&mut self, cx: &mut Context<Self>) {
        let symbols = self.table.read(cx).symbols();
        let chart = self.chart.read(cx);
        let sub = xq_feed::Subscribe { symbols, chart: chart.symbol(), period: chart.period().feed_code().into() };
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
        let mut changed = 0;
        if !ib.quotes.is_empty() {
            if self.conn != Conn::Ready {
                self.conn = Conn::Ready;
                cx.notify();
            }
            changed = self.table.update(cx, |t, cx| t.apply(&ib.order, &ib.quotes, cx));
            let sym = self.chart.read(cx).symbol();
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
        let us = t0.elapsed().as_micros() as u64;
        self.apply_us += us;
        if let Some(tb) = self.tick.as_mut() {
            if ib.stamp.is_some() {
                cx.notify();
            }
            tb.on_apply(ib.stamp_first, ib.stamp, ib.quotes.len(), changed, us);
        }
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let frames_in = self.bridge.counters.frames.load(Ordering::Relaxed);
        let rows_in = self.bridge.counters.quote_rows.load(Ordering::Relaxed);
        let (paints, paint_us) = {
            self.chart.read(cx).paint_totals()
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
        if let Some(tb) = self.tick.as_mut() {
            tb.report(rows_in, frames_in, self.stats.fps, self.stats.rss_mb);
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

pub fn rss_mb() -> f64 {
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
        self.total_frames += 1;
        FRAMES.fetch_add(1, Ordering::Relaxed);
        if self.track_jank {
            let now = Instant::now();
            if let Some(prev) = self.last_frame.replace(now) {
                let dt = now.duration_since(prev).as_secs_f64() * 1000.0;
                if dt > 32.0 {
                    self.jank_n += 1;
                    self.jank_ms += dt;
                }
            }
        }
        if let Some(tb) = self.tick.as_mut() {
            tb.on_render(window);
        }
        if self.bench {
            if !self.bench_frame_logged {
                self.bench_frame_logged = true;
                window.on_next_frame(|_, _| eprintln!("BENCH first-frame"));
            }
            // 第一批報價會把 conn 切成 Ready 並 notify 根 view；這一幀畫完即報價已上畫面
            if !self.bench_quote_logged && self.conn == Conn::Ready {
                self.bench_quote_logged = true;
                window.on_next_frame(|_, _| eprintln!("BENCH first-quote"));
            }
        }
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
