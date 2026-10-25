//! 報價表：uniform_list 虛擬化，只畫可見列；行情只改有變的列，
//! 字串在 render 時只為可見列格式化（5 萬列也不預先建字串）。

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    canvas, deferred, div, font, point, prelude::*, px, uniform_list, App, Bounds, Context, ContentMask, Entity, EventEmitter,
    Hsla, MouseButton, Pixels, ScrollStrategy, ShapedLine, SharedString, TextAlign, TextRun, UniformListScrollHandle, Window,
};
use xq_feed::Quote;

use crate::search::{find_matches, SearchEvent, SearchInput};
use crate::theme::*;

pub fn row_h() -> f32 { xq_tokens::t().layout.table_row }
const COLS: [(&str, f32); 6] = [("代號", 58.0), ("名稱", 92.0), ("成交", 76.0), ("漲跌", 70.0), ("幅度", 64.0), ("總量", 92.0)];
pub const STRESS_ROWS: usize = 50_000;

pub struct Row {
    pub symbol: SharedString,
    pub name: SharedString,
    pub price: f64,
    pub change: f64,
    pub volume: i64,
    /// 上一筆 tick 方向（1 / -1 / 0），成交價底色提示
    pub tick: i8,
    /// 最後一次被行情改到的時間（Unix µs；XQ_SYNC_CHECK 算畫面落後多久用，每批只取一次時間）
    pub changed_us: u64,
}

pub enum TableEvent {
    /// 第二個欄位：點選發生在哪個視窗（多視窗共用同一個報價表 entity，各視窗只切自己的走勢圖）
    Select(SharedString, Option<gpui::WindowId>),
}

pub struct QuoteTable {
    rows: Vec<Row>,
    index: HashMap<SharedString, usize>,
    real_len: usize,
    pub selected: usize,
    scroll: UniformListScrollHandle,
    pub stress: bool,
    /// 壓測量測：可見列更新次數／累積耗時（µs，含量測呼叫）／建立的格數（列×6 欄）
    pub paints: u64,
    pub paint_us: u64,
    pub cells: u64,
    /// 股名／代號搜尋（IME 測試）：建議清單只在查詢字串改變時算一次
    search: Option<Entity<SearchInput>>,
    matches: Vec<usize>,
    match_sel: usize,
}

impl EventEmitter<TableEvent> for QuoteTable {}

impl QuoteTable {
    pub fn new(watch: &[(&str, &str)]) -> Self {
        let rows: Vec<Row> = watch
            .iter()
            .map(|(s, n)| Row { symbol: (*s).into(), name: (*n).into(), price: 0.0, change: 0.0, volume: 0, tick: 0, changed_us: 0 })
            .collect();
        let index = rows.iter().enumerate().map(|(i, r)| (r.symbol.clone(), i)).collect();
        QuoteTable { real_len: rows.len(), rows, index, selected: 0, scroll: UniformListScrollHandle::new(), stress: false, paints: 0, paint_us: 0, cells: 0, search: None, matches: Vec::new(), match_sel: 0 }
    }

    /// 建立搜尋框並訂閱它的事件（QuoteTable::new 沒有 cx，所以另外呼叫）
    pub fn attach_search(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(SearchInput::new);
        cx.subscribe(&input, |this: &mut Self, input, ev: &SearchEvent, cx| match ev {
            SearchEvent::Changed(q) => {
                this.matches = find_matches(this.rows.iter().enumerate().map(|(i, r)| (i, r.symbol.as_ref(), r.name.as_ref())), q, 8);
                this.match_sel = 0;
                cx.notify();
            }
            SearchEvent::Move(d) => {
                if !this.matches.is_empty() {
                    let n = this.matches.len() as i32;
                    this.match_sel = ((this.match_sel as i32 + d).rem_euclid(n)) as usize;
                    cx.notify();
                }
            }
            SearchEvent::Submit => {
                if let Some(&ix) = this.matches.get(this.match_sel) {
                    this.pick(ix, &input, cx);
                }
            }
            SearchEvent::Cancel => input.update(cx, |i, cx| i.clear(cx)),
        })
        .detach();
        self.search = Some(input);
    }

    fn pick(&mut self, ix: usize, input: &Entity<SearchInput>, cx: &mut Context<Self>) {
        self.matches.clear();
        input.update(cx, |i, cx| i.clear(cx));
        self.select(ix, None, cx);
    }

