//! 走勢圖視圖：三個疊起來的 cached view（GPUI 只重畫有 notify 的那層）
//!
//! - PlotLayer：格線、軸、K 棒、20 組主圖疊加、≤10 副圖（tick／平移／縮放時重畫）
//! - DrawLayer：上千筆畫線（只在尺度／內容變時重建；平移時頂點整批位移；不動就整層沿用上一幀）
//! - LiveLayer：圖例、十字線、選取／繪製中的畫線（滑鼠移動只重畫這層）
//!
//! 左側畫線工具列、上方週期／指標選單。XQ_CHART_STRESS=1 跑與 wry 版相同的重圖表壓測（見 stress.rs）。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, point, prelude::*, px, App, Bounds, ContentMask, Context, DispatchPhase, Entity, EventEmitter,
    FocusHandle, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollWheelEvent, SharedString, StyleRefinement, Subscription, Window,
};

use crate::drawings::{self, Emit, PaintCache, Tool};
use crate::gfx::{self, hex, hexa, p, Tris, P};
use crate::indicators::{OvSpec, SubKind, MAX_OVERLAYS, MAX_SUBS};
use crate::model::{IndCfg, Model, PaneKind, ALL, AXIS_W, DRAW, HEAD, LIVE, PLOT, SAVE, TIME_H};
use crate::series::Period;
use crate::store::{Entry, Store};
use crate::theme::*;

// wry 版 chart.js 的配色
pub const C_UP: u32 = 0xe5484d;
pub const C_DOWN: u32 = 0x30a46c;
const C_GRID: u32 = 0x242424;
const C_AXIS: u32 = 0x8a8a8a;
const C_TEXT: u32 = 0xcfcfcf;
const C_SEP: u32 = 0x333333;
const C_CROSS: u32 = 0x8b95a5;
const C_CROSS_LABEL: u32 = 0x3b4252;
pub const OV_COLORS: [u32; 20] = [
    0xf5c542, 0xc678dd, 0x56b6c2, 0xff7f50, 0x7fdbff, 0xe5484d, 0x30a46c, 0xf06292, 0xffb74d, 0x4fc3f7, 0xaed581, 0xce93d8,
    0x80cbc4, 0xffab91, 0x90caf9, 0xfff176, 0xef9a9a, 0xb0bec5, 0xdce775, 0xb39ddb,
];

fn sub_colors(k: SubKind) -> &'static [u32] {
    match k {
        SubKind::Vol => &[0xf5c542, 0xc678dd],
        SubKind::Kd => &[0xf5a623, 0x4fc3f7],
        SubKind::Macd => &[0xf5c542, 0x4fc3f7],
        SubKind::Rsi => &[0xc678dd],
        SubKind::Wr => &[0xf06292],
        SubKind::Dmi => &[0xe5484d, 0x30a46c, 0xf5c542],
        SubKind::Atr => &[0xffb74d],
        SubKind::Obv => &[0x4fc3f7],
    }
}

fn fmt_p(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "--".into() }
}
fn fmt_v(v: f64) -> String {
    if v.is_finite() { fmt_int(v.round() as i64) } else { "--".into() }
}
fn fmt_auto(v: f64) -> String {
    if !v.is_finite() { "--".into() } else if v.abs() >= 10000.0 { fmt_v(v) } else { format!("{v:.2}") }
}
fn nice_step(range: f64, count: f64) -> f64 {
    let raw = range / count.max(1.0);
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    (if n < 1.5 { 1.0 } else if n < 3.0 { 2.0 } else if n < 7.0 { 5.0 } else { 10.0 }) * mag
}
fn fmt_time(t: u32, period: Period, long: bool) -> String {
    match period {
        Period::Intraday => format!("{:02}:{:02}", t / 60, t % 60),
        Period::Monthly => format!("{}/{:02}", t / 10_000, t / 100 % 100),
        Period::Weekly => format!("{}/{:02}/{:02}", t / 10_000, t / 100 % 100, t % 100),
        Period::Daily if long => format!("{}/{:02}/{:02}", t / 10_000, t / 100 % 100, t % 100),
        Period::Daily => format!("{:02}/{:02}", t / 100 % 100, t % 100),
    }
}

pub type M = Rc<RefCell<Model>>;

// ───────────────────────── 繪圖層 ─────────────────────────

pub struct PlotLayer {
    m: M,
}
pub struct DrawLayer {
    m: M,
}
pub struct LiveLayer {
    m: M,
}

macro_rules! layer_render {
    ($ty:ty, $f:ident, $k:expr) => {
        impl Render for $ty {
            fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                let m = self.m.clone();
                canvas(
                    |_, _, _| (),
                    move |b, _, window, cx| {
                        let t = Instant::now();
                        // 整層一個 scene layer：同一個 order → quad 一批、path 一批（只做一次 MSAA 中介貼圖）、字一批
                        if split_layers() {
                            $f(&mut m.borrow_mut(), b, window, cx);
                        } else {
                            window.paint_layer(b, |window| $f(&mut m.borrow_mut(), b, window, cx));
                        }
                        let st = &mut m.borrow_mut().stats;
                        st.paints[$k] += 1;
                        st.paint_us[$k] += t.elapsed().as_micros() as u64;
                    },
                )
                .size_full()
            }
        }
    };
}
layer_render!(PlotLayer, paint_plot, 0);
layer_render!(DrawLayer, paint_draw, 1);
layer_render!(LiveLayer, paint_live, 2);

/// 圖層分法（GPUI 的 path 每一批都要清一次全視窗 MSAA 中介貼圖再合成，軟體 Vulkan 上很貴）：
/// - split（XQ_LAYER_MODE=split）：quad 一個 scene layer、path 不包 layer（各自 order 連號 → 一批、合成時一張聯集貼圖）、字一個 layer
/// - single（預設）：整層包一個 layer（同 order → 一批，但每條 path 各自合成一次）
pub fn split_layers() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("XQ_LAYER_MODE").as_deref() == Ok("split"))
}

pub fn maybe_layer<R>(window: &mut Window, b: Bounds<Pixels>, f: impl FnOnce(&mut Window) -> R) -> R {
    if split_layers() { window.paint_layer(b, f) } else { f(window) }
}

fn clip(window: &mut Window, x: f32, y: f32, w: f32, h: f32, f: impl FnOnce(&mut Window)) {
    let b = Bounds::new(point(px(x), px(y)), gpui::size(px(w.max(0.0)), px(h.max(0.0))));
    window.with_content_mask(Some(ContentMask { bounds: b }), f);
}

fn line_tris(t: &mut Tris, w: f32, i0: usize, i1: usize, x: impl Fn(usize) -> f32, y: impl Fn(f64) -> f32, v: &[f64]) {
    gfx::polyline(t, w, (i0..=i1).map(|i| v[i].is_finite().then(|| p(x(i), y(v[i])))));
}

