//! 報價表：uniform_list 虛擬化，只畫可見列；行情只改有變的列，
//! 字串在 render 時只為可見列格式化（5 萬列也不預先建字串）。

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    div, prelude::*, px, uniform_list, Context, EventEmitter, MouseButton, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window,
};
use xq_feed::Quote;

use crate::theme::*;

pub const ROW_H: f32 = 22.0;
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
}

pub enum TableEvent {
    Select(SharedString),
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
}

impl EventEmitter<TableEvent> for QuoteTable {}

impl QuoteTable {
    pub fn new(watch: &[(&str, &str)]) -> Self {
        let rows: Vec<Row> = watch
            .iter()
            .map(|(s, n)| Row { symbol: (*s).into(), name: (*n).into(), price: 0.0, change: 0.0, volume: 0, tick: 0 })
            .collect();
        let index = rows.iter().enumerate().map(|(i, r)| (r.symbol.clone(), i)).collect();
        QuoteTable { real_len: rows.len(), rows, index, selected: 0, scroll: UniformListScrollHandle::new(), stress: false, paints: 0, paint_us: 0, cells: 0 }
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

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.rows.len() {
            return;
        }
        self.selected = ix;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.emit(TableEvent::Select(self.rows[ix].symbol.clone()));
        cx.notify();
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t0 = std::time::Instant::now();
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
                1 => Some(gpui::rgba(0xff4d4f22)),
                -1 => Some(gpui::rgba(0x22c55e22)),
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
                    .h(px(ROW_H))
                    .w_full()
                    .when(ix == self.selected, |d| d.bg(selected()))
                    .when(ix != self.selected, |d| d.hover(|s| s.bg(header())))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.select(ix, cx)))
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

fn rgb_vol() -> gpui::Rgba {
    gpui::rgb(0xf0c674)
}

fn cell(w: f32, right: bool) -> gpui::Div {
    let d = div().w(px(w)).px_1().overflow_hidden().whitespace_nowrap();
    if right { d.flex().justify_end().font_family(MONO_FONT) } else { d }
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
            .text_size(px(12.))
            .text_color(text())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(26.))
                    .px_2()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child(div().flex().gap_2().child("報價").child(div().text_color(dim()).child(format!("{} 檔", fmt_int(count as i64)))))
                    .child(
                        div()
                            .id("stress")
                            .px_2()
                            .rounded_sm()
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
                    .h(px(22.))
                    .items_center()
                    .text_color(dim())
                    .border_b_1()
                    .border_color(border())
                    .children(COLS.iter().enumerate().map(|(i, (label, w))| cell(*w, i >= 2).child(*label))),
            )
            .child(
                uniform_list("quotes", count, cx.processor(|this, range, _window, cx| this.render_rows(range, cx)))
                    .track_scroll(&self.scroll)
                    .flex_1(),
            )
    }
}