    pub fn symbols(&self) -> Vec<String> {
        self.rows[..self.real_len].iter().map(|r| r.symbol.to_string()).collect()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, symbol: &str) -> Option<&Row> {
        self.index.get(symbol).map(|&i| &self.rows[i])
    }

    /// 只碰有變的列；有變才 notify。回傳變動列數。
    pub fn apply(&mut self, order: &[String], quotes: &HashMap<String, Quote>, cx: &mut Context<Self>) -> usize {
        let mut changed = 0;
        let now = crate::plat::unix_us();
        for sym in order {
            let Some(q) = quotes.get(sym) else { continue };
            let Some(&i) = self.index.get(sym.as_str()) else { continue };
            let r = &mut self.rows[i];
            if r.price == q.price && r.change == q.change && r.volume == q.volume {
                continue;
            }
            r.tick = if r.price == 0.0 || q.price == r.price { r.tick } else if q.price > r.price { 1 } else { -1 };
            r.price = q.price;
            r.change = q.change;
            r.volume = q.volume;
            r.changed_us = now;
            if !q.name.is_empty() && q.name != q.symbol && r.name.as_ref() != q.name {
                r.name = q.name.clone().into();
            }
            changed += 1;
        }
        if changed > 0 {
            cx.notify();
        }
        changed
    }

    pub fn toggle_stress(&mut self, cx: &mut Context<Self>) {
        self.stress = !self.stress;
        self.rows.truncate(self.real_len);
        self.index.retain(|_, i| *i < self.real_len);
        if self.stress {
            self.rows.reserve_exact(STRESS_ROWS);
            for k in 0..STRESS_ROWS {
                // 合成靜態列：不訂閱、不 tick，只用來驗證虛擬化與記憶體
                let h = (k as u64).wrapping_mul(2654435761) % 100_000;
                let price = 10.0 + (h % 90_000) as f64 / 100.0;
                let change = ((h % 400) as f64 - 200.0) / 100.0;
                self.rows.push(Row {
                    symbol: format!("Z{k:05}").into(),
                    name: "壓力測試".into(),
                    price,
                    change,
                    volume: (h * 7) as i64,
                    tick: 0,
                    changed_us: 0,
                });
            }
        } else {
            self.rows.shrink_to_fit();
            if self.selected >= self.real_len {
                self.selected = 0;
            }
        }
        cx.notify();
    }

    /// XQ_TICK_BENCH=1：開 5 萬合成列並把 Z 代號放進索引，讓 engine 報價可以 O(1) 對到合成列。
    pub fn enable_stress_indexed(&mut self, cx: &mut Context<Self>) {
        if !self.stress {
            self.toggle_stress(cx);
        }
        self.index.reserve(self.rows.len() - self.real_len);
        for i in self.real_len..self.rows.len() {
            self.index.insert(self.rows[i].symbol.clone(), i);
        }
    }

    /// 壓測：在 5 萬合成列裡隨機挑 n 列跳價（同 wry 版 applyTicks），有變就 notify。
    pub fn synth_ticks(&mut self, n: usize, rng: &mut u64, cx: &mut Context<Self>) {
        let base = self.real_len;
        let cnt = self.rows.len().saturating_sub(base);
        if !self.stress || cnt == 0 {
            return;
        }
        let mut next = || {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            *rng
        };
        for _ in 0..n {
            let i = base + (next() % cnt as u64) as usize;
            let u = (next() % 10_000) as f64 / 10_000.0 - 0.5;
            let r = &mut self.rows[i];
            let wobble = u * (r.price.abs() * 0.002).max(0.2);
            r.price += wobble;
            r.change += wobble;
            r.volume += (next() % 200) as i64;
            r.tick = if wobble >= 0.0 { 1 } else { -1 };
        }
        cx.notify();
    }