/// 主繪圖：quad（格線、K 棒、量、柱、SAR）→ path（均線、布林、副圖線）→ 字（軸）。
/// 先把所有 quad 送完再送 path、最後送字，GPU 批次切換最少。
fn paint_plot(m: &mut Model, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let g = m.geo(b).clone();
    let m = &*m;
    let bars = &m.s.bars;
    let (bx, by, bw) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width));
    if bars.is_empty() {
        let msg = if m.s.period == Period::Intraday { "等待引擎分時資料…（需連上 WryFeedHost）" } else { "沒有資料" };
        gfx::text(window, cx, msg, bx + 16.0, by + 16.0, 12.0, hex(C_AXIS));
        return;
    }
    let intraday = m.s.period == Period::Intraday;
    let (i0, i1) = (g.i0, g.i1);
    let x = |i: usize| g.x(i as f64);
    let mut labels: Vec<(String, f32, f32)> = Vec::with_capacity(64);
    let mut pane_paths: Vec<Vec<(Tris, Hsla)>> = Vec::with_capacity(g.panes.len());
    let (up_c, dn_c) = (hex(C_UP), hex(C_DOWN));

    let last_c = bars[bars.len() - 1].c;
    let tag = maybe_layer(window, b, |window| {
    for pane in &g.panes {
        let mut paths: Vec<(Tris, Hsla)> = Vec::new();
        let (top, h) = (pane.y0, pane.ph);
        if pane.top > by {
            gfx::rect(window, g.plot_l, pane.top, g.plot_w, 1.0, hex(C_SEP));
        }
        for &i in &g.ticks {
            let xx = x(i).round();
            if xx >= g.plot_l && xx <= g.plot_r {
                gfx::rect(window, xx, top, 1.0, h, hex(C_GRID));
            }
        }
        let sub = match pane.kind {
            PaneKind::Price => None,
            PaneKind::Sub(k) => Some(&m.subs[k]),
        };
        let axis_tx = g.plot_r + 6.0;
        if let Some((_, guides)) = sub.and_then(|s| s.fixed()) {
            for &t in guides {
                let y = pane.y(t).round();
                gfx::hdash(window, g.plot_l, g.plot_r, y, hex(C_GRID), 3.0, 3.0);
                labels.push((format!("{t}"), axis_tx, y));
            }
        } else {
            let st = nice_step(pane.hi - pane.lo, if sub.is_some() { (h / 30.0).max(1.0) as f64 } else { (h / 45.0).max(2.0) as f64 });
            let mut t = (pane.lo / st).ceil() * st;
            let mut guard = 0;
            while t <= pane.hi && guard < 50 {
                let y = pane.y(t).round();
                if y >= top + 4.0 && y <= top + h - 2.0 {
                    gfx::rect(window, g.plot_l, y, g.plot_w, 1.0, hex(C_GRID));
                    let s = if t.abs() >= 10000.0 { fmt_v(t) } else if st < 1.0 { format!("{t:.2}") } else if st < 10.0 { format!("{t:.1}") } else { format!("{t:.0}") };
                    labels.push((s, axis_tx, y));
                }
                t += st;
                guard += 1;
            }
            if sub.is_some_and(|s| s.cfg.kind == SubKind::Dmi) && 20.0 > pane.lo && 20.0 < pane.hi {
                gfx::hdash(window, g.plot_l, g.plot_r, pane.y(20.0).round(), hex(0x3a3a3a), 3.0, 3.0);
            }
        }
        let yv = |v: f64| pane.y(v);
        clip(window, g.plot_l, pane.top + 1.0, g.plot_w, pane.h - 1.0, |window| match sub {
            None => {
                if intraday {
                    let mut t = Tris::default();
                    gfx::polyline(&mut t, 1.5, (i0..=i1).map(|i| Some(p(x(i), yv(bars[i].c)))));
                    paths.push((t, if m.change >= 0.0 { up_c } else { dn_c }));
                    if m.s.prev > 0.0 {
                        gfx::hdash(window, g.plot_l, g.plot_r, yv(m.s.prev), hex(0x777777), 4.0, 4.0);
                    }
                } else {
                    let bwid = (g.bar_w * 0.7).round().max(1.0) as f32;
                    for i in i0..=i1 {
                        let k = &bars[i];
                        let c = if k.c >= k.o { up_c } else { dn_c };
                        let xx = x(i).round();
                        let (yh, yl) = (yv(k.h), yv(k.l));
                        gfx::rect(window, xx, yh, 1.0, (yl - yh).max(1.0), c);
                        if g.bar_w >= 3.0 {
                            let (y1, y2) = (yv(k.o.max(k.c)), yv(k.o.min(k.c)));
                            gfx::rect(window, xx - (bwid / 2.0).floor() + 0.5, y1, bwid, (y2 - y1).max(1.0), c);
                        }
                    }
                }
                for (idx, o) in m.ovs.iter().enumerate() {
                    let col = hex(OV_COLORS[idx % 20]);
                    match o.spec {
                        OvSpec::Sar { .. } => {
                            let r = (g.bar_w * 0.2).clamp(1.2, 2.5) as f32;
                            for i in i0..=i1 {
                                let v = o.sar_at(i);
                                if v.is_finite() {
                                    gfx::dot(window, x(i), yv(v), r, if o.out.sar_up[i] { up_c } else { dn_c });
                                }
                            }
                        }
                        OvSpec::Bb { .. } => {
                            let mut fill = Tris::default();
                            for i in i0..i1 {
                                let (u0, u1, l0, l1) = (o.out.up[i], o.out.up[i + 1], o.out.lo[i], o.out.lo[i + 1]);
                                if u0.is_finite() && u1.is_finite() && l0.is_finite() && l1.is_finite() {
                                    let (a, bq, c, d) = (p(x(i), yv(u0)), p(x(i + 1), yv(u1)), p(x(i + 1), yv(l1)), p(x(i), yv(l0)));
                                    fill.tri(a, bq, c);
                                    fill.tri(a, c, d);
                                }
                            }
                            paths.push((fill, hexa(0x5c9ded, 0.08)));
                            let mut t = Tris::default();
                            line_tris(&mut t, 1.2, i0, i1, x, yv, &o.out.up);
                            line_tris(&mut t, 1.2, i0, i1, x, yv, &o.out.lo);
                            // 中軌虛線：隔一段畫一段
                            let mut prev: Option<P> = None;
                            for i in i0..=i1 {
                                let v = o.out.line[i];
                                let q = v.is_finite().then(|| p(x(i), yv(v)));
                                if let (Some(a), Some(c)) = (prev, q) {
                                    t.dashed(a, c, 1.0, 4.0, 3.0);
                                }
                                prev = q;
                            }
                            paths.push((t, col));
                        }
                        OvSpec::Ma { .. } | OvSpec::Ema { .. } => {
                            let mut t = Tris::default();
                            line_tris(&mut t, 1.2, i0, i1, x, yv, &o.out.line);
                            paths.push((t, col));
                        }
                    }
                }
            }
            Some(s) => {
                if s.cfg.kind == SubKind::Vol {
                    let bw = (g.bar_w * 0.7).max(1.0) as f32;
                    let base = pane.y0 + pane.ph;
                    for i in i0..=i1 {
                        let k = &bars[i];
                        let up = if intraday { i == 0 || k.c >= bars[i - 1].c } else { k.c >= k.o };
                        let y = yv(k.v);
                        gfx::rect(window, x(i) - bw / 2.0, y, bw, base - y, hexa(if up { C_UP } else { C_DOWN }, 0xaa as f32 / 255.0));
                    }
                }
                if !s.out.hist.is_empty() {
                    let bw = (g.bar_w * 0.6).max(1.0) as f32;
                    let y0 = yv(0.0);
                    for i in i0..=i1 {
                        let v = s.out.hist[i];
                        if v.is_finite() {
                            let y = yv(v);
                            gfx::rect(window, x(i) - bw / 2.0, y.min(y0), bw, (y - y0).abs().max(0.5), if v >= 0.0 { up_c } else { dn_c });
                        }
                    }
                }
                let cols = sub_colors(s.cfg.kind);
                for (k, line) in s.out.lines.iter().enumerate() {
                    let mut t = Tris::default();
                    line_tris(&mut t, 1.2, i0, i1, x, yv, line);
                    paths.push((t, hex(cols[k % cols.len()])));
                }
            }
        });
        pane_paths.push(paths);
    }
    gfx::rect(window, bx, g.bottom, bw, 1.0, hex(C_SEP));
    gfx::rect(window, g.plot_r, by, 1.0, g.bottom - by, hex(C_SEP));
    // 最新價標籤（底色 quad；字在最後）
    let last = bars[bars.len() - 1];
    let pr = g.panes[0];
    let ly = pr.y(last.c);
    let tag = (ly >= pr.y0 && ly <= pr.y0 + pr.ph).then(|| {
        let c = if m.change > 0.0 { C_UP } else if m.change < 0.0 { C_DOWN } else { 0x3b82f6 };
        gfx::rect(window, g.plot_r + 1.0, ly - 8.0, AXIS_W - 2.0, 16.0, hex(c));
        ly
    });
    tag
    });
    for (pane, paths) in g.panes.iter().zip(pane_paths) {
        clip(window, g.plot_l, pane.top + 1.0, g.plot_w, pane.h - 1.0, |window| {
            for (mut t, c) in paths {
                if let Some(path) = t.take() {
                    window.paint_path(path, c);
                }
            }
        });
    }
    let axis = hex(C_AXIS);
    maybe_layer(window, b, |window| {
    for (s, xx, y) in labels {
        gfx::text(window, cx, s, xx, y, 11.0, axis);
    }
    for &i in &g.ticks {
        let s = fmt_time(bars[i].t, m.s.period, false);
        let half = drawings::text_width(&s, 11.0) / 2.0 + 2.0;
        let cxp = x(i).clamp(g.plot_l + half, g.plot_r - half);
        gfx::text_center(window, cx, s, cxp, g.bottom + TIME_H / 2.0 + 1.0, 11.0, axis);
    }
    if let Some(ly) = tag {
        gfx::text(window, cx, fmt_p(last_c), g.plot_r + 5.0, ly, 11.0, hex(0xffffff));
    }
    });
}

