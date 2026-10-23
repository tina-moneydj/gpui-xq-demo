//! XQ 風格看盤 demo 手機版（Android，GPUI + longbridge/gpui-mobile，GpuiInputActivity／NativeActivity）。
//!
//! 行情、報價表、走勢圖、指標、畫線、圖層、壓測程式碼全部直接用桌面版 `../src/*.rs`（#[path]），
//! 只有版面換成手機單一畫面：上＝狀態列＋搜尋框，中間＝報價表（5 萬列 uniform_list、手指 fling）或走勢圖（點一列進入），
//! 下＝封包／套用計數。觸控（gpui-pre 0.3.9 的手勢）：點＝MouseDown/Up、單指拖曳＝帶 touch_phase 的捲動（含慣性）、
//! 雙指＝PinchEvent、長按＝LongPressEvent；走勢圖的對應寫在 chart.rs（cfg android）。
//! 設定：`adb shell setprop debug.xq.e0 KEY=VAL`（見 droid.rs），桌面版的 XQ_* 壓測開關原樣可用。
#![cfg(target_os = "android")]
#![allow(dead_code)]

#[path = "../../src/bridge.rs"]
mod bridge;
#[path = "../../src/chart.rs"]
mod chart;
mod droid;
#[path = "../../src/drawings.rs"]
mod drawings;
#[path = "../../src/gfx.rs"]
mod gfx;
#[path = "../../src/indicators.rs"]
mod indicators;
#[path = "../../src/layers.rs"]
mod layers;
#[path = "../../src/model.rs"]
mod model;
#[path = "../../src/names.rs"]
mod names;
mod search;
#[path = "../../src/series.rs"]
mod series;
#[path = "../../src/store.rs"]
mod store;
#[path = "../../src/table.rs"]
mod table;
#[path = "../../src/theme.rs"]
mod theme;
#[path = "../../src/tick_tape.rs"]
mod tick_tape;
#[path = "../../src/tickbench.rs"]
mod tickbench;
#[path = "../../src/zstress.rs"]
mod zstress;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{div, prelude::*, px, App, Context, Entity, SharedString, StyleRefinement, Subscription, Task, Window, WindowOptions};

use bridge::{Conn, Counters, FeedBridge};
use chart::{ChartEvent, ChartView};
use search::{SearchEvent, SearchInput};
use series::Period;
use table::{QuoteTable, TableEvent};
use theme::*;

/// 根 view 畫過的 frame 總數（chart.rs 的走勢圖壓測算 FPS 用）
pub static FRAMES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Chart,
}

#[derive(Default, Clone)]
struct Stats {
    fps: u32,
    rss_mb: f64,
    frames_in: u64,
    quote_rows_in: u64,
    applies: u32,
    apply_us_avg: u64,
}

