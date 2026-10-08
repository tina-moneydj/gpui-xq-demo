//! 走勢圖：GPUI canvas 自繪（K 棒／分時線、MA、成交量、十字線）。
//! 序列放在 Rc 裡，paint closure 只拿 Rc 的 clone（不複製資料）；
//! tick 時 Rc::make_mut 只改最後一根（上一個 frame 的 closure 已釋放，不會真的複製）。

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    canvas, div, font, point, prelude::*, px, quad, size, App, Bounds, Context, EventEmitter, Hsla, MouseButton,
    MouseMoveEvent, PathBuilder, Pixels, Point, Rgba, ScrollWheelEvent, SharedString, TextAlign, TextRun, Window,
};

use crate::series::{self, Period, Series};
use crate::theme::*;

pub enum ChartEvent {
    Period(Period),
}

pub struct ChartView {
    series: Rc<Series>,
    name: SharedString,
    price: f64,
    change: f64,
    volume: i64,
    period: Period,
    /// 日線是否已用真實現價重新錨定
    anchored: bool,
    cross: Option<Point<Pixels>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// 日線每根寬（滾輪縮放）
    spacing: f32,
    pub paints: Rc<Cell<u64>>,
    pub paint_us: Rc<Cell<u64>>,
}

impl EventEmitter<ChartEvent> for ChartView {}

#[derive(Clone, Copy)]
struct Geo {
    price: Bounds<Pixels>,
    vol: Bounds<Pixels>,
    axis_x: f32,
    time_y: f32,
    first: usize,
    end: usize,
    lo: f64,
    hi: f64,
    vmax: f64,
    spacing: f32,
    t0: u32,
    intraday: bool,
}

const AXIS_W: f32 = 66.0;
const TIME_H: f32 = 18.0;