/// 畫線層：快取命中就只位移貼上；尺度變了才重建（只建可見的）。
fn paint_draw(m: &mut Model, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    m.geo(b);
    if m.drawings.is_empty() {
        m.pc = None;
        return;
    }
    let dx = m.ensure_gc();
    let Some((key, right, _)) = m.draw_key() else { return };
    let xf = m.geo_now().unwrap().xf();
    if !matches!(&m.pc, Some((k, _, _)) if *k == key) {
        let t = Instant::now();
        let gc = m.gc.as_ref().unwrap();
        let mut bxf = xf;
        bxf.right = gc.right; // 位移基準：建 gc 時的 right
        let pc = PaintCache::build(window, &m.drawings, gc, &bxf);
        m.stats.rebuilds += 1;
        m.stats.rebuild_us += t.elapsed().as_micros() as u64;
        m.pc = Some((key, right, pc));
    }
    let (_, r, pc) = m.pc.as_mut().unwrap();
    *r = right;
    pc.paint(window, cx, &xf, dx);
}

/// live 層：選取／繪製中的畫線、文字輸入框、十字線、圖例（游標所在或最新一根）
fn paint_live(m: &mut Model, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let g = m.geo(b).clone();
    let m = &*m;
    let bars = &m.s.bars;
    if bars.is_empty() {
        return;
    }
    let xf = g.xf();
    let mut em = Emit::new();
    let mut any = false;
    if let Some(d) = m.selected.and_then(|k| m.drawings.get(k)) {
        em.drawing(d, &drawings::geom(d, &xf), true, true);
        any = true;
    }
    if let Some(pd) = &m.pending {
        em.drawing(&pd.d, &drawings::geom(&pd.d, &xf), false, true);
        any = true;
    }
    if any {
        window.with_content_mask(Some(ContentMask { bounds: xf.clip() }), |window| em.paint(window, cx, drawings::SEL_COLOR));
        if let Some(d) = m.selected.and_then(|k| m.drawings.get(k)).filter(|d| d.kind == Tool::Hline) {
            let y = xf.y(d.pts[0].p);
            if y > xf.pane_top + 8.0 && y < xf.pane_top + xf.pane_h - 8.0 {
                gfx::rect(window, g.plot_r + 1.0, y - 8.0, AXIS_W - 2.0, 16.0, hex(0x666666));
                gfx::text(window, cx, fmt_p(d.pts[0].p), g.plot_r + 5.0, y, 11.0, hex(0xffffff));
            }
        }
    }
    if let Some(te) = &m.text_edit {
        let s = format!("{}▏", te.text);
        let w = drawings::text_width(&s, 13.0).max(120.0) + 12.0;
        let (x0, y0) = (te.at.x, te.at.y - 11.0);
        gfx::boxed(window, x0, y0, w, 22.0, hexa(0x1e1e1e, 0.95), hex(0x3b82f6), 1.0);
        gfx::text(window, cx, s, x0 + 6.0, y0 + 12.0, 13.0, hex(drawings::DRAW_COLOR));
        gfx::text(window, cx, "輸入文字，Enter 確定，Esc 取消", x0, y0 + 32.0, 11.0, hex(C_AXIS));
    }
    let hov = g.hover_idx(m.mouse);
    let info = hov.unwrap_or(g.i1.min(bars.len() - 1));
    if let (Some(i), Some(q)) = (hov, m.mouse) {
        let xx = g.x(i as f64).round();
        let y = q.y.round();
        gfx::vdash(window, xx, f32::from(b.origin.y), g.bottom, hex(C_CROSS), 3.0, 3.0);
        gfx::hdash(window, g.plot_l, g.plot_r, y, hex(C_CROSS), 3.0, 3.0);
        let pane = g.pane_at(q).map(|k| g.panes[k]).unwrap_or(g.panes[0]);
        let v = pane.val_at(q.y);
        let is_vol = matches!(pane.kind, PaneKind::Sub(k) if matches!(m.subs[k].cfg.kind, SubKind::Vol | SubKind::Obv));
        gfx::rect(window, g.plot_r + 1.0, y - 8.0, AXIS_W - 2.0, 16.0, hex(C_CROSS_LABEL));
        gfx::text(window, cx, if is_vol { fmt_v(v) } else { fmt_auto(v) }, g.plot_r + 5.0, y, 11.0, hex(0xffffff));
        let s = fmt_time(bars[i].t, m.s.period, true);
        let w = drawings::text_width(&s, 11.0) + 10.0;
        let bxl = (xx - w / 2.0).clamp(g.plot_l, g.plot_r - w);
        gfx::rect(window, bxl, g.bottom + 1.0, w, TIME_H - 2.0, hex(C_CROSS_LABEL));
        gfx::text_center(window, cx, s, bxl + w / 2.0, g.bottom + TIME_H / 2.0 + 1.0, 11.0, hex(0xffffff));
    }
    // 主圖圖例
    let k = bars[info];
    let prev = if m.s.period == Period::Intraday { m.s.prev } else if info > 0 { bars[info - 1].c } else { k.o };
    let ch = k.c - prev;
    let col = hex(if ch >= 0.0 { C_UP } else { C_DOWN });
    let (t_c, a_c) = (hex(C_TEXT), hex(C_AXIS));
    let src = if m.s.live { "engine" } else if m.anchored { "合成K·現價" } else { "合成" };
    let sg = if ch >= 0.0 { "+" } else { "" };
    let head = vec![
        (format!("{}（{}）", m.s.period.label(), src), a_c),
        (fmt_time(k.t, m.s.period, true), t_c),
        (format!("開 {}", fmt_p(k.o)), t_c),
        (format!("高 {}", fmt_p(k.h)), t_c),
        (format!("低 {}", fmt_p(k.l)), t_c),
        (format!("收 {}", fmt_p(k.c)), col),
        (format!("{sg}{ch:.2} ({sg}{:.2}%)", if prev != 0.0 { ch / prev * 100.0 } else { 0.0 }), col),
        (format!("量 {}", fmt_v(k.v)), t_c),
    ];
    let pr = g.panes[0];
    let lx = g.plot_l + 4.0;
    let l = gfx::shape_items(window, &head, 11.0);
    gfx::paint_line(window, cx, &l, lx, pr.top + 8.0, 11.0);
    // 疊加圖例：依寬度換行（最多 6 行，與 wry 預留的圖例高度一致）
    let mut items: Vec<(String, Hsla)> = Vec::with_capacity(32);
    for (idx, o) in m.ovs.iter().enumerate() {
        let c = hex(OV_COLORS[idx % 20]);
        match o.spec {
            OvSpec::Ma { period } => items.push((format!("MA{period} {}", fmt_p(o.out.line[info])), c)),
            OvSpec::Ema { period } => items.push((format!("EMA{period} {}", fmt_p(o.out.line[info])), c)),
            OvSpec::Bb { n, k } => {
                items.push((format!("BB({n},{k}) {}", fmt_p(o.out.line[info])), c));
                items.push((format!("上 {}", fmt_p(o.out.up[info])), c));
                items.push((format!("下 {}", fmt_p(o.out.lo[info])), c));
            }
            OvSpec::Sar { .. } => items.push((format!("SAR {}", fmt_p(o.sar_at(info))), c)),
        }
    }
    let max_w = g.plot_w - 8.0;
    let mut lines: Vec<Vec<(String, Hsla)>> = vec![Vec::new()];
    let mut wsum = 0.0;
    for it in items {
        let w = drawings::text_width(&it.0, 11.0) + 10.0;
        if wsum + w > max_w && !lines.last().unwrap().is_empty() {
            lines.push(Vec::new());
            wsum = 0.0;
        }
        wsum += w;
        lines.last_mut().unwrap().push(it);
    }
    clip(window, g.plot_l, pr.top, g.plot_w, pr.h, |window| {
        let mut y = pr.top + 8.0;
        for ln in lines.iter().filter(|l| !l.is_empty()) {
            y += 14.0;
            let l = gfx::shape_items(window, ln, 11.0);
            gfx::paint_line(window, cx, &l, lx, y, 11.0);
        }
    });
    // 副圖圖例
    for pane in &g.panes[1..] {
        let PaneKind::Sub(si) = pane.kind else { continue };
        let s = &m.subs[si];
        let is_vol = s.cfg.kind == SubKind::Vol;
        let mut it = vec![(s.title(), a_c)];
        if is_vol {
            it.push((fmt_v(k.v), t_c));
        }
        let names = s.line_names();
        let cols = sub_colors(s.cfg.kind);
        for (li, l) in s.out.lines.iter().enumerate() {
            let nm = if is_vol { format!("MA{}", s.volma.map_or(0, |v| v[li])) } else { names.get(li).copied().unwrap_or("").to_string() };
            let v = if is_vol { fmt_v(l[info]) } else { fmt_auto(l[info]) };
            it.push((if nm.is_empty() { v } else { format!("{nm} {v}") }, hex(cols[li % cols.len()])));
        }
        if let Some(&hv) = s.out.hist.get(info) {
            it.push((format!("OSC {}", fmt_auto(hv)), hex(if hv >= 0.0 { C_UP } else { C_DOWN })));
        }
        let l = gfx::shape_items(window, &it, 11.0);
        clip(window, g.plot_l, pane.top, g.plot_w, pane.h, |window| gfx::paint_line(window, cx, &l, lx, pane.top + 7.0, 11.0));
    }
}

