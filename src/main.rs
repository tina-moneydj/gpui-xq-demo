//! XQ 風格看盤 demo（Rust + GPUI 原生繪圖），接 WryFeedHost（127.0.0.1:47631）行情。
//!
//! 版面：上＝走勢圖、左下＝報價表、右下＝資訊格（不嵌 WebView），分隔線可拖曳。
//! 效能：行情在背景執行緒解碼＋合併（xq-feed / bridge），UI 每 frame 最多套用一次；
//! 報價表／走勢圖／資訊格是各自的 cached view，只有 notify 的那塊會重畫。

/// 網頁版：eprintln! 改寫到 console.log（壓測腳本從 DevTools Protocol 收同樣的 BENCH／CHARTPERF／TICKBENCH 行）
#[cfg(target_arch = "wasm32")]
macro_rules! eprintln {
    ($($t:tt)*) => { $crate::plat::log(&format!($($t)*)) };
}
mod bridge;
mod plat;
mod chart;
mod drawings;
mod gfx;
mod indicators;
mod model;
mod names;
mod series;
mod search;
mod store;
mod table;
mod theme;
mod tick_tape;
mod tickbench;
mod layers;
mod zstress;
#[cfg(not(target_arch = "wasm32"))]
mod stall;
#[cfg(not(target_arch = "wasm32"))]
mod gpuilog;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_arch = "wasm32"))]
mod memrelief;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use crate::plat::Instant;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, MouseButton, MouseMoveEvent, SharedString,
    StyleRefinement, Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowId, WindowOptions,
};

use std::sync::Arc;

use bridge::{Conn, Counters, FeedBridge, Inbox, Taker};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use chart::{ChartEvent, ChartView};
use series::Period;
use table::{QuoteTable, TableEvent};
use tick_tape::{TapeFeed, TickTape};
use theme::*;

/// 根 view 畫過的 frame 總數（走勢圖壓測算 FPS 用）
pub static FRAMES: AtomicU64 = AtomicU64::new(0);

/// A4（2026-10-24）：實機高頻情境偶爾整個停 0.3–0.47 秒，原因還不知道。超過門檻（XQ_LONGFRAME_MS，預設 100 ms）的幀
/// 在 stderr 印一行 `LONGFRAME|{json}`：kind＝wake（行情到了、這個視窗隔多久才開始畫；gapMs＝距上一次畫，
/// 涵蓋上一幀 paint／present 卡住）／apply（套用行情本身）。有了時間點就能對照是第一次畫某種字、開新視窗、還是 GPU present 卡住。
fn longframe_ms() -> f64 {
    static V: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::plat::var("XQ_LONGFRAME_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(100.0))
}
/// 2026-10-25：舊版在第一次印 LONGFRAME 時才開始計時，所以 t 永遠是 0.000；改成程式一啟動就開始算（原生版跟 STALL 同一個起點）
#[cfg(not(target_arch = "wasm32"))]
fn since_start_s() -> f64 {
    stall::since_start_s()
}
#[cfg(target_arch = "wasm32")]
fn since_start_s() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}
/// 主執行緒打卡（網頁版沒有看門狗）
#[inline]
fn beat(p: u8) {
    #[cfg(not(target_arch = "wasm32"))]
    stall::beat(match p {
        1 => stall::Phase::Render,
        2 => stall::Phase::Painted,
        3 => stall::Phase::Wake,
        4 => stall::Phase::Timer,
        _ => stall::Phase::Fallback,
    });
    #[cfg(target_arch = "wasm32")]
    let _ = p;
}
fn log_long(win: usize, kind: &str, ms: f64, extra: &str) {
    eprintln!("LONGFRAME|{{\"win\":{win},\"kind\":\"{kind}\",\"ms\":{ms:.1},\"t\":{:.3}{extra}}}", since_start_s());
}

fn title_h() -> f32 { xq_tokens::t().layout.title_bar }
/// 主視窗超過這麼久沒畫（被蓋住／縮小），後備計時器直接套用行情
const FALLBACK_MS: u64 = 25;

