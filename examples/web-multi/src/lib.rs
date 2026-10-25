//! 可行性範例：同一個 html 頁面嵌 4 個 GPUI 元件（對照 egui-xq-demo 的 examples/web-multi）。
//!
//! - gpui_web 的 `WebPlatform` 一個 Application 只能開一個視窗（再開回 `AlreadyOpen`），而且 canvas 由它自己建、掛在 body 上，
//!   所以**每格各自一個 wasm instance**（同一份 wasm 只下載、編譯一次），JS 再把它建的 canvas 搬進格子裡。
//! - 報價表、走勢圖直接用桌面版 `../../src/*.rs`（#[path]），程式碼不改。
//! - 行情：每格自己跑一份合成行情，規則同 `WryFeedHost --demo-ticks`／`xq-demofeed`（每 25 ms 每檔 ±0.15% 隨機走、量加 10–400），
//!   **不是真實行情**，不接 WebSocket。
//! - JS 端：`start(kind)`、`frames()`、`panic_next_frame()`、`destroy()`、`wasm_memory_bytes()`（一個 instance 一個元件，所以都是模組層級函式）。
#![cfg(target_arch = "wasm32")]
#![allow(dead_code)]

#[path = "../../../src/chart.rs"]
mod chart;
#[path = "../../../src/drawings.rs"]
mod drawings;
#[path = "../../../src/gfx.rs"]
mod gfx;
#[path = "../../../src/indicators.rs"]
mod indicators;
#[path = "../../../src/layers.rs"]
mod layers;
#[path = "../../../src/model.rs"]
mod model;
#[path = "../../../src/names.rs"]
mod names;
#[path = "../../../src/plat.rs"]
mod plat;
#[path = "../../../src/search.rs"]
mod search;
#[path = "../../../src/series.rs"]
mod series;
#[path = "../../../src/store.rs"]
mod store;
#[path = "../../../src/table.rs"]
mod table;
#[path = "../../../src/theme.rs"]
mod theme;
#[path = "../../../src/zstress.rs"]
mod zstress;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::{div, prelude::*, px, AnyWindowHandle, App, Context, Entity, Window, WindowOptions};
use wasm_bindgen::prelude::*;
use xq_feed::Quote;

use crate::plat::Instant;
use chart::ChartView;
use series::Period;
use table::QuoteTable;
use theme::*;

/// chart.rs 壓測紀錄會呼叫（同 main.rs 的網頁版）
pub fn rss_mb() -> f64 {
    plat::wasm_heap_mb()
}
pub fn heap_kv() -> String {
    String::new()
}

/// 已畫的幀數（chart.rs 壓測紀錄也讀這個，同 main.rs）
pub static FRAMES: AtomicU64 = AtomicU64::new(0);

const TABLE_ROWS: usize = 50;
const STEP: Duration = Duration::from_millis(25);

// 一個 instance 只有一個元件，狀態放模組層級
thread_local! {
    static PANIC: Cell<bool> = const { Cell::new(false) };
    static DESTROY: Cell<bool> = const { Cell::new(false) };
    static HANDLE: RefCell<Option<AnyWindowHandle>> = const { RefCell::new(None) };
}

// ───────────────────────── 合成行情（xq-demofeed 同一套規則） ─────────────────────────

struct Sym {
    symbol: String,
    prev: f64,
    price: f64,
    volume: i64,
}