// ───────────────────────── 左側畫線工具列 ─────────────────────────

pub enum RailEvent {
    Tool(Option<Tool>),
    Delete,
    Clear,
}

pub struct Rail {
    tool: Option<Tool>,
    can_delete: bool,
    can_clear: bool,
}

impl EventEmitter<RailEvent> for Rail {}

#[derive(Clone, Copy)]
enum Icon {
    Cursor,
    Tool(Tool),
    Delete,
    Clear,
}

/// 圖示用 path 畫（不需要 SVG 資源）
fn paint_icon(icon: Icon, b: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let s = f32::from(b.size.width) / 24.0;
    let q = |x: f32, y: f32| p(x0 + x * s, y0 + y * s);
    let mut t = Tris::default();
    let w = 1.7;
    let mut dots: Vec<P> = Vec::new();
    let poly = |t: &mut Tris, pts: &[(f32, f32)]| {
        for k in 1..pts.len() {
            t.seg(q(pts[k - 1].0, pts[k - 1].1), q(pts[k].0, pts[k].1), w);
        }
    };
    match icon {
        Icon::Cursor => poly(&mut t, &[(6.0, 4.0), (18.0, 12.5), (12.5, 14.0), (9.5, 20.0), (6.0, 4.0)]),
        Icon::Tool(Tool::Trend) => {
            poly(&mut t, &[(4.0, 18.0), (20.0, 6.0)]);
            dots.extend([q(4.0, 18.0), q(20.0, 6.0)]);
        }
        Icon::Tool(Tool::Ray) => {
            poly(&mut t, &[(5.0, 18.0), (19.0, 6.0), (22.0, 4.5)]);
            dots.push(q(5.0, 18.0));
        }
        Icon::Tool(Tool::Hline) => {
            poly(&mut t, &[(3.0, 12.0), (21.0, 12.0)]);
            dots.push(q(12.0, 12.0));
        }
        Icon::Tool(Tool::Vline) => {
            poly(&mut t, &[(12.0, 3.0), (12.0, 21.0)]);
            dots.push(q(12.0, 12.0));
        }
        Icon::Tool(Tool::Channel) => {
            poly(&mut t, &[(4.0, 16.0), (16.0, 4.0)]);
            poly(&mut t, &[(8.0, 20.0), (20.0, 8.0)]);
            dots.extend([q(4.0, 16.0), q(16.0, 4.0)]);
        }
        Icon::Tool(Tool::Fib) => {
            for y in [5.0, 10.0, 14.0, 19.0] {
                poly(&mut t, &[(4.0, y), (20.0, y)]);
            }
            poly(&mut t, &[(4.0, 5.0), (4.0, 19.0)]);
        }
        Icon::Tool(Tool::Rect) => poly(&mut t, &[(4.0, 6.0), (20.0, 6.0), (20.0, 18.0), (4.0, 18.0), (4.0, 6.0)]),
        Icon::Tool(Tool::Text) => {
            poly(&mut t, &[(5.0, 6.0), (19.0, 6.0)]);
            poly(&mut t, &[(12.0, 6.0), (12.0, 18.0)]);
            poly(&mut t, &[(8.0, 18.0), (16.0, 18.0)]);
        }
        Icon::Delete => {
            poly(&mut t, &[(5.0, 7.0), (19.0, 7.0)]);
            poly(&mut t, &[(9.0, 7.0), (9.0, 5.0), (15.0, 5.0), (15.0, 7.0)]);
            poly(&mut t, &[(8.0, 7.0), (9.0, 19.0), (15.0, 19.0), (16.0, 7.0)]);
        }
        Icon::Clear => {
            poly(&mut t, &[(5.0, 6.0), (19.0, 6.0)]);
            poly(&mut t, &[(6.2, 6.0), (7.2, 20.0), (16.8, 20.0), (17.8, 6.0)]);
            for x in [9.0, 12.0, 15.0] {
                poly(&mut t, &[(x, 10.0), (x, 16.0)]);
            }
        }
    }
    for d in dots {
        gfx::dot(window, d.x, d.y, 1.6 * s, color);
    }
    if let Some(path) = t.take() {
        window.paint_path(path, color);
    }
}