/// A1（2026-10-24）：多視窗共用的行情匯流。
/// 舊做法：只有主視窗在自己的 root render 開頭取件、再把同一份 Inbox 推給其他視窗——其他視窗要等「主視窗畫了、
/// 自己再被 notify、等到自己的下一個 vsync」，實機上第 2–4 個視窗固定晚一幀（Mac／Windows 4 視窗 p99 約 64 ms vs egui 34 ms）。
/// 新做法：行情一到就 notify 全部視窗；哪個視窗先 render 就由它取件、合併進共用報價表，並把套用結果（Rc）放進每個視窗
/// 自己的佇列；每個視窗在自己的 render 開頭把佇列套到自己的走勢圖／資訊格。每個視窗都在自己這一幀就看到新價。
struct Applied {
    ib: Inbox,
    changed: usize,
    us: u64,
}
struct Hub {
    taker: Taker,
    tape: Option<Entity<TickTape>>,
    /// (視窗編號, 還沒套到這個視窗的批次)
    queues: RefCell<Vec<(usize, Vec<Rc<Applied>>)>>,
    /// 2026-10-25：最近一次的連線狀態。GPUI 的 open_window 會先同步畫一幀，主視窗在第 2～4 個視窗註冊前就可能已取走
    /// 「連線就緒」那一包；之後開的視窗從這裡接手，不必等下一包（壓測時要等約 3 秒後的快照）才離開「連線中…」。
    conn: Cell<Option<Conn>>,
}
impl Hub {
    fn register(&self, win: usize) {
        self.queues.borrow_mut().push((win, Vec::new()));
    }
    fn has_queued(&self, win: usize) -> bool {
        self.queues.borrow().iter().any(|(w, q)| *w == win && !q.is_empty())
    }
    fn drain(&self, win: usize) -> Vec<Rc<Applied>> {
        self.queues.borrow_mut().iter_mut().find(|(w, _)| *w == win).map(|(_, q)| std::mem::take(q)).unwrap_or_default()
    }
}
fn status_h() -> f32 { xq_tokens::t().layout.status_bar }
fn split() -> f32 { xq_tokens::t().layout.splitter }

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
    /// 上一秒主執行緒／整個程序 CPU（%，100＝一顆核心）；量不到是 -1
    cpu_main: f64,
    cpu_proc: f64,
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
                .child(div().font_family(mono_font()).text_color(c).child(v))
        };
        let s = &self.stats;
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(panel())
            .text_size(px(tk().font.md))
            .text_color(text())
            .child(
                div()
                    .h(px(tk().layout.title_bar))
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
                            .child(div().text_size(px(tk().font.xl)).pb_1().child(format!("{} {}", self.symbol, self.name)))
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
                            .child(div().text_size(px(tk().font.lg)).pb_1().text_color(dim()).child("效能（每秒取樣）"))
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
                    .text_size(px(tk().font.sm))
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
    /// 上次 sample() 的（時間、主執行緒 CPU 秒、程序 CPU 秒）
    last_cpu: Option<(Instant, f64, f64)>,
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
    /// 捲動時每幀的幀間隔（ms），算 p50／p95／p99；捲動開始時一次配好 FRAME_SAMPLES 格，滿了就不再記
    frame_ms: Vec<f32>,
    /// 捲動壓測（預設）：每一幀在 root render 開頭推進一次捲動位置；None＝沒在捲
    scroll_drive: Option<ScrollDrive>,
    /// XQ_TICK_BENCH=1：高頻進價壓測量測（預設 None，熱路徑不多做事）
    tick: Option<tickbench::TickBench>,
    /// 主視窗：行情在 root render 開頭套用（預設）；false＝舊的「喚醒就套用＋睡 16ms」（XQ_APPLY=timer）
    render_apply: bool,
    /// 這個視窗上一次 root render 的時間（後備套用判斷主視窗是不是停畫了）
    last_render: Option<Instant>,
    /// 後備計時器代為套用的次數（主視窗被蓋住／縮小時才會 > 0）
    fallback_applies: u64,
    /// 量測用的補畫次數（TickBench::awaiting_older_than）
    probe_kicks: u64,
    /// XQ_SYNC_CHECK=1：多視窗報價同步驗證（每幀比對畫面上的價與共用報價表）
    sync: Option<SyncCheck>,
    /// A1：共用行情匯流（主視窗建立，其他視窗共用）
    hub: Rc<Hub>,
    /// A4：第一次被行情叫醒、還沒畫的時間
    wake_at: Option<Instant>,
    long_frames: u64,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl Terminal {
    /// `shared`＝None：主視窗（開行情連線、建報價表）；Some：XQ_WINDOWS 的其他視窗，共用主視窗的報價表 entity
    fn new(win: usize, shared: Option<(Entity<QuoteTable>, Arc<Counters>, Rc<Hub>)>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let primary = shared.is_none();
        let (sym, name) = names::WATCH[win % names::WATCH.len()];
        let tape_feed =
            (primary && crate::plat::var_os("XQ_TAPE").is_some_and(|v| v != "0")).then(|| TapeFeed::new(sym));
        let tape = tape_feed.clone().map(|f| cx.new(|cx| TickTape::new(f, sym, cx)));
        let (bridge, wake_rx, table, counters, hub) = match shared {
            None => {
                let (bridge, wake_rx) = FeedBridge::start(tape_feed);
                let counters = bridge.counters.clone();
                let hub = Rc::new(Hub { taker: bridge.taker(), tape: tape.clone(), queues: RefCell::new(Vec::new()), conn: Cell::new(None) });
                (Some(bridge), Some(wake_rx), cx.new(|cx| {
                    let mut t = QuoteTable::new(names::WATCH);
                    if crate::plat::search_enabled() {
                        t.attach_search(cx);
                    }
                    t
                }), counters, hub)
            }
            Some((table, counters, hub)) => (None, None, table, counters, hub),
        };
        hub.register(win);
        let window_id = window.window_handle().window_id();
        // XQ_PERIOD=D：啟動就開日線（基準測試用，和 wry 版同畫面）；預設分時
        let period = crate::plat::var("XQ_PERIOD").ok().and_then(|v| Period::from_code(&v)).unwrap_or(Period::Intraday);
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
        let render_apply = crate::plat::var("XQ_APPLY").as_deref() != Ok("timer");
        let feed_task = wake_rx.map(|mut wake_rx| cx.spawn(async move |this, cx| {
            while wake_rx.next().await.is_some() {
                beat(3);
                // A1：一到就叫全部視窗畫（不是只叫主視窗、等它畫完再轉手）
                let r = this.update(cx, |t, cx| if t.render_apply { t.notify_all(cx) } else { t.apply_feed(cx) });
                if r.is_err() {
                    break;
                }
                if !render_apply {
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                }
            }
        }));
        // 後備：這個視窗被蓋住／縮小（GPUI 停掉它的刷新）時，由計時器代為取件／套用自己的佇列（每個視窗各一個）
        let fallback_task = render_apply.then(|| cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(FALLBACK_MS)).await;
            beat(5);
            let r = this.update(cx, |t, cx| {
                let stale = t.last_render.is_none_or(|r| r.elapsed() >= Duration::from_millis(FALLBACK_MS));
                if stale && (t.hub.taker.pending() || t.hub.has_queued(t.win)) {
                    t.fallback_applies += 1;
                    t.pump(cx);
                } else if t.tick.as_ref().is_some_and(|tb| tb.awaiting_older_than(FALLBACK_MS * 1000)) {
                    // 量測用：最後一批報價畫好後沒有下一幀，叫一次重畫把 present 記在真的下一幀（見 TickBench::awaiting_older_than）
                    t.probe_kicks += 1;
                    cx.notify();
                }
            });
            if r.is_err() {
                break;
            }
        }));
        // 每秒取樣 FPS / RSS（不跟 tick 綁）
        let stats_task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            beat(4);
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
            top_frac: crate::plat::var("XQ_TOP_FRAC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.56),
            left_w: 470.0,
            drag: None,
            frames: 0,
            applies: 0,
            apply_us: 0,
            last_frames_in: 0,
            last_rows_in: 0,
            last_paints: 0,
            last_paint_us: 0,
            last_cpu: None,
            stats: Stats::default(),
            metrics_log: crate::plat::var_os("XQ_METRICS").is_some(),
            no_cache: crate::plat::var_os("XQ_NO_VIEW_CACHE").is_some(),
            bench: crate::plat::var_os("XQ_BENCH").is_some_and(|v| v != "0"),
            bench_frame_logged: false,
            bench_quote_logged: false,
            total_frames: 0,
            track_jank: false,
            last_frame: None,
            jank_n: 0,
            jank_ms: 0.0,
            frame_ms: Vec::new(),
            scroll_drive: None,
            tick: crate::plat::tick_bench_enabled().then(|| tickbench::TickBench::for_window(win)),
            render_apply,
            last_render: None,
            fallback_applies: 0,
            probe_kicks: 0,
            sync: (crate::plat::var_os("XQ_SYNC_CHECK").is_some_and(|v| v != "0")).then(SyncCheck::default),
            hub,
            wake_at: None,
            long_frames: 0,
            _tasks: feed_task.into_iter().chain(fallback_task).chain([stats_task]).collect(),
            _subs: subs,
        };
        if !primary {
            t.adopt_shared_state(cx);
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
        if !crate::plat::var_os("XQ_GROUP_STRESS").is_some_and(|v| v != "0") {
            return None;
        }
        let num = |k: &str, d: u64| crate::plat::var(k).ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).unwrap_or(d);
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

            let s0 = Instant::now();
            let frame_driven = scroll_driver_frame();
            let _ = this.update(cx, |t, cx| {
                t.track_jank = true;
                t.last_frame = None;
                t.jank_n = 0;
                t.jank_ms = 0.0;
                t.frame_ms = Vec::with_capacity(FRAME_SAMPLES);
                if frame_driven {
                    t.scroll_drive = Some(ScrollDrive { s0, secs: scroll_ms as f64 / 1000.0, last_tick: s0, batch, rng, updates: 0 });
                    cx.notify();
                }
            });
            let mut updates = 0u64;
            if frame_driven {
                // 位置每幀由 render 推進；這裡只等時間到（結束時間差一個計時器刻度沒關係）
                cx.background_executor().timer(Duration::from_millis(scroll_ms)).await;
            } else {
                // 舊做法（XQ_SCROLL_DRIVER=timer）：背景計時器每 16 ms 推一次。Windows 計時器刻度約 15.6 ms，
                // 實際常變成 31 ms 才推一次，報價表每秒只更新約 42 次，畫面看起來就是一頓一頓
                let mut last_tick = Instant::now();
                while s0.elapsed() < Duration::from_millis(scroll_ms) {
                    let frac = scroll_pos(s0.elapsed().as_secs_f64() / (scroll_ms as f64 / 1000.0));
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
                    updates += 1;
                    cx.background_executor().timer(Duration::from_millis(16)).await;
                }
            }
            let secs = s0.elapsed().as_secs_f64();
            let Ok(c) = this.update(cx, |t, cx| {
                t.track_jank = false;
                if let Some(d) = t.scroll_drive.take() {
                    updates = d.updates;
                }
                let ft = frame_stats(&mut t.frame_ms);
                t.frame_ms = Vec::new();
                (snap(t, cx), t.jank_n, t.jank_ms, ft)
            }) else {
                return;
            };
            let (c, jn, jms, ft) = c;
            eprintln!(
                "GROUPPERF|{{\"phase\":\"done\",\"rows\":{},\"cols\":6,\"tickFps\":{:.1},\"scrollFps\":{:.1},\"scrollJankCount\":{jn},\"scrollJankAvgMs\":{:.1},\"scrollDriver\":\"{}\",\"scrollUpdatesPerSec\":{:.1},{ft},{}}}",
                c.4,
                (b.0 - a.0) as f64 / (tick_ms as f64 / 1000.0),
                (c.0 - b.0) as f64 / secs,
                if jn > 0 { jms / jn as f64 } else { 0.0 },
                if frame_driven { "frame" } else { "timer" },
                updates as f64 / secs,
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

    /// 後開的視窗：接手主視窗已經收到的連線狀態，並從共用報價表補上自己走勢圖／資訊格那一檔的現價
    /// （這些在它註冊進 Hub 之前就被取走了，不會再出現在它的佇列裡）
    fn adopt_shared_state(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.hub.conn.get() {
            self.conn = c;
        }
        let sym = self.chart.read(cx).symbol();
        let Some((price, change, volume, name)) =
            self.table.read(cx).row(&sym).filter(|r| r.price > 0.0).map(|r| (r.price, r.change, r.volume, r.name.to_string()))
        else {
            return;
        };
        self.chart.update(cx, |c, cx| c.apply_quote(price, change, volume, &name, cx));
        self.info.update(cx, |i, _| {
            i.price = price;
            i.change = change;
            i.volume = volume;
        });
    }

    /// 行情一到：叫全部視窗畫（每個視窗在自己的 render 開頭 pump）
    fn notify_all(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.wake_at.get_or_insert(now);
        cx.notify();
        if !self.peers.is_empty() {
            self.peers.retain(|p| p.upgrade().is_some());
            for p in self.peers.clone() {
                let _ = p.update(cx, |p, cx| {
                    p.wake_at.get_or_insert(now);
                    cx.notify()
                });
            }
        }
    }

    /// 有新行情就取件（只會有一個視窗取到）：合併進共用報價表＋成交明細，套用結果放進每個視窗的佇列；
    /// 再把這個視窗自己的佇列套用完。
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.hub.taker.pending() {
            let t0 = Instant::now();
            let ib = self.hub.taker.take();
            // 報價表只合併一次（多視窗共用同一個 entity，notify 後每個畫它的視窗各自重畫可見列）
            let changed = if ib.quotes.is_empty() {
                0
            } else {
                self.table.update(cx, |t, cx| t.apply(&ib.order, &ib.quotes, cx))
            };
            if let Some(tape) = &self.hub.tape {
                // 和報價表同一個節奏（每幀最多一次）；沒有新成交就不 notify
                tape.update(cx, |t, cx| t.apply(cx));
            }
            if let Some(c) = ib.conn {
                self.hub.conn.set(Some(c));
            }
            if !ib.quotes.is_empty() {
                self.hub.conn.set(Some(Conn::Ready));
            }
            let a = Rc::new(Applied { ib, changed, us: t0.elapsed().as_micros() as u64 });
            for (_, q) in self.hub.queues.borrow_mut().iter_mut() {
                q.push(a.clone());
            }
        }
        self.drain(cx);
    }

    fn drain(&mut self, cx: &mut Context<Self>) {
        for a in self.hub.drain(self.win) {
            let t1 = Instant::now();
            self.apply_local(&a.ib, cx);
            let us = a.us + t1.elapsed().as_micros() as u64;
            self.finish_apply(&a.ib, a.changed, us, cx);
        }
    }

    /// XQ_APPLY=timer（舊做法對照）：喚醒就套用，主視窗取件後直接套到全部視窗
    fn apply_feed(&mut self, cx: &mut Context<Self>) {
        self.pump(cx);
        if !self.peers.is_empty() {
            self.peers.retain(|p| p.upgrade().is_some());
            for p in self.peers.clone() {
                let _ = p.update(cx, |p, cx| p.drain(cx));
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
        // 自我測試卡頓偵測：XQ_STALL_TEST_MS=N → 第 5 次每秒統計時讓主執行緒睡 N ms（只做一次）
        #[cfg(not(target_arch = "wasm32"))]
        {
            static N: AtomicU64 = AtomicU64::new(0);
            if self.win == 0 && N.fetch_add(1, Ordering::Relaxed) == 4 {
                if let Some(ms) = crate::plat::var("XQ_STALL_TEST_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
                    std::thread::sleep(Duration::from_millis(ms));
                }
            }
        }
        let frames_in = self.counters.frames.load(Ordering::Relaxed);
        let rows_in = self.counters.quote_rows.load(Ordering::Relaxed);
        let (paints, paint_us) = {
            self.chart.read(cx).paint_totals()
        };
        let dp = paints - self.last_paints;
        let now = Instant::now();
        let cpu = cpu_secs();
        let (cpu_main, cpu_proc) = match (cpu, self.last_cpu) {
            (Some((m1, p1)), Some((t0, m0, p0))) => {
                let el = now.duration_since(t0).as_secs_f64().max(1e-3);
                ((m1 - m0) / el * 100.0, (p1 - p0) / el * 100.0)
            }
            _ => (-1.0, -1.0),
        };
        self.last_cpu = cpu.map(|(m, p)| (now, m, p));
        self.stats = Stats {
            fps: self.frames,
            rss_mb: rss_mb(),
            frames_in: frames_in - self.last_frames_in,
            quote_rows_in: rows_in - self.last_rows_in,
            applies: self.applies,
            apply_us_avg: if self.applies > 0 { self.apply_us / self.applies as u64 } else { 0 },
            chart_paints: dp,
            chart_paint_us_avg: if dp > 0 { (paint_us - self.last_paint_us) / dp } else { 0 },
            cpu_main,
            cpu_proc,
        };
        if self.metrics_log {
            let s = &self.stats;
            eprintln!(
                "[metrics] fps={} rss_mb={:.1} feed_frames/s={} quote_rows/s={} applies/s={} apply_us_avg={} chart_paints/s={} chart_paint_us_avg={} cpu_main={:.1} cpu_proc={:.1} conn={:?}",
                s.fps, s.rss_mb, s.frames_in, s.quote_rows_in, s.applies, s.apply_us_avg, s.chart_paints, s.chart_paint_us_avg, s.cpu_main, s.cpu_proc, self.conn
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
        let now = crate::plat::unix_us();
        let sym = self.chart.read(cx).symbol();
        let shown = [self.info.read(cx).shown_price, self.chart.read(cx).shown_price()];
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
            self.win, crate::plat::unix_us(), sc.frames, sc.checks, sc.mismatches, sc.violations, sc.max_stale_us as f64 / 1000.0, self.fallback_applies
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
                let avail = f32::from(vp.height) - title_h() - status_h() - split();
                let y = f32::from(ev.position.y) - title_h();
                self.top_frac = (y / avail).clamp(0.15, 0.85);
            }
            Drag::Vertical => {
                self.left_w = f32::from(ev.position.x).clamp(200.0, f32::from(vp.width) - 200.0);
            }
        }
        cx.notify();
    }
}

/// 2026-10-25：CPU 拆解用——（這條執行緒、整個程序）累計 CPU 秒數。在 sample()（主執行緒、每秒一次）呼叫，
/// metrics 行印 cpu_main／cpu_proc（100%＝一顆核心）：看得出多的 CPU 是主執行緒（GPUI＋我們）還是驅動／其他執行緒。
/// Windows：GetThreadTimes／GetProcessTimes；Linux：/proc/thread-self/stat、/proc/self/stat；其他平台回 None。
#[cfg(windows)]
pub fn cpu_secs() -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes};
    let ft = |f: &FILETIME| (((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64) as f64 / 1e7;
    unsafe {
        let z = || FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z(), z(), z(), z());
        if GetThreadTimes(GetCurrentThread(), &mut c, &mut e, &mut k, &mut u) == 0 {
            return None;
        }
        let th = ft(&k) + ft(&u);
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) == 0 {
            return None;
        }
        Some((th, ft(&k) + ft(&u)))
    }
}