    /// 壓測：捲到第 frac（0..1）的位置（同 wry 版 scrollTop = frac × maxScroll）
    pub fn scroll_frac(&mut self, frac: f64, cx: &mut Context<Self>) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let ix = ((n - 1) as f64 * frac.clamp(0.0, 1.0)) as usize;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
        cx.notify();
    }

    fn select(&mut self, ix: usize, win: Option<gpui::WindowId>, cx: &mut Context<Self>) {
        if ix >= self.rows.len() {
            return;
        }
        self.selected = ix;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.emit(TableEvent::Select(self.rows[ix].symbol.clone(), win));
        cx.notify();
    }

    /// 搜尋框（手機版）：代號開頭或名稱包含，最多 max 筆，回傳（代號, 名稱）
    #[allow(dead_code)]
    pub fn search(&self, q: &str, max: usize) -> Vec<(SharedString, SharedString)> {
        let q = q.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let ql = q.to_lowercase();
        self.rows
            .iter()
            .filter(|r| r.symbol.to_lowercase().starts_with(&ql) || r.name.contains(q))
            .take(max)
            .map(|r| (r.symbol.clone(), r.name.clone()))
            .collect()
    }

    /// 直接選某個代號（搜尋結果點選用）；會發 Select 事件
    #[allow(dead_code)]
    pub fn select_symbol(&mut self, symbol: &str, cx: &mut Context<Self>) {
        if let Some(&ix) = self.index.get(symbol) {
            self.select(ix, None, cx);
        }
    }

    /// A2（2026-10-24）：每列只建一個 div（點選／hover／選取底色）＋一個 canvas 自己畫 6 格文字。
    /// 舊做法每格一個 div＋文字元素（每列 13 個排版節點，可見 30 列就是約 400 個），實機 Windows 5 萬列捲動卡頓 70 次、
    /// CPU 是 egui 的 1.8 倍；現在每列 2 個節點，文字用 shape 快取（代號、名稱幾乎不變；價格變了才重排字）。
    /// XQ_TABLE_DIVS=1 切回舊做法對照。
    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        if table_divs() {
            return self.render_rows_divs(range, cx);
        }
        let t0 = crate::plat::Instant::now();
        if range.len() > 1 {
            self.paints += 1;
        }
        self.cells += (range.len() * COLS.len()) as u64;
        let mut out = Vec::with_capacity(range.len());
        let up_flash = crate::theme::rgba_tok(tk().market.up_flash);
        let down_flash = crate::theme::rgba_tok(tk().market.down_flash);
        for ix in range {
            let r = &self.rows[ix];
            let color: Hsla = dir_color(r.change).into();
            let has = r.price > 0.0;
            let dash: SharedString = "--".into();
            let (price, chg, pc, vol): (SharedString, SharedString, SharedString, SharedString) = if has {
                (
                    format!("{:.2}", r.price).into(),
                    fmt_change(r.change).into(),
                    format!("{:+.2}%", pct(r.price, r.change)).into(),
                    fmt_int(r.volume).into(),
                )
            } else {
                (dash.clone(), dash.clone(), dash.clone(), dash)
            };
            let tick_bg: Option<Hsla> = match r.tick {
                1 => Some(up_flash.into()),
                -1 => Some(down_flash.into()),
                _ => None,
            };
            let cells: [(SharedString, Hsla); 6] = [
                (r.symbol.clone(), text().into()),
                (r.name.clone(), text().into()),
                (price, color),
                (chg, color),
                (pc, color),
                (vol, rgb_vol().into()),
            ];
            let row = canvas(
                move |_, _, _| (),
                move |b, _, window, cx| paint_row(b, &cells, tick_bg, window, cx),
            )
            .size_full();
            out.push(
                div()
                    .id(ix)
                    .h(px(row_h()))
                    .w_full()
                    .when(ix == self.selected, |d| d.bg(selected()))
                    .when(ix != self.selected, |d| d.hover(|s| s.bg(header())))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.select(ix, Some(window.window_handle().window_id()), cx)))
                    .child(row)
                    .into_any_element(),
            );
        }
        self.paint_us += t0.elapsed().as_micros() as u64;
        out
    }

    /// 舊做法（每格一個 div）：XQ_TABLE_DIVS=1 對照用
    fn render_rows_divs(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t0 = crate::plat::Instant::now();
        // uniform_list 會先用 0..1 量第一列高度，那次不算一次「可見列更新」
        if range.len() > 1 {
            self.paints += 1;
        }
        self.cells += (range.len() * COLS.len()) as u64;
        let mut out = Vec::with_capacity(range.len());
        for ix in range {
            let r = &self.rows[ix];
            let color = dir_color(r.change);
            let has = r.price > 0.0;
            let (price, chg, pc, vol) = if has {
                (
                    format!("{:.2}", r.price),
                    fmt_change(r.change),
                    format!("{:+.2}%", pct(r.price, r.change)),
                    fmt_int(r.volume),
                )
            } else {
                ("--".into(), "--".into(), "--".into(), "--".into())
            };
            let tick_bg = match r.tick {
                1 => Some(crate::theme::rgba_tok(tk().market.up_flash)),
                -1 => Some(crate::theme::rgba_tok(tk().market.down_flash)),
                _ => None,
            };
            let mut price_cell = cell(COLS[2].1, true).text_color(color).child(price);
            if let Some(bg) = tick_bg {
                price_cell = price_cell.bg(bg);
            }
            out.push(
                div()
                    .id(ix)
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(row_h()))
                    .w_full()
                    .when(ix == self.selected, |d| d.bg(selected()))
                    .when(ix != self.selected, |d| d.hover(|s| s.bg(header())))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| this.select(ix, Some(window.window_handle().window_id()), cx)))
                    .child(cell(COLS[0].1, false).child(r.symbol.clone()))
                    .child(cell(COLS[1].1, false).text_color(text()).child(r.name.clone()))
                    .child(price_cell)
                    .child(cell(COLS[3].1, true).text_color(color).child(chg))
                    .child(cell(COLS[4].1, true).text_color(color).child(pc))
                    .child(cell(COLS[5].1, true).text_color(rgb_vol()).child(vol))
                    .into_any_element(),
            );
        }
        self.paint_us += t0.elapsed().as_micros() as u64;
        out
    }
}

