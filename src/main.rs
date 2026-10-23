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
mod tick_tape;
mod tickbench;
mod layers;
mod zstress;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, MouseButton, MouseMoveEvent, SharedString,
    StyleRefinement, Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowId, WindowOptions,
};

use std::sync::Arc;

use bridge::{Conn, Counters, FeedBridge, Inbox};
use chart::{ChartEvent, ChartView};
use series::Period;
use table::{QuoteTable, TableEvent};
use tick_tape::{TapeFeed, TickTape};
use theme::*;

/// 根 view 畫過的 frame 總數（走勢圖壓測算 FPS 用）
pub static FRAMES: AtomicU64 = AtomicU64::new(0);

const TITLE_H: f32 = 26.0;
/// 主視窗超過這麼久沒畫（被蓋住／縮小），後備計時器直接套用行情
const FALLBACK_MS: u64 = 25;
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
    /// 上一次 render 畫出的成交價（XQ_SYNC_CHECK 比對用）
    shown_price: f64,
    price: f64,
    change: f64,
    volume: i64,
    stats: Stats,
}

impl Render for InfoPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.shown_price = self.price;
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

/// XQ_SYNC_CHECK=1：多視窗報價同步驗證。每個視窗每一幀（root render 開頭）比對「上一幀畫在畫面上的價」
/// （資訊格、走勢圖標題列）和共用報價表裡同一檔的價。不一致時，落後時間＝現在 − 報價表那一列被改的時間；
/// 如果報價表在「上一幀開始之前」就改了、上一幀卻沒畫出來，就算「落後超過一幀」（違規）。每秒印一行 `SYNC|{json}`。
#[derive(Default)]
struct SyncCheck {
    frames: u64,
    checks: u64,
    /// 畫面與報價表不一致的次數（新價在上一幀之後才到、下一幀就會畫：正常）
    mismatches: u64,
    /// 落後超過一幀的次數（新價在上一幀開始前就到了，上一幀卻沒畫：不該發生）
    violations: u64,
    max_stale_us: u64,
    /// 落後時間樣本（µs，只收不一致的）與幀間隔樣本（µs）
    stale: Vec<u64>,
    intervals: Vec<u64>,
    prev_render_us: u64,
}