impl Geo {
    fn new(b: Bounds<Pixels>, s: &Series, spacing: f32) -> Option<Geo> {
        let (x, y, w, h) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
        if s.bars.is_empty() || w < AXIS_W + 40.0 || h < 80.0 {
            return None;
        }
        let plot_w = w - AXIS_W - 8.0;
        let body_h = h - TIME_H;
        let price_h = (body_h * 0.76).floor();
        let price = Bounds::new(point(px(x + 8.0), px(y + 6.0)), size(px(plot_w), px(price_h - 6.0)));
        let vol = Bounds::new(point(px(x + 8.0), px(y + price_h + 6.0)), size(px(plot_w), px(body_h - price_h - 8.0)));
        let n = s.bars.len();
        let intraday = s.period == Period::Intraday;
        let (first, end, spacing, t0) = if intraday {
            let t0 = s.bars[0].t;
            let span = (s.bars[n - 1].t.saturating_sub(t0) + 15).max(270);
            (0, n, plot_w / span as f32, t0)
        } else {
            let cap = ((plot_w / spacing).floor() as usize).max(2);
            let vis = n.min(cap);
            (n - vis, n, spacing, 0)
        };
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        let mut vmax: f64 = 1.0;
        for i in first..end {
            let b = &s.bars[i];
            lo = lo.min(b.l);
            hi = hi.max(b.h);
            vmax = vmax.max(b.v);
            for ma in &s.mas {
                if let Some(v) = ma.vals.get(i).copied().filter(|v| v.is_finite()) {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        if intraday && s.prev > 0.0 {
            lo = lo.min(s.prev);
            hi = hi.max(s.prev);
        }
        let pad = ((hi - lo) * 0.06).max(hi.abs() * 0.001).max(0.01);
        Some(Geo {
            price,
            vol,
            axis_x: x + w - AXIS_W,
            time_y: y + h - TIME_H,
            first,
            end,
            lo: lo - pad,
            hi: hi + pad,
            vmax,
            spacing,
            t0,
            intraday,
        })
    }

    fn left(&self) -> f32 { f32::from(self.price.origin.x) }
    fn right(&self) -> f32 { self.left() + f32::from(self.price.size.width) }

    fn x(&self, s: &Series, i: usize) -> f32 {
        if self.intraday {
            self.left() + ((s.bars[i].t - self.t0) as f32 + 0.5) * self.spacing
        } else {
            self.left() + ((i - self.first) as f32 + 0.5) * self.spacing
        }
    }

    fn y(&self, v: f64) -> f32 {
        let top = f32::from(self.price.origin.y);
        let h = f32::from(self.price.size.height);
        top + ((self.hi - v) / (self.hi - self.lo)) as f32 * h
    }

    fn price_at(&self, y: f32) -> f64 {
        let top = f32::from(self.price.origin.y);
        let h = f32::from(self.price.size.height);
        self.hi - ((y - top) / h) as f64 * (self.hi - self.lo)
    }

    fn vy(&self, v: f64) -> f32 {
        let top = f32::from(self.vol.origin.y);
        let h = f32::from(self.vol.size.height);
        top + h - (v / self.vmax) as f32 * h
    }

    fn index_at(&self, s: &Series, x: f32) -> usize {
        if self.intraday {
            let t = self.t0 as f32 + ((x - self.left()) / self.spacing).max(0.0);
            let i = s.bars.partition_point(|b| (b.t as f32) < t - 0.5);
            i.min(self.end - 1)
        } else {
            let k = ((x - self.left()) / self.spacing).floor().max(0.0) as usize;
            (self.first + k).min(self.end - 1)
        }
    }
}

impl ChartView {
    pub fn new(symbol: &str, name: &str, period: Period) -> Self {
        let series = match period {
            Period::Intraday => Series::empty(symbol, period),
            Period::Daily => series::synth_daily(symbol, 0.0, 0.0, 0),
        };
        ChartView {
            series: Rc::new(series),
            name: name.to_string().into(),
            price: 0.0,
            change: 0.0,
            volume: 0,
            period,
            anchored: false,
            cross: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            spacing: 6.0,
            paints: Rc::new(Cell::new(0)),
            paint_us: Rc::new(Cell::new(0)),
        }
    }

    pub fn symbol(&self) -> &str {
        &self.series.symbol
    }

    pub fn period(&self) -> Period {
        self.period
    }

    /// 換代號或週期：重建序列（一次 O(n)），之後都是增量。
    pub fn load(&mut self, symbol: &str, name: &str, period: Period, quote: Option<(f64, f64, i64)>, cx: &mut Context<Self>) {
        let (price, change, volume) = quote.unwrap_or((0.0, 0.0, 0));
        self.period = period;
        self.name = name.to_string().into();
        self.price = price;
        self.change = change;
        self.volume = volume;
        self.anchored = price > 0.0;
        self.series = Rc::new(match period {
            Period::Intraday => Series::empty(symbol, period),
            Period::Daily => series::synth_daily(symbol, price, change, volume),
        });
        cx.notify();
    }

    pub fn apply_quote(&mut self, price: f64, change: f64, volume: i64, name: &str, cx: &mut Context<Self>) {
        // demo-ticks 長時間跑會把價格溢位成 ±21474836.48；這種值不畫進 K 線
        if !(price > 0.0 && price < 10_000_000.0) {
            return;
        }
        if self.price == price && self.change == change && self.volume == volume {
            return;
        }
        self.price = price;
        self.change = change;
        self.volume = volume;
        if self.name.as_ref() != name && !name.is_empty() {
            self.name = name.to_string().into();
        }
        if self.period == Period::Daily && !self.anchored && price > 0.0 {
            // 第一筆真實價：重鋪合成日線讓最後一根對上實價（只發生一次）
            self.anchored = true;
            self.series = Rc::new(series::synth_daily(&self.series.symbol, price, change, volume));
        } else {
            let day_vol = (self.period == Period::Daily).then_some(volume);
            Rc::make_mut(&mut self.series).apply_price(price, day_vol);
        }
        cx.notify();
    }

    pub fn set_intraday(&mut self, symbol: &str, prev: f64, bars: &[xq_feed::Bar], cx: &mut Context<Self>) {
        if self.period != Period::Intraday || symbol != self.series.symbol {
            return;
        }
        self.series = Rc::new(series::from_feed(symbol, prev, bars));
        cx.notify();
    }

    pub fn apply_minute(&mut self, symbol: &str, bar: &xq_feed::Bar, cx: &mut Context<Self>) {
        if self.period != Period::Intraday || symbol != self.series.symbol {
            return;
        }
        Rc::make_mut(&mut self.series).apply_minute(bar);
        cx.notify();
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.period != Period::Daily {
            return;
        }
        let dy = f32::from(ev.delta.pixel_delta(px(16.)).y);
        if dy == 0.0 {
            return;
        }
        let f = if dy > 0.0 { 1.12 } else { 1.0 / 1.12 };
        self.spacing = (self.spacing * f).clamp(2.0, 30.0);
        cx.notify();
    }

    fn readout(&self) -> Option<SharedString> {
        let p = self.cross?;
        let g = Geo::new(self.bounds.get(), &self.series, self.spacing)?;
        if f32::from(p.x) < g.left() || f32::from(p.x) > g.right() {
            return None;
        }
        let i = g.index_at(&self.series, f32::from(p.x));
        let b = self.series.bars[i];
        let ma: Vec<String> = self
            .series
            .mas
            .iter()
            .filter_map(|m| m.vals.get(i).filter(|v| v.is_finite()).map(|v| format!("MA{} {:.2}", m.n, v)))
            .collect();
        Some(
            format!(
                "{}  開 {:.2} 高 {:.2} 低 {:.2} 收 {:.2} 量 {}  {}",
                time_label(&self.series, b.t),
                b.o,
                b.h,
                b.l,
                b.c,
                fmt_int(b.v as i64),
                ma.join(" ")
            )
            .into(),
        )
    }
}

fn time_label(s: &Series, t: u32) -> String {
    match s.period {
        Period::Intraday => format!("{:02}:{:02}", t / 60, t % 60),
        Period::Daily => format!("{}/{:02}/{:02}", t / 10_000, t / 100 % 100, t % 100),
    }
}

fn hsla(c: Rgba) -> Hsla {
    c.into()
}

fn text_line(window: &mut Window, cx: &mut App, s: String, x: f32, y: f32, color: Rgba, right_edge: Option<f32>) {
    let s: SharedString = s.into();
    let run = TextRun {
        len: s.len(),
        font: font(MONO_FONT),
        color: hsla(color),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(s, px(11.), &[run], None);
    let x = match right_edge {
        Some(r) => r - f32::from(line.width),
        None => x,
    };
    let _ = line.paint(point(px(x), px(y)), px(14.), TextAlign::Left, None, window, cx);
}

fn rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: impl Into<gpui::Background>) {
    window.paint_quad(quad(
        Bounds::new(point(px(x), px(y)), size(px(w.max(0.5)), px(h.max(0.5)))),
        px(0.),
        color,
        px(0.),
        gpui::transparent_black(),
        Default::default(),
    ));
}

fn paint_chart(bounds: Bounds<Pixels>, s: &Series, spacing: f32, cross: Option<Point<Pixels>>, window: &mut Window, cx: &mut App) {
    let Some(g) = Geo::new(bounds, s, spacing) else {
        let msg = if s.period == Period::Intraday { "等待引擎分時資料…（需連上 WryFeedHost）" } else { "沒有資料" };
        text_line(window, cx, msg.into(), f32::from(bounds.origin.x) + 16.0, f32::from(bounds.origin.y) + 16.0, dim(), None);
        return;
    };
    // 格線＋右側價格軸
    let rows = 5;
    for k in 0..=rows {
        let v = g.lo + (g.hi - g.lo) * k as f64 / rows as f64;
        let y = g.y(v);
        rect(window, g.left(), y, g.right() - g.left(), 1.0, grid());
        text_line(window, cx, format!("{:.2}", v), 0.0, y - 7.0, dim(), Some(g.axis_x + AXIS_W - 6.0));
    }
    rect(window, g.left(), f32::from(g.vol.origin.y) - 3.0, g.right() - g.left(), 1.0, border());
    rect(window, g.axis_x, f32::from(bounds.origin.y), 1.0, f32::from(bounds.size.height), border());

    // 時間軸標籤（約每 110px 一個）
    let every = ((110.0 / g.spacing).ceil() as usize).max(1);
    let mut last_x = f32::MIN;
    for i in (g.first..g.end).step_by(if g.intraday { 1 } else { every }) {
        let x = g.x(s, i);
        if x - last_x < 110.0 || (g.intraday && s.bars[i].t % 30 != 0 && i != g.first) {
            continue;
        }
        last_x = x;
        rect(window, x, f32::from(g.price.origin.y), 1.0, g.time_y - f32::from(g.price.origin.y), gpui::rgba(0x1c223088));
        text_line(window, cx, time_label(s, s.bars[i].t), x - 18.0, g.time_y + 2.0, dim(), None);
    }

    let vol_base = g.vy(0.0);
    if g.intraday {
        // 昨收參考線（虛線）
        if s.prev > 0.0 {
            let y = g.y(s.prev);
            let mut pb = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(3.)]);
            pb.move_to(point(px(g.left()), px(y)));
            pb.line_to(point(px(g.right()), px(y)));
            if let Ok(p) = pb.build() {
                window.paint_path(p, hsla(dim()));
            }
        }
        // 成交量
        let w = (g.spacing * 0.7).max(1.0);
        for i in g.first..g.end {
            let b = &s.bars[i];
            let c = if b.c >= b.o { up() } else { down() };
            let y = g.vy(b.v);
            rect(window, g.x(s, i) - w / 2.0, y, w, vol_base - y, c);
        }
        // 價格線
        let mut pb = PathBuilder::stroke(px(1.5));
        for i in g.first..g.end {
            let p = point(px(g.x(s, i)), px(g.y(s.bars[i].c)));
            if i == g.first { pb.move_to(p) } else { pb.line_to(p) }
        }
        if let Ok(p) = pb.build() {
            window.paint_path(p, hsla(gpui::rgb(0xe8eef7)));
        }
    } else {
        let body = (g.spacing * 0.7).clamp(1.0, 18.0);
        for i in g.first..g.end {
            let b = &s.bars[i];
            let x = g.x(s, i);
            let c = if b.c >= b.o { up() } else { down() };
            rect(window, x - 0.5, g.y(b.h), 1.0, g.y(b.l) - g.y(b.h), c);
            let (top, bot) = (g.y(b.o.max(b.c)), g.y(b.o.min(b.c)));
            rect(window, x - body / 2.0, top, body, (bot - top).max(1.0), c);
            let y = g.vy(b.v);
            rect(window, x - body / 2.0, y, body, vol_base - y, gpui::Rgba { a: 0.75, ..c });
        }
    }
    // MA
    let colors = ma_colors();
    for (k, ma) in s.mas.iter().enumerate() {
        let mut pb = PathBuilder::stroke(px(1.));
        let mut started = false;
        for i in g.first..g.end.min(ma.vals.len()) {
            let v = ma.vals[i];
            if !v.is_finite() {
                continue;
            }
            let p = point(px(g.x(s, i)), px(g.y(v)));
            if started { pb.line_to(p) } else { pb.move_to(p); started = true; }
        }
        if started {
            if let Ok(p) = pb.build() {
                window.paint_path(p, hsla(colors[k % colors.len()]));
            }
        }
    }
    // 最新價標籤
    if let Some(last) = s.bars.last() {
        let y = g.y(last.c);
        let c = if s.prev > 0.0 { dir_color(last.c - s.prev) } else { accent() };
        rect(window, g.axis_x + 1.0, y - 8.0, AXIS_W - 2.0, 16.0, c);
        text_line(window, cx, format!("{:.2}", last.c), 0.0, y - 7.0, gpui::rgb(0x0b0e14), Some(g.axis_x + AXIS_W - 6.0));
    }
    // 十字線
    if let Some(p) = cross {
        let (mx, my) = (f32::from(p.x), f32::from(p.y));
        if mx >= g.left() && mx <= g.right() && my >= f32::from(bounds.origin.y) && my <= g.time_y {
            let i = g.index_at(s, mx);
            let x = g.x(s, i);
            rect(window, x, f32::from(g.price.origin.y), 1.0, g.time_y - f32::from(g.price.origin.y), cross_color());
            rect(window, g.left(), my, g.right() - g.left(), 1.0, cross_color());
            if my <= f32::from(g.price.origin.y + g.price.size.height) {
                rect(window, g.axis_x + 1.0, my - 8.0, AXIS_W - 2.0, 16.0, gpui::rgb(0x3a4252));
                text_line(window, cx, format!("{:.2}", g.price_at(my)), 0.0, my - 7.0, text(), Some(g.axis_x + AXIS_W - 6.0));
            }
            let label = time_label(s, s.bars[i].t);
            let w = if g.intraday { 44.0 } else { 80.0 };
            rect(window, x - w / 2.0, g.time_y, w, TIME_H, gpui::rgb(0x3a4252));
            text_line(window, cx, label, x - w / 2.0 + 4.0, g.time_y + 2.0, text(), None);
        }
    }
}

fn cross_color() -> Rgba {
    Rgba { a: 0.7, ..cross() }
}

impl Render for ChartView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let series = self.series.clone();
        let spacing = self.spacing;
        let cross = self.cross;
        let bounds_slot = self.bounds.clone();
        let paints = self.paints.clone();
        let paint_us = self.paint_us.clone();
        let color = dir_color(self.change);
        let has = self.price > 0.0;
        let period = self.period;
        let readout = self.readout();
        let colors = ma_colors();