struct MobileRoot {
    bridge: FeedBridge,
    counters: Arc<Counters>,
    table: Entity<QuoteTable>,
    chart: Entity<ChartView>,
    search: Entity<SearchInput>,
    hits: Vec<(SharedString, SharedString)>,
    conn: Conn,
    screen: Screen,
    frames: u32,
    total_frames: u64,
    applies: u32,
    apply_us: u64,
    last_frames_in: u64,
    last_rows_in: u64,
    stats: Stats,
    metrics_log: bool,
    flog: droid::FrameLog,
    bench: bool,
    bench_frame_logged: bool,
    bench_quote_logged: bool,
    track_jank: bool,
    last_frame: Option<Instant>,
    jank_n: u64,
    jank_ms: f64,
    tick: Option<tickbench::TickBench>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl MobileRoot {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sym, name) = names::WATCH[0];
        let (bridge, mut wake_rx) = FeedBridge::start(None);
        let counters = bridge.counters.clone();
        let table = cx.new(|_| QuoteTable::new(names::WATCH));
        let period = std::env::var("XQ_PERIOD").ok().and_then(|v| Period::from_code(&v)).unwrap_or(Period::Intraday);
        let chart = cx.new(|cx| ChartView::new(sym, name, period, cx));
        let search = cx.new(SearchInput::new);
        let subs = vec![
            cx.subscribe(&table, |this: &mut Self, _, ev: &TableEvent, cx| match ev {
                TableEvent::Select(sym, _) => {
                    if let Some(tb) = this.tick.as_mut() {
                        tb.on_switch(sym);
                    }
                    this.switch_chart(sym.to_string(), None, cx);
                    // 壓測（高頻進價）時留在報價表，量的是「點選→畫面」；一般使用點一列進走勢圖
                    if this.tick.is_none() {
                        this.screen = Screen::Chart;
                    }
                    cx.notify();
                }
            }),
            cx.subscribe(&chart, |this: &mut Self, _, ev: &ChartEvent, cx| match ev {
                ChartEvent::Period(p) => {
                    let sym = this.chart.read(cx).symbol();
                    this.switch_chart(sym, Some(*p), cx);
                }
            }),
            cx.subscribe(&search, |this: &mut Self, _, ev: &SearchEvent, cx| match ev {
                SearchEvent::Changed(q) => {
                    this.hits = this.table.read(cx).search(q, 12);
                    cx.notify();
                }
                SearchEvent::Submit => {
                    if let Some((sym, _)) = this.hits.first().cloned() {
                        this.pick(sym, cx);
                    }
                }
                _ => {}
            }),
        ];
        let feed_task = cx.spawn(async move |this, cx| {
            while wake_rx.next().await.is_some() {
                if this.update(cx, |t, cx| t.apply_feed(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
        });
        let stats_task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |t, cx| t.sample(cx)).is_err() {
                break;
            }
        });
        let stress_chart = std::env::var_os("XQ_CHART_STRESS").is_some_and(|v| v != "0");
        let screen = if stress_chart || std::env::var("XQ_SCREEN").is_ok_and(|v| v == "chart") { Screen::Chart } else { Screen::List };
        let mut t = MobileRoot {
            bridge,
            counters,
            table,
            chart,
            search,
            hits: Vec::new(),
            conn: Conn::Connecting,
            screen,
            frames: 0,
            total_frames: 0,
            applies: 0,
            apply_us: 0,
            last_frames_in: 0,
            last_rows_in: 0,
            stats: Stats::default(),
            metrics_log: std::env::var_os("XQ_METRICS").is_some(),
            flog: droid::FrameLog::default(),
            bench: std::env::var_os("XQ_BENCH").is_some_and(|v| v != "0"),
            bench_frame_logged: false,
            bench_quote_logged: false,
            track_jank: false,
            last_frame: None,
            jank_n: 0,
            jank_ms: 0.0,
            tick: xq_feed::tick_bench_enabled().then(|| tickbench::TickBench::for_window(0)),
            _tasks: vec![feed_task, stats_task],
            _subs: subs,
        };
        if t.tick.is_some() {
            t.table.update(cx, |tb, cx| tb.enable_stress_indexed(cx));
        }
        if std::env::var_os("XQ_STRESS_LIST").is_some_and(|v| v != "0") {
            t.table.update(cx, |tb, cx| if !tb.stress { tb.toggle_stress(cx) });
        }
        t.resubscribe(cx);
        if let Some(task) = Self::spawn_stress(cx) {
            t._tasks.push(task);
        }
        t
    }

    fn pick(&mut self, sym: SharedString, cx: &mut Context<Self>) {
        // select_symbol 會發 TableEvent::Select → switch_chart＋進走勢圖
        self.table.update(cx, |t, cx| t.select_symbol(&sym, cx));
        self.search.update(cx, |s, cx| s.clear(cx));
        self.hits.clear();
        self.screen = Screen::Chart;
        cx.notify();
    }

    /// 與桌面版 XQ_GROUP_STRESS 同流程（1.2 秒後開 5 萬列 → 只跳價 → 邊跳價邊正弦捲動），印 GROUPPERF|{json}
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
            fn snap(t: &mut MobileRoot, cx: &mut Context<MobileRoot>) -> Snap {
                let tb = t.table.read(cx);
                (t.total_frames, tb.paints, tb.paint_us, tb.cells, tb.len())
            }
            fn stats(a: Snap, b: Snap, secs: f64) -> String {
                let paints = b.1 - a.1;
                let avg_ms = if paints > 0 { (b.2 - a.2) as f64 / paints as f64 / 1000.0 } else { 0.0 };
                let cells = if paints > 0 { (b.3 - a.3) / paints } else { 0 };
                format!("\"fps\":{:.1},\"paints\":{},\"avgPaintMs\":{:.3},\"avgCellsPerPaint\":{},\"cellUpdates\":{}", (b.0 - a.0) as f64 / secs, paints, avg_ms, cells, b.3 - a.3)
            }
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            if this.update(cx, |t, cx| t.table.update(cx, |tb, cx| if !tb.stress { tb.toggle_stress(cx) })).is_err() {
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
            let Ok((c, jn, jms)) = this.update(cx, |t, cx| {
                t.track_jank = false;
                (snap(t, cx), t.jank_n, t.jank_ms)
            }) else {
                return;
            };
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
        self.resubscribe(cx);
    }

    fn apply_feed(&mut self, cx: &mut Context<Self>) {
        let t0 = Instant::now();
        let ib = self.bridge.take();
        let changed = if ib.quotes.is_empty() { 0 } else { self.table.update(cx, |t, cx| t.apply(&ib.order, &ib.quotes, cx)) };
        if let Some(c) = ib.conn {
            self.conn = c;
            cx.notify();
        }
        if !ib.quotes.is_empty() {
            if self.conn != Conn::Ready {
                self.conn = Conn::Ready;
                cx.notify();
            }
            let sym = self.chart.read(cx).symbol();
            if let Some(q) = ib.quotes.get(&sym) {
                let name = if q.name.is_empty() || q.name == q.symbol { self.table.read(cx).row(&sym).map(|r| r.name.to_string()).unwrap_or_default() } else { q.name.clone() };
                self.chart.update(cx, |c, cx| c.apply_quote(q.price, q.change, q.volume, &name, cx));
            }
        }
        if let Some((sym, prev, bars)) = &ib.intraday {
            self.chart.update(cx, |c, cx| c.set_intraday(sym, *prev, bars, cx));
        }
        for (sym, bar) in &ib.minutes {
            self.chart.update(cx, |c, cx| c.apply_minute(sym, bar, cx));
        }
        let us = t0.elapsed().as_micros() as u64;
        self.applies += 1;
        self.apply_us += us;
        if let Some(tb) = self.tick.as_mut() {
            if ib.stamp.is_some() {
                cx.notify();
            }
            tb.on_apply(ib.stamp_first, ib.stamp, ib.quotes.len(), changed, us);
        }
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let frames_in = self.counters.frames.load(Ordering::Relaxed);
        let rows_in = self.counters.quote_rows.load(Ordering::Relaxed);
        self.stats = Stats {
            fps: self.frames,
            rss_mb: rss_mb(),
            frames_in: frames_in - self.last_frames_in,
            quote_rows_in: rows_in - self.last_rows_in,
            applies: self.applies,
            apply_us_avg: if self.applies > 0 { self.apply_us / self.applies as u64 } else { 0 },
        };
        if self.metrics_log {
            let s = &self.stats;
            self.flog.report(&format!(",\"rssMb\":{:.1},\"rowsIn\":{},\"applies\":{},\"applyUs\":{},\"screen\":\"{}\"", s.rss_mb, s.quote_rows_in, s.applies, s.apply_us_avg, if self.screen == Screen::Chart { "chart" } else { "list" }));
        }
        if let Some(tb) = self.tick.as_mut() {
            tb.report(rows_in, frames_in, self.stats.fps, self.stats.rss_mb);
        }
        self.last_frames_in = frames_in;
        self.last_rows_in = rows_in;
        self.frames = 0;
        self.applies = 0;
        self.apply_us = 0;
        cx.notify();
    }
}