struct Terminal {
    /// 只有主視窗（win 0）持有行情連線；XQ_WINDOWS>1 的其他視窗共用主視窗的報價表與計數器
    bridge: Option<FeedBridge>,
    counters: Arc<Counters>,
    win: usize,
    window_id: WindowId,
    /// 其他視窗：主視窗每次套用行情後，用同一份 Inbox 更新它們的走勢圖／資訊格（不重複解碼、不重複合併）
    peers: Vec<WeakEntity<Terminal>>,
    table: Entity<QuoteTable>,
    chart: Entity<ChartView>,
    info: Entity<InfoPane>,
    /// 成交明細範例面板（XQ_TAPE=1 才開；預設 None，版面與壓測不變）
    tape: Option<Entity<TickTape>>,
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
    /// 主視窗：行情在 root render 開頭套用（預設）；false＝舊的「喚醒就套用＋睡 16ms」（XQ_APPLY=timer）
    render_apply: bool,
    /// 這個視窗上一次 root render 的時間（後備套用判斷主視窗是不是停畫了）
    last_render: Option<Instant>,
    /// 後備計時器代為套用的次數（主視窗被蓋住／縮小時才會 > 0）
    fallback_applies: u64,
    /// XQ_SYNC_CHECK=1：多視窗報價同步驗證（每幀比對畫面上的價與共用報價表）
    sync: Option<SyncCheck>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Terminal {
    /// `shared`＝None：主視窗（開行情連線、建報價表）；Some：XQ_WINDOWS 的其他視窗，共用主視窗的報價表 entity
    fn new(win: usize, shared: Option<(Entity<QuoteTable>, Arc<Counters>)>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let primary = shared.is_none();
        let (sym, name) = names::WATCH[win % names::WATCH.len()];
        let tape_feed =
            (primary && std::env::var_os("XQ_TAPE").is_some_and(|v| v != "0")).then(|| TapeFeed::new(sym));
        let tape = tape_feed.clone().map(|f| cx.new(|cx| TickTape::new(f, sym, cx)));
        let (bridge, wake_rx, table, counters) = match shared {
            None => {
                let (bridge, wake_rx) = FeedBridge::start(tape_feed);
                let counters = bridge.counters.clone();
                (Some(bridge), Some(wake_rx), cx.new(|_| QuoteTable::new(names::WATCH)), counters)
            }
            Some((table, counters)) => (None, None, table, counters),
        };
        let window_id = window.window_handle().window_id();
        // XQ_PERIOD=D：啟動就開日線（基準測試用，和 wry 版同畫面）；預設分時
        let period = std::env::var("XQ_PERIOD").ok().and_then(|v| Period::from_code(&v)).unwrap_or(Period::Intraday);
        let chart = cx.new(|cx| ChartView::new(sym, name, period, cx));
        let info = cx.new(|_| InfoPane {
            symbol: sym.into(),
            name: name.into(),
            shown_price: 0.0,
            price: 0.0,
            change: 0.0,
            volume: 0,
            stats: Stats::default(),
        });

        let subs = vec![
            cx.subscribe(&table, |this: &mut Self, _, ev: &TableEvent, cx| match ev {
                TableEvent::Select(_, Some(w)) if *w != this.window_id => {}
                TableEvent::Select(sym, _) => {
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

        // 行情（2026-10-23 改成跟著畫面走）：背景喚醒 → 只標記主視窗要畫 → 下一幀 root render 開頭才套用（同一幀就畫出來）。
        //   舊做法是「喚醒就套用、再固定睡 16ms」，套用時間點跟刷新格點不同步，常常多等一幀；XQ_APPLY=timer 可切回舊做法對照。
        //   後備：主視窗被蓋住／縮小時不會畫（GPUI 停掉它的刷新），改由下面的後備計時器直接套用，其他視窗與報價表照樣即時。
        let render_apply = std::env::var("XQ_APPLY").as_deref() != Ok("timer");
        let feed_task = wake_rx.map(|mut wake_rx| cx.spawn(async move |this, cx| {
            while wake_rx.next().await.is_some() {
                let r = this.update(cx, |t, cx| if t.render_apply { cx.notify() } else { t.apply_feed(cx) });
                if r.is_err() {
                    break;
                }
                if !render_apply {
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                }
            }
        }));
        let fallback_task = (primary && render_apply).then(|| cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(FALLBACK_MS)).await;
            let r = this.update(cx, |t, cx| {
                let stale = t.last_render.is_none_or(|r| r.elapsed() >= Duration::from_millis(FALLBACK_MS));
                if stale && t.bridge.as_ref().is_some_and(|b| b.pending()) {
                    t.fallback_applies += 1;
                    t.apply_feed(cx);
                }
            });
            if r.is_err() {
                break;
            }
        }));
        // 每秒取樣 FPS / RSS（不跟 tick 綁）
        let stats_task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |t, cx| t.sample(cx)).is_err() {
                break;
            }
        });