#[cfg(target_os = "linux")]
pub fn cpu_secs() -> Option<(f64, f64)> {
    // stat 第 14、15 欄 utime／stime（clock ticks，Linux 一律 100 Hz）；comm 可能含空白，從最後一個 ')' 之後切
    let ticks = |path: &str| -> Option<f64> {
        let s = std::fs::read_to_string(path).ok()?;
        let rest = s.get(s.rfind(')')? + 2..)?;
        let f: Vec<&str> = rest.split_whitespace().collect();
        Some((f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?) / 100.0)
    };
    Some((ticks("/proc/thread-self/stat")?, ticks("/proc/self/stat")?))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn cpu_secs() -> Option<(f64, f64)> {
    None
}

/// Windows：目前程序的工作集（Working Set，相當於 RSS）
#[cfg(windows)]
pub fn rss_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            c.WorkingSetSize as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}

/// 2026-10-25：記憶體拆解用——配置器（malloc）實際在用的量與向系統要來的量（MB），印在 CHARTPERF 行。
/// RSS 減掉這個＝程式碼／字型檔對映＋GPU 驅動（Metal／wgpu 的緩衝區與貼圖）＋其他。
/// macOS：malloc_zone_statistics（全部 zone）；Linux（glibc）：mallinfo2；其他平台不印。
pub fn heap_kv() -> String {
    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        #[derive(Default)]
        struct MallocStats { blocks_in_use: u32, size_in_use: usize, max_size_in_use: usize, size_allocated: usize }
        extern "C" {
            fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStats);
        }
        let mut st = MallocStats::default();
        unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut st) };
        let mb = |v: usize| v as f64 / 1048576.0;
        return format!(
            ",\"heapUseMb\":{:.1},\"heapAllocMb\":{:.1},\"heapPeakMb\":{:.1}{}",
            mb(st.size_in_use),
            mb(st.size_allocated),
            mb(st.max_size_in_use),
            memrelief::kv()
        );
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        #[repr(C)]
        #[derive(Default)]
        struct Mallinfo2 { arena: usize, ordblks: usize, smblks: usize, hblks: usize, hblkhd: usize, usmblks: usize, fsmblks: usize, uordblks: usize, fordblks: usize, keepcost: usize }
        extern "C" {
            fn mallinfo2() -> Mallinfo2;
        }
        let m = unsafe { mallinfo2() };
        let mb = |v: usize| v as f64 / 1048576.0;
        return format!(",\"heapUseMb\":{:.1},\"heapAllocMb\":{:.1}", mb(m.uordblks + m.hblkhd), mb(m.arena + m.hblkhd));
    }
    #[allow(unreachable_code)]
    String::new()
}