struct Tip(SharedString);
impl Render for Tip {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().px_2().py_1().bg(gpui::rgb(0x222831)).border_1().border_color(border()).rounded_sm().text_size(px(11.)).text_color(text()).child(self.0.clone())
    }
}

impl Rail {
    fn btn(&self, id: &'static str, title: &'static str, icon: Icon, on: bool, enabled: bool, ev: fn() -> RailEvent, cx: &mut Context<Self>) -> impl IntoElement {
        let color: Hsla = if !enabled { hexa(0xcfcfcf, 0.3) } else if on { hex(0xffffff) } else { hex(0xb8b8b8) };
        div()
            .id(id)
            .w(px(28.))
            .h(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .border_1()
            .border_color(if on { accent() } else { gpui::rgba(0x00000000) })
            .when(on, |d| d.bg(gpui::rgb(0x1f3a66)))
            .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(gpui::rgb(0x2a2a2a))))
            .tooltip(move |_, cx| cx.new(|_| Tip(title.into())).into())
            .when(enabled, |d| d.on_click(cx.listener(move |_, _, _, cx| cx.emit(ev()))))
            .child(canvas(|_, _, _| (), move |b, _, window, _| paint_icon(icon, b, color, window)).w(px(18.)).h(px(18.)))
    }
}

impl Render for Rail {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sep = || div().w(px(22.)).h(px(1.)).bg(gpui::rgb(0x333333)).my(px(3.));
        let tool = self.tool;
        let mut col = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .py(px(4.))
            .w(px(34.))
            .h_full()
            .bg(panel())
            .border_r_1()
            .border_color(border())
            .child(self.btn("rail-cursor", "游標", Icon::Cursor, tool.is_none(), true, || RailEvent::Tool(None), cx))
            .child(sep());
        macro_rules! tb {
            ($t:expr, $id:literal) => {
                col = col.child(self.btn($id, $t.label(), Icon::Tool($t), tool == Some($t), true, || RailEvent::Tool(Some($t)), cx));
            };
        }
        tb!(Tool::Trend, "rail-trend");
        tb!(Tool::Ray, "rail-ray");
        tb!(Tool::Hline, "rail-hline");
        tb!(Tool::Vline, "rail-vline");
        tb!(Tool::Channel, "rail-channel");
        tb!(Tool::Fib, "rail-fib");
        tb!(Tool::Rect, "rail-rect");
        tb!(Tool::Text, "rail-text");
        col.child(sep())
            .child(self.btn("rail-del", "刪除選取", Icon::Delete, false, self.can_delete, || RailEvent::Delete, cx))
            .child(self.btn("rail-clear", "全部清除", Icon::Clear, false, self.can_clear, || RailEvent::Clear, cx))
    }
}

// ───────────────────────── 壓測驅動（XQ_CHART_STRESS） ─────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Ticks,
    Crosshair,
    Pan,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Ticks => "ticks",
            Mode::Crosshair => "crosshair",
            Mode::Pan => "pan",
        }
    }
}

#[derive(Clone, Copy)]
struct Snap {
    frames: u64,
    st: crate::model::LayerStats,
    t: Instant,
}

enum Stage {
    WaitQuote(Instant),
    Delay(Instant),
    HeavyPending,
    Warm(Instant),
    Run { mode: Mode, s0: Snap, steps: u64, base_right: f64 },
    Done,
}

struct StressCfg {
    drawings: usize,
    warm: Duration,
    phase: Duration,
    wait: Duration,
    t0: Instant,
}

fn env_ms(k: &str, d: u64) -> Duration {
    Duration::from_millis(std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d))
}

// ───────────────────────── ChartView ─────────────────────────

pub enum ChartEvent {
    Period(Period),
}