fn table_divs() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::plat::var_os("XQ_TABLE_DIVS").is_some_and(|v| v != "0"))
}

/// 格內左右留白（同舊版 px_1）
const CELL_PAD: f32 = 4.0;

// shape 快取：key＝(文字, 是否等寬字, 顏色)。兩代輪替：這一代找不到就去上一代拿，上一代滿了整代丟掉，
// 所以只留最近用過的（可見列的代號／名稱／沒變的價），不會無限長大。
type ShapeKey = (SharedString, bool, [u32; 4]);
thread_local! {
    static SHAPES: std::cell::RefCell<(HashMap<ShapeKey, ShapedLine>, HashMap<ShapeKey, ShapedLine>)> =
        std::cell::RefCell::new((HashMap::new(), HashMap::new()));
}
/// 文字排版快取：預設關（0）。實測 GPUI 自己的逐幀 LineLayout 快取已夠用，這層快取 2048×2 筆反而多吃 ~41 MB RSS、FPS 沒有可量測的好處；`XQ_SHAPE_GEN=N` 可打開（每代 N 筆、兩代輪替）
fn shape_gen() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::plat::var("XQ_SHAPE_GEN").ok().and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn shaped(window: &mut Window, s: &SharedString, mono: bool, color: Hsla, size: f32) -> ShapedLine {
    if shape_gen() == 0 {
        let f = if mono { font(mono_font()) } else { window.text_style().font() };
        let run = TextRun { len: s.len(), font: f, color, background_color: None, underline: None, strikethrough: None };
        return window.text_system().shape_line(s.clone(), px(size), &[run], None);
    }
    let key: ShapeKey = (s.clone(), mono, [color.h.to_bits(), color.s.to_bits(), color.l.to_bits(), color.a.to_bits() ^ size.to_bits()]);
    if let Some(l) = SHAPES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(l) = c.0.get(&key) {
            return Some(l.clone());
        }
        let l = c.1.remove(&key)?;
        c.0.insert(key.clone(), l.clone());
        Some(l)
    }) {
        return l;
    }
    let f = if mono { font(mono_font()) } else { window.text_style().font() };
    let run = TextRun { len: s.len(), font: f, color, background_color: None, underline: None, strikethrough: None };
    let l = window.text_system().shape_line(s.clone(), px(size), &[run], None);
    SHAPES.with(|c| {
        let mut c = c.borrow_mut();
        if c.0.len() >= shape_gen() {
            let cur = std::mem::take(&mut c.0);
            c.1 = cur;
        }
        c.0.insert(key, l.clone());
    });
    l
}