#[cfg(target_arch = "wasm32")]
pub fn rss_mb() -> f64 {
    plat::wasm_heap_mb()
}

#[cfg(not(any(windows, target_arch = "wasm32")))]
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
                if self.frame_ms.len() < self.frame_ms.capacity() {
                    self.frame_ms.push(dt as f32);
                }
            }
        }
        // 捲動壓測：這一幀的位置由經過時間算（跟著畫面幀走，不靠計時器），再要下一幀
        if let Some(d) = self.scroll_drive.as_mut() {
            let frac = scroll_pos(d.s0.elapsed().as_secs_f64() / d.secs);
            let tick = d.last_tick.elapsed() >= Duration::from_millis(33);
            if tick {
                d.last_tick = Instant::now();
            }
            d.updates += 1;
            let (batch, rng) = (d.batch, &mut d.rng);
            self.table.update(cx, |tb, cx| {
                tb.scroll_frac(frac, cx);
                if tick {
                    tb.synth_ticks(batch, rng, cx);
                }
            });
            window.request_animation_frame();
        }
        beat(1);
        let r0 = Instant::now();
        let prev_render = self.last_render.replace(r0);
        let thr = longframe_ms();
        if let Some(w) = self.wake_at.take() {
            let wake_ms = r0.duration_since(w).as_secs_f64() * 1000.0;
            let gap_ms = prev_render.map(|p| r0.duration_since(p).as_secs_f64() * 1000.0);
            if wake_ms > thr {
                self.long_frames += 1;
                log_long(self.win, "wake", wake_ms, &format!(",\"gapMs\":{:.1},\"frames\":{},\"n\":{}", gap_ms.unwrap_or(-1.0), self.total_frames, self.long_frames));
            }
        }
        // 行情在這一幀開頭套用：子 view 在這之後才 prepaint，被 notify 的會在同一幀重畫
        if self.render_apply && (self.hub.taker.pending() || self.hub.has_queued(self.win)) {
            self.pump(cx);
            let ms = r0.elapsed().as_secs_f64() * 1000.0;
            if ms > thr {
                self.long_frames += 1;
                log_long(self.win, "apply", ms, &format!(",\"frames\":{},\"n\":{}", self.total_frames, self.long_frames));
            }
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
        let avail = f32::from(vp.height) - title_h() - status_h() - split();
        let top_h = (avail * self.top_frac).floor();
        let (conn_text, conn_color) = match self.conn {
            Conn::Connecting => ("連線中…", dim()),
            Conn::Up => ("已連線，等待行情", accent()),
            Conn::Ready => ("行情連線", down()),
            Conn::Down => ("行情中斷，重連中", up()),
        };
        let addr = crate::plat::feed_label();
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
            .font_family(ui_font())
            .text_size(px(tk().font.md))
            .text_color(text())
            .on_mouse_move(cx.listener(Self::on_drag))
            .on_any_mouse_down(|ev, _, _| {
                if crate::plat::var_os("XQ_DEBUG").is_some() {
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
                    .h(px(title_h()))
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
                    .h(px(split()))
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
                            .w(px(split()))
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
                        d.child(div().w(px(split())).h_full().bg(border()))
                            .child(div().w(px(tk().layout.tape_width)).h_full().flex_none().child(view(tape.into())))
                    }),
            )
            .child(
                div()
                    .h(px(status_h()))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .px_3()
                    .bg(header())
                    .border_t_1()
                    .border_color(border())
                    .text_size(px(tk().font.sm))
                    .child(div().text_color(conn_color).child(format!("● {conn_text} {addr}")))
                    .child(div().flex_1())
                    .child(div().font_family(mono_font()).child(format!("FPS {}", s.fps)))
                    .child(div().font_family(mono_font()).child(format!("{} {:.1} MB", if plat::IS_WEB { "WASM" } else { "RSS" }, s.rss_mb)))
                    .child(div().font_family(mono_font()).text_color(dim()).child(format!(
                        "封包 {}/s → 套用 {}/s",
                        s.frames_in, s.applies
                    ))),
            )
            // 卡頓偵測：最後一個子元素，paint 到這裡＝我們這一幀畫完（之後是 GPUI 送出／present）
            .child(gpui::canvas(|_, _, _| {}, |_, _, _, _| beat(2)).absolute().size_0())
    }
}