struct DemoFeed {
    syms: Vec<Sym>,
    rng: u64,
    next: Instant,
    order: Vec<String>,
    quotes: HashMap<String, Quote>,
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl DemoFeed {
    fn new(list: &[(String, String)], seed: u64) -> Self {
        let syms: Vec<Sym> = list
            .iter()
            .map(|(s, _)| {
                let h = fnv(s);
                let prev = 50.0 + (h % 900) as f64;
                Sym { symbol: s.clone(), prev, price: prev, volume: (h % 100_000) as i64 }
            })
            .collect();
        let order = list.iter().map(|(s, _)| s.clone()).collect();
        let quotes = list
            .iter()
            .zip(&syms)
            .map(|((s, n), st)| (s.clone(), Quote { symbol: s.clone(), name: n.clone(), price: st.price, change: 0.0, volume: st.volume }))
            .collect();
        DemoFeed { syms, rng: seed | 1, next: Instant::now(), order, quotes }
    }

    fn unit(&mut self) -> f64 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 走完到現在為止該走的 25 ms 步（同代號合併，只留最新）；回傳是否有新報價
    fn poll(&mut self) -> bool {
        let now = Instant::now();
        let mut n = 0u32;
        while self.next <= now && n < 40 {
            for i in 0..self.syms.len() {
                let (a, b) = (self.unit(), self.unit());
                let s = &mut self.syms[i];
                let wobble = (a - 0.48) * (s.price * 0.0015).max(0.5);
                s.price = (s.price + wobble).max(0.01);
                s.volume += 10 + (b * 390.0) as i64;
            }
            self.next += STEP;
            n += 1;
        }
        if self.next <= now {
            // 分頁在背景被節流很久：不補跑，直接跳到現在
            self.next = now + STEP;
        }
        if n > 0 {
            for s in &self.syms {
                if let Some(q) = self.quotes.get_mut(&s.symbol) {
                    q.price = (s.price * 100.0).round() / 100.0;
                    q.change = ((s.price - s.prev) * 100.0).round() / 100.0;
                    q.volume = s.volume;
                }
            }
        }
        n > 0
    }
}

// ───────────────────────── 元件 ─────────────────────────

enum Body {
    Table(Entity<QuoteTable>),
    Chart(Entity<ChartView>),
    Counter,
}

struct Widget {
    kind: String,
    body: Body,
    feed: Option<DemoFeed>,
}

impl Widget {
    fn new(kind: &str, cx: &mut Context<Self>) -> Self {
        let seed = fnv(kind) ^ (js_sys::Math::random() * 1e15) as u64;
        let (body, feed) = match kind {
            "table" => {
                // 50 列：預設自選 33 檔＋補到 50 檔（合成代號）
                let mut list: Vec<(String, String)> = names::WATCH.iter().map(|(s, n)| ((*s).to_string(), (*n).to_string())).collect();
                let mut k = 0;
                while list.len() < TABLE_ROWS {
                    list.push((format!("9{k:03}"), format!("模擬{k:02}")));
                    k += 1;
                }
                list.truncate(TABLE_ROWS);
                let refs: Vec<(&str, &str)> = list.iter().map(|(s, n)| (s.as_str(), n.as_str())).collect();
                let t = cx.new(|_| QuoteTable::new(&refs));
                (Body::Table(t), Some(DemoFeed::new(&list, seed)))
            }
            "chart" => {
                let (sym, name) = names::WATCH[0];
                let c = cx.new(|cx| ChartView::new(sym, name, Period::Daily, cx));
                (Body::Chart(c), Some(DemoFeed::new(&[(sym.to_string(), name.to_string())], seed)))
            }
            _ => (Body::Counter, None),
        };
        if feed.is_some() {
            // 行情節奏：每 25 ms 醒來一次，把累積的步合併成一包再套用（每次最多一次 apply）
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(STEP).await;
                if this.update(cx, |w, cx| w.tick(cx)).is_err() {
                    break;
                }
            })
            .detach();
        }
        Widget { kind: kind.to_string(), body, feed }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let Some(feed) = self.feed.as_mut() else { return };
        if !feed.poll() {
            return;
        }
        match &self.body {
            Body::Table(t) => {
                t.update(cx, |t, cx| t.apply(&feed.order, &feed.quotes, cx));
            }
            Body::Chart(c) => {
                let sym = c.read(cx).symbol();
                if let Some(q) = feed.quotes.get(&sym) {
                    let (p, ch, v, n) = (q.price, q.change, q.volume, q.name.clone());
                    c.update(cx, |c, cx| c.apply_quote(p, ch, v, &n, cx));
                }
            }
            Body::Counter => {}
        }
        // 外框（Widget）本身也重畫一次，讓幀數＝這個元件實際畫的次數
        cx.notify();
    }
}