        let period_btn = |p: Period, cx: &mut Context<Self>| {
            let on = p == period;
            div()
                .id(p.code())
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(if on { accent() } else { border() })
                .text_color(if on { text() } else { dim() })
                .when(on, |d| d.bg(selected()))
                .hover(|s| s.bg(selected()))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(ChartEvent::Period(p))))
                .child(p.label())
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg())
            .text_size(px(12.))
            .text_color(text())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .h(px(28.))
                    .px_2()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child(div().text_size(px(13.)).child(format!("{} {}", self.series.symbol, self.name)))
                    .child(div().font_family(MONO_FONT).text_color(color).child(if has { format!("{:.2}", self.price) } else { "--".into() }))
                    .child(div().font_family(MONO_FONT).text_color(color).child(if has {
                        format!("{} ({:+.2}%)", fmt_change(self.change), pct(self.price, self.change))
                    } else {
                        String::new()
                    }))
                    .child(div().text_color(dim()).child(if has { format!("量 {}", fmt_int(self.volume)) } else { String::new() }))
                    .child(div().flex_1())
                    .children(self.series.mas.iter().enumerate().map(|(k, m)| {
                        div().text_color(colors[k % colors.len()]).child(format!("MA{}", m.n))
                    }))
                    .child(div().text_color(dim()).child(if series.live { "引擎" } else if period == Period::Daily { "合成日線" } else { "" }))
                    .child(period_btn(Period::Intraday, cx))
                    .child(period_btn(Period::Daily, cx)),
            )
            .child(
                div()
                    .h(px(18.))
                    .px_2()
                    .flex()
                    .items_center()
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .text_color(dim())
                    .child(readout.unwrap_or_else(|| "移動滑鼠看十字線；日線可用滾輪縮放".into())),
            )
            .child(
                div()
                    .id("chart-canvas")
                    .flex_1()
                    .w_full()
                    .cursor_crosshair()
                    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                        this.cross = Some(ev.position);
                        cx.notify();
                    }))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.cross = None;
                            cx.notify();
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.on_wheel(ev, cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| {
                        this.spacing = 6.0;
                        cx.notify();
                    }))
                    .child(
                        canvas(
                            move |bounds, _, _| bounds_slot.set(bounds),
                            move |bounds, _, window, cx| {
                                let t = Instant::now();
                                paint_chart(bounds, &series, spacing, cross, window, cx);
                                paints.set(paints.get() + 1);
                                paint_us.set(paint_us.get() + t.elapsed().as_micros() as u64);
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}