pub struct ChartView {
    m: M,
    plot: Entity<PlotLayer>,
    draw: Entity<DrawLayer>,
    live: Entity<LiveLayer>,
    rail: Entity<Rail>,
    focus: FocusHandle,
    menu_open: bool,
    store: Store,
    persist: bool,
    save_gen: u64,
    stress: Option<(StressCfg, Stage)>,
    stress_started: bool,
    total_steps: u64,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ChartEvent> for ChartView {}

impl ChartView {
    pub fn new(symbol: &str, name: &str, period: Period, cx: &mut Context<Self>) -> Self {
        let stress_on = std::env::var_os("XQ_CHART_STRESS").is_some_and(|v| v != "0");
        let persist = !stress_on && !std::env::var_os("XQ_BENCH").is_some_and(|v| v != "0");
        let store = if persist { Store::load() } else { Store::default() };
        let m: M = Rc::new(RefCell::new(Model::new(symbol, name, period)));
        let plot = cx.new(|_| PlotLayer { m: m.clone() });
        let draw = cx.new(|_| DrawLayer { m: m.clone() });
        let live = cx.new(|_| LiveLayer { m: m.clone() });
        let rail = cx.new(|_| Rail { tool: None, can_delete: false, can_clear: false });
        let subs = vec![cx.subscribe(&rail, |this: &mut Self, _, ev: &RailEvent, cx| {
            let f = {
                let mut m = this.m.borrow_mut();
                match ev {
                    RailEvent::Tool(t) => m.set_tool(*t),
                    RailEvent::Delete => m.delete_selected(),
                    RailEvent::Clear => m.clear_drawings(),
                }
            };
            this.refresh(f, cx);
        })];
        let stress = stress_on.then(|| {
            let now = Instant::now();
            (
                StressCfg {
                    drawings: std::env::var("XQ_CHART_DRAWINGS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000),
                    warm: env_ms("XQ_CHART_WARM_MS", 3000),
                    phase: env_ms("XQ_CHART_PHASE_MS", 10_000),
                    wait: env_ms("XQ_CHART_WAIT_MS", 15_000),
                    t0: now,
                },
                Stage::WaitQuote(now),
            )
        });
        let mut v = ChartView {
            m,
            plot,
            draw,
            live,
            rail,
            focus: cx.focus_handle(),
            menu_open: false,
            store,
            persist,
            save_gen: 0,
            stress,
            stress_started: false,
            total_steps: 0,
            _subs: subs,
        };
        v.restore_saved();
        v
    }

    pub fn symbol(&self) -> String {
        self.m.borrow().symbol().to_string()
    }
    pub fn period(&self) -> Period {
        self.m.borrow().period()
    }
    /// 三層各自的重畫次數／累計 µs（給狀態列與 metrics）
    pub fn paint_totals(&self) -> (u64, u64) {
        let st = self.m.borrow().stats;
        (st.paints.iter().sum(), st.paint_us.iter().sum())
    }

    fn refresh(&mut self, f: u8, cx: &mut Context<Self>) {
        if f == 0 {
            return;
        }
        if f & PLOT != 0 {
            self.plot.update(cx, |_, cx| cx.notify());
        }
        let draw = f & DRAW != 0 || (f & PLOT != 0 && self.m.borrow_mut().draw_layer_stale());
        if draw {
            self.draw.update(cx, |_, cx| cx.notify());
        }
        if f & LIVE != 0 {
            self.live.update(cx, |_, cx| cx.notify());
        }
        if f & HEAD != 0 {
            let (tool, can_delete, can_clear) = {
                let m = self.m.borrow();
                (m.tool, m.selected.is_some(), !m.drawings.is_empty())
            };
            self.rail.update(cx, |r, cx| {
                if r.tool != tool || r.can_delete != can_delete || r.can_clear != can_clear {
                    r.tool = tool;
                    r.can_delete = can_delete;
                    r.can_clear = can_clear;
                    cx.notify();
                }
            });
            cx.notify();
        }
        if f & SAVE != 0 {
            self.schedule_save(cx);
        }
    }

    // ───── 存檔 ─────

    fn restore_saved(&mut self) {
        if !self.persist {
            return;
        }
        let mut m = self.m.borrow_mut();
        let key = m.key();
        let e = self.store.keys.get(&key).cloned().unwrap_or_default();
        let cfg = e.cfg.or_else(|| self.store.last_cfg.clone());
        if let Some(c) = cfg {
            if c != m.cfg {
                m.set_cfg(c);
            }
        }
        m.drawings = e.drawings;
        m.selected = None;
        m.drawings_changed();
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if !self.persist {
            return;
        }
        {
            let m = self.m.borrow();
            let key = m.key();
            self.store.v = 1;
            self.store.last_cfg = Some(m.cfg.clone());
            self.store.keys.insert(key, Entry { cfg: Some(m.cfg.clone()), drawings: m.drawings.clone() });
        }
        self.save_gen += 1;
        let gen = self.save_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(450)).await;
            let _ = this.update(cx, |v, _| {
                if v.save_gen == gen {
                    v.store.save_async();
                }
            });
        })
        .detach();
    }

    // ───── 資料 ─────

    pub fn load(&mut self, symbol: &str, name: &str, period: Period, quote: Option<(f64, f64, i64)>, cx: &mut Context<Self>) {
        self.m.borrow_mut().load(symbol, name, period, quote);
        self.restore_saved();
        self.refresh(ALL, cx);
    }

    pub fn apply_quote(&mut self, price: f64, change: f64, volume: i64, name: &str, cx: &mut Context<Self>) {
        let f = self.m.borrow_mut().apply_quote(price, change, volume, name);
        if f & DRAW != 0 && self.persist {
            // 第一筆真實價重鋪 K 棒：畫線沿用
        }
        self.refresh(f, cx);
    }

    pub fn set_intraday(&mut self, symbol: &str, prev: f64, bars: &[xq_feed::Bar], cx: &mut Context<Self>) {
        let f = {
            let mut m = self.m.borrow_mut();
            if m.symbol() != symbol || m.period() != Period::Intraday {
                return;
            }
            m.set_intraday(prev, bars)
        };
        self.refresh(f, cx);
    }

    pub fn apply_minute(&mut self, symbol: &str, bar: &xq_feed::Bar, cx: &mut Context<Self>) {
        let f = {
            let mut m = self.m.borrow_mut();
            if m.symbol() != symbol || m.period() != Period::Intraday {
                return;
            }
            m.apply_minute(bar)
        };
        self.refresh(f, cx);
    }

    fn change_cfg(&mut self, f: impl FnOnce(&mut IndCfg), cx: &mut Context<Self>) {
        {
            let mut m = self.m.borrow_mut();
            let mut c = m.cfg.clone();
            f(&mut c);
            c.overlays.truncate(MAX_OVERLAYS);
            if c != m.cfg {
                m.set_cfg(c);
            }
        }
        self.refresh(ALL | SAVE, cx);
    }

    // ───── 壓測 ─────

    fn snap(&self) -> Snap {
        Snap { frames: crate::FRAMES.load(std::sync::atomic::Ordering::Relaxed), st: self.m.borrow().stats, t: Instant::now() }
    }

    fn stress_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((cfg, stage)) = self.stress.take() else { return };
        let now = Instant::now();
        let next = match stage {
            Stage::WaitQuote(t) => {
                let has = self.m.borrow().anchored;
                if has || now.duration_since(t) >= cfg.wait {
                    if !has {
                        eprintln!("CHARTPERF|{{\"phase\":\"warn\",\"msg\":\"no quote after {} ms, using synthetic bars\"}}", cfg.wait.as_millis());
                    }
                    Stage::Delay(now)
                } else {
                    Stage::WaitQuote(t)
                }
            }
            Stage::Delay(t) if now.duration_since(t) >= Duration::from_millis(300) => {
                let t_setup = Instant::now();
                {
                    let mut m = self.m.borrow_mut();
                    m.set_cfg(IndCfg::stress());
                    m.load_perf_drawings(cfg.drawings);
                    m.mouse = None;
                }
                let setup_ms = t_setup.elapsed().as_secs_f64() * 1000.0;
                eprintln!("CHARTPERF|{{\"phase\":\"setup-begin\",\"indicatorSetupMs\":{setup_ms:.2}}}");
                self.refresh(ALL, cx);
                Stage::HeavyPending
            }
            s @ Stage::Delay(_) => s,
            Stage::HeavyPending => {
                // 上一幀（含 20 疊加＋8 副圖＋1000 畫線）已畫完
                eprintln!("BENCH heavy-frame");
                let m = self.m.borrow_mut();
                let b = m.bounds.unwrap_or_default();
                let st = m.stats;
                let vis = m.gc.as_ref().map_or(0, |g| g.visible);
                let verts = m.pc.as_ref().map_or(0, |p| p.2.vertex_count());
                let first_paint_ms = st.paint_us.iter().sum::<u64>() as f64 / 1000.0;
                eprintln!(
                    "CHARTPERF|{{\"phase\":\"setup\",\"cssW\":{:.0},\"cssH\":{:.0},\"overlays\":{},\"subs\":{},\"drawings\":{},\"visibleDrawings\":{},\"drawVertices\":{},\"bars\":{},\"sinceStartMs\":{:.0},\"rebuildMs\":{:.2},\"paintMsTotal\":{:.2},\"rssMb\":{:.1}}}",
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                    m.ovs.len(),
                    m.subs.len(),
                    m.drawings.len(),
                    vis,
                    verts,
                    m.s.bars.len(),
                    cfg.t0.elapsed().as_secs_f64() * 1000.0,
                    st.rebuild_us as f64 / 1000.0,
                    first_paint_ms,
                    crate::rss_mb()
                );
                drop(m);
                eprintln!("CHARTPHASE|warm|start");
                Stage::Warm(now)
            }
            Stage::Warm(t) => {
                let f = self.stress_step(Mode::Ticks, self.total_steps, 0.0);
                self.refresh(f, cx);
                if now.duration_since(t) >= cfg.warm {
                    self.begin_phase(Mode::Ticks)
                } else {
                    Stage::Warm(t)
                }
            }
            Stage::Run { mode, s0, steps, base_right } => {
                if now.duration_since(s0.t) >= cfg.phase {
                    let s1 = self.snap();
                    let secs = s1.t.duration_since(s0.t).as_secs_f64();
                    let frames = s1.frames - s0.frames;
                    let lp: Vec<u64> = (0..3).map(|k| s1.st.paints[k] - s0.st.paints[k]).collect();
                    let lu: Vec<u64> = (0..3).map(|k| s1.st.paint_us[k] - s0.st.paint_us[k]).collect();
                    let tot_us: u64 = lu.iter().sum();
                    let per = |k: usize| if lp[k] > 0 { lu[k] as f64 / lp[k] as f64 / 1000.0 } else { 0.0 };
                    eprintln!(
                        "CHARTPERF|{{\"phase\":\"{}\",\"seconds\":{:.2},\"frames\":{},\"fps\":{:.1},\"steps\":{},\"avgPaintMs\":{:.3},\"plotPaints\":{},\"plotMs\":{:.3},\"drawPaints\":{},\"drawMs\":{:.3},\"livePaints\":{},\"liveMs\":{:.3},\"drawRebuilds\":{},\"rssMb\":{:.1}}}",
                        mode.name(),
                        secs,
                        frames,
                        frames as f64 / secs,
                        steps,
                        if frames > 0 { tot_us as f64 / frames as f64 / 1000.0 } else { 0.0 },
                        lp[0],
                        per(0),
                        lp[1],
                        per(1),
                        lp[2],
                        per(2),
                        s1.st.rebuilds - s0.st.rebuilds,
                        crate::rss_mb()
                    );
                    eprintln!("CHARTPHASE|{}|end", mode.name());
                    if mode == Mode::Pan {
                        let f = {
                            let mut m = self.m.borrow_mut();
                            m.set_right(base_right);
                            m.set_busy(false);
                            ALL
                        };
                        self.refresh(f, cx);
                    }
                    if mode == Mode::Crosshair {
                        let f = self.m.borrow_mut().mouse_move(p(-1.0, -1.0), false);
                        self.refresh(f, cx);
                    }
                    match mode {
                        Mode::Ticks => self.begin_phase(Mode::Crosshair),
                        Mode::Crosshair => self.begin_phase(Mode::Pan),
                        Mode::Pan => {
                            eprintln!("CHARTPERF|{{\"phase\":\"done\",\"sinceStartMs\":{:.0}}}", cfg.t0.elapsed().as_secs_f64() * 1000.0);
                            Stage::Done
                        }
                    }
                } else {
                    let f = self.stress_step(mode, steps, base_right);
                    self.refresh(f, cx);
                    Stage::Run { mode, s0, steps: steps + 1, base_right }
                }
            }
            Stage::Done => {
                // 結束後繼續跳價（量 CPU 的腳本可以晚一點收），不再回報
                let f = self.stress_step(Mode::Ticks, self.total_steps, 0.0);
                self.refresh(f, cx);
                Stage::Done
            }
        };
        self.stress = Some((cfg, next));
        cx.on_next_frame(window, |this, window, cx| this.stress_frame(window, cx));
    }

    fn begin_phase(&mut self, mode: Mode) -> Stage {
        eprintln!("CHARTPHASE|{}|start", mode.name());
        let base_right = {
            let mut m = self.m.borrow_mut();
            if mode == Mode::Pan {
                m.set_busy(true);
            }
            m.view.map_or(0.0, |v| v.right)
        };
        Stage::Run { mode, s0: self.snap(), steps: 0, base_right }
    }

    /// 一幀的動作：跳一筆價＋（十字線移動｜平移）。與 wry `__perfContinuous` 同公式。
    fn stress_step(&mut self, mode: Mode, f: u64, base_right: f64) -> u8 {
        self.total_steps += 1;
        let mut m = self.m.borrow_mut();
        let mut fl = m.synth_tick(f);
        match mode {
            Mode::Ticks => {}
            Mode::Crosshair | Mode::Pan => {
                // wry 的 pan 也同時移動十字線
                if let Some(b) = m.bounds {
                    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                    let x = 80.0 + ((f * 13) as f32) % (w - 160.0).max(40.0);
                    let y = 40.0 + ((f * 7) as f32) % (h * 0.5).max(40.0);
                    fl |= m.mouse_move(p(f32::from(b.origin.x) + x, f32::from(b.origin.y) + y), true);
                }
            }
        }
        if mode == Mode::Pan {
            m.set_right(base_right + (f as f64 / 6.0).sin() * 12.0);
            fl |= PLOT | DRAW | LIVE;
        }
        // 壓測只量走勢圖本身（wry `__perfContinuous` 也只重畫 canvas），不重繪標頭
        fl & !HEAD
    }
}