impl Render for MobileRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        self.total_frames += 1;
        self.flog.frame();
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
            if !self.bench_quote_logged && self.conn == Conn::Ready {
                self.bench_quote_logged = true;
                window.on_next_frame(|_, _| eprintln!("BENCH first-quote"));
            }
        }
        let (conn_text, conn_color) = match self.conn {
            Conn::Connecting => ("連線中", dim()),
            Conn::Up => ("等待行情", accent()),
            Conn::Ready => ("行情連線", down()),
            Conn::Down => ("重連中", up()),
        };
        let s = self.stats.clone();
        let full = || StyleRefinement::default().size_full();
        let n_rows = self.table.read(cx).len();
        let body: gpui::AnyElement = match self.screen {
            Screen::List => div().flex_1().min_h_0().w_full().child(self.table.clone().cached(full())).into_any_element(),
            Screen::Chart => div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("back")
                        .h(px(34.))
                        .flex_none()
                        .px_3()
                        .flex()
                        .items_center()
                        .bg(header())
                        .text_size(px(15.))
                        .text_color(accent())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.screen = Screen::List;
                            cx.notify();
                        }))
                        .child("‹ 報價"),
                )
                .child(div().flex_1().min_h_0().w_full().child(self.chart.clone().cached(full())))
                .into_any_element(),
        };
        let hits = self.hits.clone();
        let rows: Vec<_> = {
            let table = self.table.read(cx);
            hits.iter()
                .map(|(sym, name)| {
                    let q = table.row(sym).map(|r| (r.price, r.change));
                    (sym.clone(), name.clone(), q)
                })
                .collect()
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg())
            .font_family(UI_FONT)
            .text_size(px(12.))
            .text_color(text())
            .child(
                div()
                    .h(px(34.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px(px(10.))
                    .bg(header())
                    .child(div().text_size(px(14.)).text_color(up()).child("XQ"))
                    .child(div().text_size(px(14.)).child("GPUI 手機版"))
                    .child(div().text_color(conn_color).child(format!("● {conn_text}")))
                    .child(div().flex_1())
                    .child(div().font_family(MONO_FONT).text_size(px(11.)).child(format!("FPS {}  RSS {:.0} MB", s.fps, s.rss_mb))),
            )
            .child(div().h(px(40.)).flex_none().px(px(8.)).flex().items_center().bg(panel()).child(self.search.clone()))
            .child(div().relative().flex_1().min_h_0().w_full().flex().flex_col().child(body).when(!self.search.read(cx).content.trim().is_empty(), |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .bg(panel())
                        .flex()
                        .flex_col()
                        .when(rows.is_empty(), |d| d.child(div().h(px(40.)).px_3().flex().items_center().text_size(px(14.)).text_color(dim()).child("找不到")))
                        .children(rows.into_iter().enumerate().map(|(k, (sym, name, q))| {
                            let s2 = sym.clone();
                            div()
                                .id(("hit", k))
                                .h(px(40.))
                                .px_3()
                                .flex()
                                .flex_row()
                                .items_center()
                                .border_b_1()
                                .border_color(border())
                                .text_size(px(15.))
                                .on_click(cx.listener(move |this, _, _, cx| this.pick(s2.clone(), cx)))
                                .child(div().w(px(78.)).font_family(MONO_FONT).child(sym))
                                .child(div().flex_1().child(name))
                                .when_some(q.filter(|(p, _)| *p > 0.0), |d, (p, c)| d.child(div().font_family(MONO_FONT).text_size(px(14.)).text_color(dir_color(c)).child(format!("{p:.2}"))))
                        })),
                )
            }))
            .child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px(px(10.))
                    .bg(header())
                    .border_t_1()
                    .border_color(border())
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .text_color(dim())
                    .child(format!("{}", xq_feed::feed_addr()))
                    .child(div().flex_1())
                    .child(format!("封包 {}/s → 套用 {}/s · {} 列", s.frames_in, s.applies, n_rows)),
            )
    }
}

pub fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()))
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    droid::init(app.internal_data_path());
    gpui_mobile::android::jni::install_panic_hook();
    let _platform = gpui_mobile::android::jni::init_platform(&app);
    let Some(shared) = gpui_mobile::android::jni::shared_platform() else {
        eprintln!("[gpui] shared_platform() = None");
        return;
    };
    eprintln!("[gpui] android_main: starting Application");
    gpui::Application::with_platform(shared.into_rc()).run(|cx: &mut App| {
        tick_tape::bind_keys(cx);
        search::bind_keys(cx);
        cx.open_window(WindowOptions::default(), |window, cx| cx.new(|cx| MobileRoot::new(window, cx))).expect("open window");
        cx.activate(true);
    });
    eprintln!("[gpui] Application.run returned");
}