        let mut t = Terminal {
            bridge,
            counters,
            win,
            window_id,
            peers: Vec::new(),
            table,
            chart,
            info,
            tape,
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
            tick: xq_feed::tick_bench_enabled().then(|| tickbench::TickBench::for_window(win)),
            render_apply,
            last_render: None,
            fallback_applies: 0,
            sync: (std::env::var_os("XQ_SYNC_CHECK").is_some_and(|v| v != "0")).then(SyncCheck::default),
            _tasks: feed_task.into_iter().chain(fallback_task).chain([stats_task]).collect(),
            _subs: subs,
        };
        if !primary {
            return t;
        }
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
        // 其他視窗不另外訂閱：報價表訂閱的代號（含 5 萬壓測列）主視窗都已訂了
        let Some(bridge) = self.bridge.as_mut() else { return };
        let symbols = self.table.read(cx).symbols();
        let chart = self.chart.read(cx);
        let sub = xq_feed::Subscribe { symbols, chart: chart.symbol(), period: chart.period().feed_code().into() };
        bridge.subscribe(sub);
    }

    fn switch_chart(&mut self, symbol: String, period: Option<Period>, cx: &mut Context<Self>) {
        let (name, quote) = match self.table.read(cx).row(&symbol) {
            Some(r) => (r.name.to_string(), (r.price > 0.0).then_some((r.price, r.change, r.volume))),
            None => (symbol.clone(), None),
        };
        let period = period.unwrap_or_else(|| self.chart.read(cx).period());
        self.chart.update(cx, |c, cx| c.load(&symbol, &name, period, quote, cx));
        if let Some(tape) = &self.tape {
            tape.update(cx, |t, cx| t.set_symbol(&symbol, cx));
        }
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
        let Some(bridge) = self.bridge.as_mut() else { return };
        let ib = bridge.take();
        // 報價表只合併一次（多視窗共用同一個 entity，notify 後每個畫它的視窗各自重畫可見列）
        let changed = if ib.quotes.is_empty() {
            0
        } else {
            self.table.update(cx, |t, cx| t.apply(&ib.order, &ib.quotes, cx))
        };
        self.apply_local(&ib, cx);
        if let Some(tape) = &self.tape {
            // 和報價表同一個節奏（每幀最多一次）；沒有新成交就不 notify
            tape.update(cx, |t, cx| t.apply(cx));
        }
        let us = t0.elapsed().as_micros() as u64;
        self.finish_apply(&ib, changed, us, cx);
        if !self.peers.is_empty() {
            self.peers.retain(|p| p.upgrade().is_some());
            for p in self.peers.clone() {
                let _ = p.update(cx, |p, cx| {
                    let t1 = Instant::now();
                    p.apply_local(&ib, cx);
                    let us = us + t1.elapsed().as_micros() as u64;
                    p.finish_apply(&ib, changed, us, cx);
                });
            }
        }
    }

    /// 每個視窗自己的部分：連線狀態、自己這張走勢圖、資訊格
    fn apply_local(&mut self, ib: &Inbox, cx: &mut Context<Self>) {
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
                let name = if q.name.is_empty() || q.name == q.symbol {
                    self.table.read(cx).row(&sym).map(|r| r.name.to_string()).unwrap_or_default()
                } else {
                    q.name.clone()
                };
                self.chart.update(cx, |c, cx| c.apply_quote(q.price, q.change, q.volume, &name, cx));
                self.info.update(cx, |i, cx| {
                    // 沒變就不 notify：這個視窗顯示的東西都沒變時，GPUI 就不會重畫它
                    if i.price != q.price || i.change != q.change || i.volume != q.volume {
                        i.price = q.price;
                        i.change = q.change;
                        i.volume = q.volume;
                        cx.notify();
                    }
                });
            }
        }
        if let Some((sym, prev, bars)) = &ib.intraday {
            self.chart.update(cx, |c, cx| c.set_intraday(sym, *prev, bars, cx));
        }
        for (sym, bar) in &ib.minutes {
            self.chart.update(cx, |c, cx| c.apply_minute(sym, bar, cx));
        }
    }

    fn finish_apply(&mut self, ib: &Inbox, changed: usize, us: u64, cx: &mut Context<Self>) {
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
        self.sync_report();
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

    fn sync_check(&mut self, cx: &mut Context<Self>) {
        let now = xq_feed::unix_us();
        let sym = self.chart.read(cx).symbol();
        let shown = [self.info.read(cx).shown_price, self.chart.read(cx).shown_price];
        let row = self.table.read(cx).row(&sym).map(|r| (r.price, r.changed_us));
        let Some(sc) = self.sync.as_mut() else { return };
        sc.frames += 1;
        let prev = std::mem::replace(&mut sc.prev_render_us, now);
        let Some((px_, changed_us)) = row else { return };
        if prev == 0 || px_ <= 0.0 {
            return;
        }
        sc.intervals.push(now - prev);
        for s in shown {
            sc.checks += 1;
            if s != px_ {
                sc.mismatches += 1;
                let st = now.saturating_sub(changed_us);
                sc.max_stale_us = sc.max_stale_us.max(st);
                sc.stale.push(st);
                if changed_us < prev {
                    sc.violations += 1;
                }
            }
        }
    }

    fn sync_report(&mut self) {
        let Some(sc) = self.sync.as_mut() else { return };
        let q = |v: &mut Vec<u64>, p: f64| -> f64 {
            if v.is_empty() {
                return 0.0;
            }
            v.sort_unstable();
            v[((v.len() - 1) as f64 * p) as usize] as f64 / 1000.0
        };
        let (s50, s99) = (q(&mut sc.stale, 0.5), q(&mut sc.stale, 0.99));
        let (i50, i99) = (q(&mut sc.intervals, 0.5), q(&mut sc.intervals, 0.99));
        eprintln!(
            "SYNC|{{\"win\":{},\"now\":{},\"frames\":{},\"checks\":{},\"mismatch\":{},\"violations\":{},\"maxStaleMs\":{:.1},\"staleP50\":{s50:.1},\"staleP99\":{s99:.1},\"frameMsP50\":{i50:.1},\"frameMsP99\":{i99:.1},\"fallbackApplies\":{}}}",
            self.win, xq_feed::unix_us(), sc.frames, sc.checks, sc.mismatches, sc.violations, sc.max_stale_us as f64 / 1000.0, self.fallback_applies
        );
        sc.frames = 0;
        sc.checks = 0;
        sc.mismatches = 0;
        sc.violations = 0;
        sc.max_stale_us = 0;
        sc.stale.clear();
        sc.intervals.clear();
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

/// macOS：目前程序的常駐記憶體（resident size，相當於 Linux RSS），用 proc_pidinfo(PROC_PIDTASKINFO)
#[cfg(target_os = "macos")]
pub fn rss_mb() -> f64 {
    unsafe {
        let mut ti: libc::proc_taskinfo = std::mem::zeroed();
        let sz = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        let n = libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, &mut ti as *mut _ as *mut libc::c_void, sz);
        if n == sz { ti.pti_resident_size as f64 / (1024.0 * 1024.0) } else { 0.0 }
    }
}

#[cfg(not(target_os = "macos"))]
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
        self.last_render = Some(Instant::now());
        // 行情在這一幀開頭套用：子 view 在這之後才 prepaint，被 notify 的會在同一幀重畫
        if self.render_apply && self.bridge.as_ref().is_some_and(|b| b.pending()) {
            self.apply_feed(cx);
        }
        if let Some(tb) = self.tick.as_mut() {
            tb.on_render(window);
        }
        if self.sync.is_some() {
            self.sync_check(cx);
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
                    .child(div().flex_1().h_full().child(view(self.info.clone().into())))
                    .when_some(self.tape.clone(), |d, tape| {
                        d.child(div().w(px(SPLIT)).h_full().bg(border()))
                            .child(div().w(px(276.)).h_full().flex_none().child(view(tape.into())))
                    }),
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

/// XQ_WINDOWS=N（預設 1，最多 8）：開 N 個看盤視窗（各自走勢圖＝WATCH[k]），
/// 共用同一條行情連線、同一個報價表 entity（同一份 5 萬列資料，只合併一次），每個視窗自己的 view／走勢圖。
fn window_count() -> usize {
    std::env::var("XQ_WINDOWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 8)
}

/// 壓測用視窗位置：`XQ_WIN_RECTS="x,y,w,h[,did,dx,dy];…"`（第 k 個給第 k 個視窗）。
/// x,y,w,h＝全域座標（邏輯點，左上角原點＝主螢幕左上）的內容區；did＝該螢幕的 display id（macOS CGDirectDisplayID），
/// dx,dy＝該螢幕左上角的全域座標。GPUI 的視窗位置是「相對於所在螢幕」，所以換算成 (x-dx, y-dy) 並指定 display。
/// 沒設定時照舊：1240×760 置中。macOS 測試包（bench/mac）用這個把視窗放到指定螢幕、指定大小（不需要輔助使用權限）。
pub fn win_rect(k: usize) -> Option<(f32, f32, f32, f32, Option<(u64, f32, f32)>)> {
    let v = std::env::var("XQ_WIN_RECTS").ok()?;
    let e = v.split(';').filter(|e| !e.trim().is_empty()).nth(k)?;
    let n: Vec<f64> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if n.len() < 4 {
        return None;
    }
    let disp = (n.len() >= 7).then(|| (n[4] as u64, n[5] as f32, n[6] as f32));
    Some((n[0] as f32, n[1] as f32, n[2] as f32, n[3] as f32, disp))
}

fn window_options(k: usize, cx: &mut App) -> WindowOptions {
    let (bounds, display_id) = match win_rect(k) {
        Some((x, y, w, h, Some((did, dx, dy)))) => {
            (Bounds::new(gpui::point(px(x - dx), px(y - dy)), size(px(w), px(h))), Some(gpui::DisplayId::new(did)))
        }
        Some((x, y, w, h, None)) => (Bounds::new(gpui::point(px(x), px(y)), size(px(w), px(h))), None),
        None => (Bounds::centered(None, size(px(1240.0), px(760.0)), cx), None),
    };
    let title = if k == 0 { "XQ 風格測試（GPUI）".to_string() } else { format!("XQ 風格測試（GPUI） #{}", k + 1) };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
        ..Default::default()
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        tick_tape::bind_keys(cx);
        let opts = window_options(0, cx);
        let main = cx
            .open_window(opts, |window, cx| cx.new(|cx| Terminal::new(0, None, window, cx)))
            .expect("open main window");
        let n = window_count();
        if n > 1 {
            let primary = main.entity(cx).expect("main view");
            let (table, counters) = {
                let p = primary.read(cx);
                (p.table.clone(), p.counters.clone())
            };
            for k in 1..n {
                let shared = (table.clone(), counters.clone());
                let opts = window_options(k, cx);
                let w = cx
                    .open_window(opts, move |window, cx| {
                        cx.new(|cx| Terminal::new(k, Some(shared), window, cx))
                    })
                    .expect("open window");
                if let Ok(peer) = w.entity(cx) {
                    let weak = peer.downgrade();
                    primary.update(cx, |p, _| p.peers.push(weak));
                }
            }
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