impl Render for Widget {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if PANIC.with(|p| p.replace(false)) {
            panic!("[{}] 驗證用：故意 panic", self.kind);
        }
        let n = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        let root = div().size_full().font_family(ui_font()).bg(bg()).text_color(text());
        match &self.body {
            Body::Table(t) => root.child(t.clone()),
            Body::Chart(c) => root.child(c.clone()),
            Body::Counter => {
                window.request_animation_frame();
                let w = (n % 120) as f32 / 120.0;
                root.flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(div().text_size(px(48.)).font_family(mono_font()).text_color(up()).child(format!("{n}")))
                    .child(div().text_size(px(14.)).text_color(dim()).child("每幀 +1"))
                    .child(div().absolute().bottom_0().left_0().h(px(4.)).w(gpui::relative(w)).bg(accent()))
            }
        }
    }
}

// ───────────────────────── JS 介面 ─────────────────────────

/// 網頁版字型（同 main.rs 的 web_fonts）：Noto Sans TC 子集＋等寬 ASCII 子集，編進 wasm
fn web_fonts(cx: &mut App) {
    static TC: &[u8] = include_bytes!("../../../web/fonts/NotoSansTC-subset.otf");
    static MONO: &[u8] = include_bytes!("../../../web/fonts/NotoSansMonoTC-ascii.otf");
    if let Err(e) = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(TC), std::borrow::Cow::Borrowed(MONO)]) {
        web_sys::console::error_1(&format!("[fonts] add_fonts 失敗: {e:?}").into());
    }
}

/// 啟動這個 instance 的元件。kind：`table`（報價表 50 列）｜`chart`（K 線，日K）｜`counter`（計數器）。
/// gpui_web 會自己建一張 canvas 掛在 body 最後面；畫出第一幀後（`frames() > 0`）由 JS 搬進格子裡。
#[wasm_bindgen]
pub fn start(kind: String) {
    console_error_panic_hook::set_once();
    // 網址參數 XQ_WEB_BACKEND=webgl|webgpu 可強制繪圖後端（預設 Auto：WebGPU 優先、失敗退回 WebGL2）
    let backend = match plat::var("XQ_WEB_BACKEND").as_deref() {
        Ok("webgl") => gpui_web::WebBackendPreference::WebGl,
        Ok("webgpu") => gpui_web::WebBackendPreference::WebGpu,
        _ => gpui_web::WebBackendPreference::Auto,
    };
    let platform = std::rc::Rc::new(gpui_web::WebPlatform::new_with_backend(false, backend));
    gpui::Application::with_platform(platform).run(move |cx: &mut App| {
        web_fonts(cx);
        theme::resolve_fonts(cx);
        let opts = WindowOptions::default();
        match cx.open_window(opts, |_, cx| cx.new(|cx| Widget::new(&kind, cx))) {
            Ok(h) => {
                HANDLE.with(|s| s.replace(Some(h.into())));
            }
            Err(e) => web_sys::console::error_1(&format!("[{kind}] open_window 失敗: {e:?}").into()),
        }
        // destroy()：JS 只能設旗標，這裡每 50 ms 看一次，真的關視窗要在 App 的 context 裡做
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if DESTROY.with(|d| d.get()) {
                if let Some(h) = HANDLE.with(|s| s.borrow_mut().take()) {
                    let _ = cx.update_window(h, |_, window, _| window.remove_window());
                }
                break;
            }
        })
        .detach();
    });
}

/// 已畫的幀數（計數器的數字就是這個）
#[wasm_bindgen]
pub fn frames() -> f64 {
    FRAMES.load(Ordering::Relaxed) as f64
}

/// 下一幀故意 panic（驗證一個元件 panic 會不會拖垮其他元件）
#[wasm_bindgen]
pub fn panic_next_frame() {
    PANIC.with(|p| p.set(true));
}

/// 關掉這個元件的視窗（gpui 的 remove_window）
#[wasm_bindgen]
pub fn destroy() {
    DESTROY.with(|d| d.set(true));
}

/// 這個 wasm instance 的線性記憶體大小（bytes）
#[wasm_bindgen]
pub fn wasm_memory_bytes() -> f64 {
    plat::wasm_heap_mb() * 1048576.0
}