// ───────────────────────── 版面與事件 ─────────────────────────

impl ChartView {
    fn on_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if self.menu_open {
            self.menu_open = false;
            cx.notify();
        }
        let q = p(f32::from(ev.position.x), f32::from(ev.position.y));
        let f = self.m.borrow_mut().mouse_down(q, ev.click_count);
        self.refresh(f, cx);
    }

    fn on_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let q = p(f32::from(ev.position.x), f32::from(ev.position.y));
        let f = {
            let mut m = self.m.borrow_mut();
            let inside = m.bounds.is_some_and(|b| b.contains(&ev.position));
            if !inside && m.mouse.is_none() && m.pan.is_none() && m.drag.is_none() && m.pending.is_none() {
                return;
            }
            m.mouse_move(q, inside)
        };
        self.refresh(f, cx);
    }

    fn on_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let q = p(f32::from(ev.position.x), f32::from(ev.position.y));
        let f = {
            let mut m = self.m.borrow_mut();
            if m.pan.is_none() && m.drag.is_none() && !m.pending.as_ref().is_some_and(|p| p.dragging) && !m.busy {
                return;
            }
            m.mouse_up(q)
        };
        self.refresh(f, cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let f = {
            let mut m = self.m.borrow_mut();
            if let Some(te) = &mut m.text_edit {
                match key {
                    "enter" => m.commit_text(true),
                    "escape" => m.commit_text(false),
                    "backspace" => {
                        te.text.pop();
                        LIVE
                    }
                    _ => {
                        let ch = ev.keystroke.key_char.clone().filter(|c| !c.chars().any(char::is_control));
                        match ch {
                            Some(c) if !ev.keystroke.modifiers.control && !ev.keystroke.modifiers.platform => {
                                te.text.push_str(&c);
                                LIVE
                            }
                            _ => 0,
                        }
                    }
                }
            } else {
                match key {
                    "delete" | "backspace" => m.delete_selected(),
                    "escape" => m.escape(),
                    _ => 0,
                }
            }
        };
        if f != 0 {
            cx.stop_propagation();
        }
        self.refresh(f, cx);
    }

    fn render_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.m.borrow().cfg.clone();
        let small = |id: SharedString, label: &'static str| {
            div()
                .id(id)
                .px(px(5.))
                .rounded_sm()
                .border_1()
                .border_color(border())
                .cursor_pointer()
                .hover(|s| s.bg(selected()))
                .child(label)
        };
        let full = cfg.overlays.len() >= MAX_OVERLAYS;
        let mut ov_rows = div().flex().flex_col().gap(px(3.));
        for (k, o) in cfg.overlays.iter().enumerate() {
            let col = gpui::rgb(OV_COLORS[k % 20]);
            ov_rows = ov_rows.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(div().w(px(110.)).text_color(col).child(o.label()))
                    .child(small(format!("ov-m-{k}").into(), "−").on_click(cx.listener(move |this, _, _, cx| {
                        this.change_cfg(|c| step_ov(&mut c.overlays[k], -1.0), cx)
                    })))
                    .child(small(format!("ov-p-{k}").into(), "+").on_click(cx.listener(move |this, _, _, cx| {
                        this.change_cfg(|c| step_ov(&mut c.overlays[k], 1.0), cx)
                    })))
                    .child(small(format!("ov-x-{k}").into(), "×").on_click(cx.listener(move |this, _, _, cx| {
                        this.change_cfg(|c| {
                            c.overlays.remove(k);
                        }, cx)
                    }))),
            );
        }
        let add = |kind: &'static str, label: &'static str, cx: &mut Context<Self>| {
            small(format!("ov-add-{kind}").into(), label)
                .when(full, |d| d.opacity(0.4))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_cfg(|c| {
                        if c.overlays.len() < MAX_OVERLAYS {
                            c.overlays.push(OvSpec::defaults(kind));
                        }
                    }, cx)
                }))
        };
        let n_on = cfg.subs.iter().filter(|s| s.on).count();
        let mut subs = div().flex().flex_col().gap(px(3.));
        for (k, s) in cfg.subs.iter().enumerate() {
            let on = s.on;
            let params = s.kind.params();
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .id(SharedString::from(format!("sub-{k}")))
                        .w(px(130.))
                        .cursor_pointer()
                        .text_color(if on { text() } else { dim() })
                        .child(format!("{} {}", if on { "☑" } else { "☐" }, s.kind.label()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change_cfg(|c| {
                                let on_now = c.subs.iter().filter(|s| s.on).count();
                                if c.subs[k].on || on_now < MAX_SUBS {
                                    c.subs[k].on = !c.subs[k].on;
                                }
                            }, cx)
                        })),
                );
            for (pi, (pn, _)) in params.iter().enumerate() {
                row = row
                    .child(div().text_color(dim()).child(format!("{pn} {}", s.p[pi])))
                    .child(small(format!("sp-m-{k}-{pi}").into(), "−").on_click(cx.listener(move |this, _, _, cx| {
                        this.change_cfg(|c| c.subs[k].p[pi] = (c.subs[k].p[pi] - 1.0).max(1.0), cx)
                    })))
                    .child(small(format!("sp-p-{k}-{pi}").into(), "+").on_click(cx.listener(move |this, _, _, cx| {
                        this.change_cfg(|c| c.subs[k].p[pi] = (c.subs[k].p[pi] + 1.0).min(250.0), cx)
                    })));
            }
            if s.kind == SubKind::Vol {
                let vm = cfg.volma.0;
                row = row.child(
                    div()
                        .id("volma")
                        .cursor_pointer()
                        .text_color(if vm { text() } else { dim() })
                        .child(format!("{} 均量 MA{}/{}", if vm { "☑" } else { "☐" }, cfg.volma.1[0], cfg.volma.1[1]))
                        .on_click(cx.listener(|this, _, _, cx| this.change_cfg(|c| c.volma.0 = !c.volma.0, cx))),
                );
            }
            subs = subs.child(row);
        }
        div()
            .id("ind-menu")
            .absolute()
            .top(px(28.))
            .right(px(8.))
            .w(px(420.))
            .max_h(px(520.))
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .bg(panel())
            .border_1()
            .border_color(border())
            .rounded_md()
            .text_size(px(11.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_color(dim()).child(format!("主圖疊加（{}/{MAX_OVERLAYS}）", cfg.overlays.len())))
            .child(ov_rows)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(add("ma", "+ MA", cx))
                    .child(add("ema", "+ EMA", cx))
                    .child(add("bb", "+ 布林", cx))
                    .child(add("sar", "+ SAR", cx)),
            )
            .child(div().h(px(1.)).bg(border()))
            .child(div().text_color(dim()).child(format!("副圖（{n_on}/{}，最多 {MAX_SUBS}）", cfg.subs.len())))
            .child(subs)
            .child(
                div().flex().flex_row().gap(px(6.)).child(small("cfg-reset".into(), "恢復預設").on_click(cx.listener(|this, _, _, cx| {
                    this.change_cfg(|c| *c = IndCfg::default(), cx)
                }))),
            )
    }
}