/// 捲動壓測一次最多記幾個幀間隔（60 fps × 2 分鐘多）
const FRAME_SAMPLES: usize = 8192;

/// 捲動壓測（XQ_GROUP_STRESS）每幀推進用的狀態；跳價跟 egui 版一樣「這一幀距上次跳價 ≥ 33 ms 才跳」
struct ScrollDrive {
    s0: Instant,
    secs: f64,
    last_tick: Instant,
    batch: usize,
    rng: u64,
    /// 推進了幾次捲動位置（＝報價表收到幾個新位置）
    updates: u64,
}

/// 預設每幀推進；XQ_SCROLL_DRIVER=timer 切回舊的「背景每 16 ms 推一次」（A/B 對照用）
fn scroll_driver_frame() -> bool {
    crate::plat::var("XQ_SCROLL_DRIVER").as_deref() != Ok("timer")
}

/// 捲動位置：經過比例 f（0..1）→ 上下正弦兩趟（同 wry／egui 版）
fn scroll_pos(f: f64) -> f64 {
    (f * std::f64::consts::PI * 4.0).sin() * 0.5 + 0.5
}

/// 幀間隔分布（ms）→ GROUPPERF 的 JSON 片段；會就地排序
fn frame_stats(ms: &mut [f32]) -> String {
    ms.sort_unstable_by(f32::total_cmp);
    let q = |p: f64| -> f32 {
        if ms.is_empty() {
            return 0.0;
        }
        let i = ((ms.len() - 1) as f64 * p).round() as usize;
        ms.get(i).copied().unwrap_or(0.0)
    };
    format!(
        "\"frameMsP50\":{:.1},\"frameMsP95\":{:.1},\"frameMsP99\":{:.1},\"frameMsMax\":{:.1},\"frameSamples\":{}",
        q(0.5), q(0.95), q(0.99), ms.last().copied().unwrap_or(0.0), ms.len()
    )
}