fn paint_row(b: Bounds<Pixels>, cells: &[(SharedString, Hsla); 6], tick_bg: Option<Hsla>, window: &mut Window, cx: &mut App) {
    let size = tk().font.md;
    let lh = b.size.height;
    let mut x = b.origin.x;
    for (k, (s, color)) in cells.iter().enumerate() {
        let w = px(COLS[k].1);
        let right = k >= 2;
        if k == 2 {
            if let Some(bg) = tick_bg {
                window.paint_quad(gpui::fill(Bounds::new(point(x, b.origin.y), gpui::size(w, lh)), bg));
            }
        }
        // 數字欄（等寬、ASCII）：放得下就直接畫字形，不排字（gfx::mono_paint）
        if right {
            if let Some(mw) = crate::gfx::mono_width(window, s, size).filter(|mw| *mw <= f32::from(w) - 2.0 * CELL_PAD) {
                let tx = f32::from(x + w) - CELL_PAD - mw;
                crate::gfx::mono_paint(window, s, tx, f32::from(b.origin.y), f32::from(lh), size, *color);
                x += w;
                continue;
            }
        }
        let line = shaped(window, s, right, *color, size);
        let lw = line.width();
        let tx = if right { x + w - px(CELL_PAD) - lw } else { x + px(CELL_PAD) };
        let cell_b = Bounds::new(point(x + px(CELL_PAD), b.origin.y), gpui::size(w - px(2.0 * CELL_PAD), lh));
        // 只有放不下時才裁切（名稱欄偶爾會超出）
        if lw > cell_b.size.width {
            window.with_content_mask(Some(ContentMask { bounds: cell_b }), |window| {
                let _ = line.paint(point(tx, b.origin.y), lh, TextAlign::Left, None, window, cx);
            });
        } else {
            let _ = line.paint(point(tx, b.origin.y), lh, TextAlign::Left, None, window, cx);
        }
        x += w;
    }
}

fn rgb_vol() -> gpui::Rgba {
    gpui::rgb(tk().market.volume)
}

fn cell(w: f32, right: bool) -> gpui::Div {
    let d = div().w(px(w)).px_1().overflow_hidden().whitespace_nowrap();
    if right { d.flex().justify_end().font_family(mono_font()) } else { d }
}

impl Render for QuoteTable {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.rows.len();
        let stress = self.stress;
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(panel())
            .text_size(px(tk().font.md))
            .text_color(text())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(tk().layout.table_header))
                    .px_2()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child(div().flex().gap_2().child("報價").child(div().text_color(dim()).child(format!("{} 檔", fmt_int(count as i64)))))
                    .children(self.search.clone().map(|input| {
                        let matches = self.matches.clone();
                        let sel = self.match_sel;
                        let list = (!matches.is_empty()).then(|| {
                            deferred(
                                div()
                                    .absolute()
                                    .top(px(22.))
                                    .left_0()
                                    .w(px(220.))
                                    .bg(header())
                                    .border_1()
                                    .border_color(accent())
                                    .rounded_sm()
                                    .shadow_md()
                                    .children(matches.into_iter().enumerate().map(|(k, ix)| {
                                        let r = &self.rows[ix];
                                        let input = input.clone();
                                        div()
                                            .id(("sr", k))
                                            .flex()
                                            .gap_2()
                                            .px_2()
                                            .h(px(20.))
                                            .items_center()
                                            .cursor_pointer()
                                            .when(k == sel, |d| d.bg(selected()))
                                            .hover(|s| s.bg(selected()))
                                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.pick(ix, &input, cx)))
                                            .child(div().w(px(56.)).font_family(mono_font()).child(r.symbol.clone()))
                                            .child(r.name.clone())
                                    })),
                            )
                            .with_priority(1)
                        });
                        div().relative().child(input).children(list)
                    }))
                    .child(
                        div()
                            .id("stress")
                            .px_2()
                            .rounded(px(tk().radius.sm))
                            .cursor_pointer()
                            .border_1()
                            .border_color(if stress { accent() } else { border() })
                            .text_color(if stress { accent() } else { dim() })
                            .hover(|s| s.bg(selected()))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_stress(cx)))
                            .child("壓力 5 萬"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(tk().layout.table_col_header))
                    .items_center()
                    .text_color(dim())
                    .border_b_1()
                    .border_color(border())
                    // 欄名一個 canvas 畫完（原本每欄一個 div＋文字，報價表每次重畫都要重排 13 個節點）
                    .child({
                        let c: Hsla = dim().into();
                        let cells: [(SharedString, Hsla); 6] = std::array::from_fn(|i| (SharedString::new_static(COLS[i].0), c));
                        canvas(|_, _, _| (), move |b, _, window, cx| paint_row(b, &cells, None, window, cx)).size_full()
                    }),
            )
            .child(
                uniform_list("quotes", count, cx.processor(|this, range, _window, cx| this.render_rows(range, cx)))
                    .track_scroll(&self.scroll)
                    .flex_1(),
            )
    }
}