fn step_ov(o: &mut OvSpec, d: f64) {
    match o {
        OvSpec::Ma { period } | OvSpec::Ema { period } => *period = ((*period as f64 + d).clamp(1.0, 500.0)) as usize,
        OvSpec::Bb { n, .. } => *n = ((*n as f64 + d).clamp(2.0, 500.0)) as usize,
        OvSpec::Sar { step, max } => {
            *step = ((*step + d * 0.01).clamp(0.01, *max) * 100.0).round() / 100.0;
        }
    }
}

impl Render for ChartView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.stress.is_some() && !self.stress_started {
            self.stress_started = true;
            cx.on_next_frame(window, |this, window, cx| this.stress_frame(window, cx));
        }
        let (title, price, change, volume, period, tool, n_draw) = {
            let m = self.m.borrow();
            (format!("{} {}", m.symbol(), m.name), m.price, m.change, m.volume, m.period(), m.tool, m.drawings.len())
        };
        let color = dir_color(change);
        let has = price > 0.0;
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
        let full = || StyleRefinement::default().size_full();
        let me = cx.entity().downgrade();
        let menu_open = self.menu_open;
        let hint = match tool {
            Some(Tool::Text) => "點一下放文字".to_string(),
            Some(Tool::Channel) => "拖出基準線，再點一下決定通道寬度".to_string(),
            Some(t) if t.need() == 1 => format!("{}：點一下放置", t.label()),
            Some(t) => format!("{}：按住拖曳（或點兩下）", t.label()),
            None if n_draw > 0 => format!("畫線 {n_draw}"),
            None => String::new(),
        };

        div()
            .relative()
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
                    .child(div().text_size(px(13.)).child(title))
                    .child(div().font_family(MONO_FONT).text_color(color).child(if has { format!("{price:.2}") } else { "--".into() }))
                    .child(div().font_family(MONO_FONT).text_color(color).child(if has {
                        format!("{} ({:+.2}%)", fmt_change(change), pct(price, change))
                    } else {
                        String::new()
                    }))
                    .child(div().text_color(dim()).child(if has { format!("量 {}", fmt_int(volume)) } else { String::new() }))
                    .child(div().text_color(dim()).text_size(px(11.)).child(hint))
                    .child(div().flex_1())
                    .children(Period::ALL.iter().map(|&p| period_btn(p, cx)))
                    .child(
                        div()
                            .id("ind-btn")
                            .px_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .border_1()
                            .border_color(if menu_open { accent() } else { border() })
                            .hover(|s| s.bg(selected()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.menu_open = !this.menu_open;
                                cx.notify();
                            }))
                            .child("指標 ▾"),
                    )
                    .child(
                        div()
                            .id("reset-zoom")
                            .px_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .border_1()
                            .border_color(border())
                            .hover(|s| s.bg(selected()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let f = this.m.borrow_mut().reset_view();
                                this.refresh(f, cx);
                            }))
                            .child("重設縮放"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.rail.clone().cached(StyleRefinement::default().h_full().w(px(34.)).flex_none()))
                    .child(
                        div()
                            .id("chart-stage")
                            .relative()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .track_focus(&self.focus)
                            .cursor(if tool.is_some() { gpui::CursorStyle::Crosshair } else { gpui::CursorStyle::Arrow })
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
                            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                                let f = this.m.borrow_mut().wheel(dy);
                                this.refresh(f, cx);
                            }))
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
                            .child(div().absolute().inset_0().child(self.plot.clone().cached(full())))
                            .child(div().absolute().inset_0().child(self.draw.clone().cached(full())))
                            .child(div().absolute().inset_0().child(self.live.clone().cached(full())))
                            .child(
                                // 視窗層級的 move／up（拖曳出圖外也收得到）
                                canvas(
                                    |_, _, _| (),
                                    move |_, _, window, _| {
                                        let me1 = me.clone();
                                        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                            if phase == DispatchPhase::Bubble {
                                                let _ = me1.update(cx, |v, cx| v.on_move(ev, cx));
                                            }
                                        });
                                        let me2 = me.clone();
                                        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                                                let _ = me2.update(cx, |v, cx| v.on_up(ev, cx));
                                            }
                                        });
                                    },
                                )
                                .absolute()
                                .size_0(),
                            ),
                    ),
            )
            .when(menu_open, |d| d.child(self.render_menu(cx)))
    }
}