/// XQ_WINDOWS=N（預設 1，最多 8）：開 N 個看盤視窗（各自走勢圖＝WATCH[k]），
/// 共用同一條行情連線、同一個報價表 entity（同一份 5 萬列資料，只合併一次），每個視窗自己的 view／走勢圖。
fn window_count() -> usize {
    if plat::IS_WEB {
        return 1; // 瀏覽器只有一張 canvas：gpui_web 第二個視窗會回 WebWindowError::AlreadyOpen
    }
    crate::plat::var("XQ_WINDOWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 8)
}

/// 2026-10-25：GPUI 預設對「沒有焦點的視窗」限速（inactive_frame_interval＝33.3ms）：只要這個視窗有 on_next_frame
/// 回呼（重圖表壓測、動畫、拖曳中的每幀更新）就最多約 30 fps，60Hz 下實際常卡在 20–23 fps。看盤多視窗要每個視窗都即時，
/// 所以全部視窗都關掉（None）。XQ_INACTIVE_THROTTLE=1 恢復 GPUI 預設（A/B 對照用）。
fn inactive_frame_interval() -> Option<Duration> {
    if crate::plat::var("XQ_INACTIVE_THROTTLE").as_deref() == Ok("1") {
        WindowOptions::default().inactive_frame_interval
    } else {
        None
    }
}

fn window_options(k: usize, cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(1240.0), px(760.0)), cx);
    let title = if k == 0 { "XQ 風格測試（GPUI）".to_string() } else { format!("XQ 風格測試（GPUI） #{}", k + 1) };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
        inactive_frame_interval: inactive_frame_interval(),
        ..Default::default()
    }
}

gpui::actions!(xq_theme, [ToggleTheme]);

fn run(app: gpui::Application) {
    app.run(|cx: &mut App| {
        #[cfg(target_arch = "wasm32")]
        web_fonts(cx);
        theme::resolve_fonts(cx);
        tick_tape::bind_keys(cx);
        search::bind_keys(cx);
        // 設計 token：F8 切換深色／淺色；refresh_windows 會略過 view 快取整個重畫
        cx.bind_keys([gpui::KeyBinding::new("f8", ToggleTheme, None)]);
        cx.on_action(|_: &ToggleTheme, cx: &mut App| {
            let m = xq_tokens::toggle_mode();
            eprintln!("[xq-tokens] 主題：{}", m.name());
            cx.refresh_windows();
        });
        // 熱更新（XQ_TOKENS_DIR＋debug 版或 XQ_TOKENS_WATCH=1）：背景執行緒重載 token，這裡每 300ms 看版本號，變了就整個重畫。
        // 沒開熱更新時不建這個 task，壓測數字不受影響。網頁版沒有檔案系統，watch() 一律回 false。
        if xq_tokens::watch() {
            cx.spawn(async move |cx| {
                let mut seen = xq_tokens::generation();
                loop {
                    cx.background_executor().timer(Duration::from_millis(300)).await;
                    let g = xq_tokens::generation();
                    if g != seen {
                        seen = g;
                        cx.update(|cx| cx.refresh_windows());
                    }
                }
            })
            .detach();
        }
        let opts = window_options(0, cx);
        let main = cx
            .open_window(opts, |window, cx| cx.new(|cx| Terminal::new(0, None, window, cx)))
            .expect("open main window");
        // 卡頓看門狗（src/stall.rs）：背景收行情執行緒的封包數用來分辨「只有主執行緒卡住」還是「整個程序被暫停」
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(p) = main.entity(cx) {
            let c = p.read(cx).counters.clone();
            stall::start(Arc::new(move || c.frames.load(Ordering::Relaxed)));
        }
        let n = window_count();
        if n > 1 {
            let primary = main.entity(cx).expect("main view");
            let (table, counters, hub) = {
                let p = primary.read(cx);
                (p.table.clone(), p.counters.clone(), p.hub.clone())
            };
            for k in 1..n {
                let shared = (table.clone(), counters.clone(), hub.clone());
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

/// 設定檔根目錄：Windows %APPDATA%；其他 $XDG_DATA_HOME 或 ~/.local/share
pub fn data_dir() -> Option<std::path::PathBuf> {
    let env = |k: &str| crate::plat::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    #[cfg(windows)]
    let base = env("APPDATA").or_else(|| env("LOCALAPPDATA"));
    #[cfg(not(windows))]
    let base = env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")));
    base
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // macOS：先確定 malloc 的大塊快取已關（可能 exec 自己一次，見 memrelief.rs），再做任何事
    memrelief::ensure_no_large_cache();
    gpuilog::init();
    #[cfg(target_os = "macos")]
    macos::begin_latency_critical_activity();
    run(gpui_platform::application());
}

/// 網頁版字型：瀏覽器拿不到系統的 Noto Sans CJK，跟 egui 網頁版編同一份子集進 wasm（Big5 一級字＋標點＋注音，約 3 MB）。
/// 字型家族名稱跟原生一樣（"Noto Sans CJK TC" / "Noto Sans Mono CJK TC"），theme.rs 不用改。
#[cfg(target_arch = "wasm32")]
fn web_fonts(cx: &mut App) {
    static TC: &[u8] = include_bytes!("../web/fonts/NotoSansTC-subset.otf");
    static MONO: &[u8] = include_bytes!("../web/fonts/NotoSansMonoTC-ascii.otf");
    if let Err(e) = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(TC), std::borrow::Cow::Borrowed(MONO)]) {
        eprintln!("[fonts] add_fonts 失敗: {e:?}");
    }
}

/// 網頁版入口：gpui_web 單執行緒模式（多執行緒模式要 nightly＋atomics＋SharedArrayBuffer，見 README「網頁版」）
#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    gpui_web::init_logging();
    eprintln!("BENCH wasm-start");
    let platform = std::rc::Rc::new(gpui_web::WebPlatform::new(false));
    run(gpui::Application::with_platform(platform));
}

#[cfg(test)]
mod tests {
    use super::{frame_stats, scroll_pos};

    #[test]
    fn frame_stats_percentiles() {
        let mut ms: Vec<f32> = (1..=100).map(|v| v as f32).collect();
        ms.reverse();
        let s = frame_stats(&mut ms);
        assert_eq!(s, "\"frameMsP50\":51.0,\"frameMsP95\":95.0,\"frameMsP99\":99.0,\"frameMsMax\":100.0,\"frameSamples\":100");
        assert_eq!(frame_stats(&mut []), "\"frameMsP50\":0.0,\"frameMsP95\":0.0,\"frameMsP99\":0.0,\"frameMsMax\":0.0,\"frameSamples\":0");
    }

    #[test]
    fn scroll_pos_two_round_trips() {
        assert!((scroll_pos(0.0) - 0.5).abs() < 1e-9);
        assert!((scroll_pos(0.125) - 1.0).abs() < 1e-9);
        assert!(scroll_pos(0.375).abs() < 1e-9);
        assert!((0.0..=1.0).contains(&scroll_pos(0.7)));
    }
}
