//! 低階繪圖小工具：自己把線段展開成三角形（不經 lyon），一個顏色一條 Path，
//! 可整條平移（畫線層平移快取用）。軸向 1px 線一律用 quad。

use gpui::{
    font, point, px, quad, size, App, Background, Bounds, Hsla, Path, Pixels, Point, Rgba, SharedString, ShapedLine,
    TextAlign, TextRun, Window,
};

use crate::theme::MONO_FONT;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

pub const fn p(x: f32, y: f32) -> P {
    P { x, y }
}

pub fn hex(c: u32) -> Hsla {
    gpui::rgb(c).into()
}

pub fn hexa(c: u32, a: f32) -> Hsla {
    let mut h: Hsla = gpui::rgb(c).into();
    h.a = a;
    h
}

/// 三角形累積器：第一個三角形才建立 Path（避免 bounds 從 (0,0) 起算）。
#[derive(Default)]
pub struct Tris {
    path: Option<Path<Pixels>>,
}

impl Tris {
    #[inline]
    pub fn tri(&mut self, a: P, b: P, c: P) {
        let pt = |q: P| point(px(q.x), px(q.y));
        let path = self.path.get_or_insert_with(|| Path::new(pt(a)));
        let s = point(0.0f32, 1.0f32);
        path.push_triangle((pt(a), pt(b), pt(c)), (s, s, s));
    }

    /// 線段 → 兩個三角形（兩端各外推半個線寬，折線接縫不會缺角）
    #[inline]
    pub fn seg(&mut self, a: P, b: P, w: f32) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if !(len > 1e-3) || !len.is_finite() {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let h = w * 0.5;
        let (nx, ny) = (-uy * h, ux * h);
        let (ex, ey) = (ux * h * 0.5, uy * h * 0.5);
        let a0 = p(a.x - ex + nx, a.y - ey + ny);
        let a1 = p(a.x - ex - nx, a.y - ey - ny);
        let b0 = p(b.x + ex + nx, b.y + ey + ny);
        let b1 = p(b.x + ex - nx, b.y + ey - ny);
        self.tri(a0, b0, b1);
        self.tri(a0, b1, a1);
    }

    /// 虛線（dash 實、gap 空）
    pub fn dashed(&mut self, a: P, b: P, w: f32, dash: f32, gap: f32) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if !(len > 1e-3) || !len.is_finite() {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let mut t = 0.0;
        while t < len {
            let e = (t + dash).min(len);
            self.seg(p(a.x + ux * t, a.y + uy * t), p(a.x + ux * e, a.y + uy * e), w);
            t += dash + gap;
        }
    }

    /// 凸多邊形填色（扇形三角化）
    pub fn poly(&mut self, pts: &[P]) {
        for k in 1..pts.len().saturating_sub(1) {
            self.tri(pts[0], pts[k], pts[k + 1]);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.path.is_none()
    }

    pub fn take(&mut self) -> Option<Path<Pixels>> {
        self.path.take()
    }
}

/// 折線：NaN 斷開
pub fn polyline(t: &mut Tris, w: f32, mut pts: impl Iterator<Item = Option<P>>) {
    let mut prev: Option<P> = None;
    while let Some(q) = pts.next() {
        match q {
            Some(q) if q.x.is_finite() && q.y.is_finite() => {
                if let Some(a) = prev {
                    t.seg(a, q, w);
                }
                prev = Some(q);
            }
            _ => prev = None,
        }
    }
}

/// 快取的 Path 整條水平平移（只改頂點座標，不重新三角化）
pub fn shifted(path: &Path<Pixels>, dx: f32) -> Path<Pixels> {
    let mut p = path.clone();
    if dx != 0.0 {
        let d = px(dx);
        for v in &mut p.vertices {
            v.xy_position.x += d;
        }
        p.bounds.origin.x += d;
    }
    p
}

#[inline]
pub fn rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: impl Into<Background>) {
    if !(w > 0.0 && h > 0.0) {
        return;
    }
    window.paint_quad(gpui::fill(Bounds::new(point(px(x), px(y)), size(px(w), px(h))), color));
}

pub fn boxed(window: &mut Window, x: f32, y: f32, w: f32, h: f32, bg: impl Into<Background>, border: Hsla, bw: f32) {
    window.paint_quad(quad(
        Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        px(0.),
        bg,
        px(bw),
        border,
        Default::default(),
    ));
}

pub fn dot(window: &mut Window, x: f32, y: f32, r: f32, color: Hsla) {
    window.paint_quad(quad(
        Bounds::new(point(px(x - r), px(y - r)), size(px(2.0 * r), px(2.0 * r))),
        px(r),
        color,
        px(0.),
        gpui::transparent_black(),
        Default::default(),
    ));
}

/// 水平虛線（quad）
pub fn hdash(window: &mut Window, x0: f32, x1: f32, y: f32, color: Hsla, dash: f32, gap: f32) {
    let mut x = x0;
    while x < x1 {
        rect(window, x, y, dash.min(x1 - x), 1.0, color);
        x += dash + gap;
    }
}

pub fn vdash(window: &mut Window, x: f32, y0: f32, y1: f32, color: Hsla, dash: f32, gap: f32) {
    let mut y = y0;
    while y < y1 {
        rect(window, x, y, 1.0, dash.min(y1 - y), color);
        y += dash + gap;
    }
}

pub fn run(len: usize, color: Hsla) -> TextRun {
    TextRun { len, font: font(MONO_FONT), color, background_color: None, underline: None, strikethrough: None }
}

pub fn shape(window: &mut Window, s: impl Into<SharedString>, size_px: f32, color: Hsla) -> ShapedLine {
    let s: SharedString = s.into();
    let r = run(s.len(), color);
    window.text_system().shape_line(s, px(size_px), &[r], None)
}

/// 多色一行（圖例）：items 之間空 2 格
pub fn shape_items(window: &mut Window, items: &[(String, Hsla)], size_px: f32) -> ShapedLine {
    let mut s = String::new();
    let mut runs = Vec::with_capacity(items.len() * 2);
    for (k, (t, c)) in items.iter().enumerate() {
        if k > 0 {
            s.push_str("  ");
            runs.push(run(2, *c));
        }
        s.push_str(t);
        runs.push(run(t.len(), *c));
    }
    window.text_system().shape_line(s.into(), px(size_px), &runs, None)
}

/// 垂直置中於 y
pub fn paint_line(window: &mut Window, cx: &mut App, line: &ShapedLine, x: f32, y_mid: f32, size_px: f32) {
    let lh = size_px + 3.0;
    let _ = line.paint(point(px(x), px(y_mid - lh / 2.0)), px(lh), TextAlign::Left, None, window, cx);
}

pub fn text(window: &mut Window, cx: &mut App, s: impl Into<SharedString>, x: f32, y_mid: f32, size_px: f32, color: Hsla) -> f32 {
    let l = shape(window, s, size_px, color);
    paint_line(window, cx, &l, x, y_mid, size_px);
    f32::from(l.width())
}

pub fn text_right(window: &mut Window, cx: &mut App, s: impl Into<SharedString>, right: f32, y_mid: f32, size_px: f32, color: Hsla) {
    let l = shape(window, s, size_px, color);
    let w = f32::from(l.width());
    paint_line(window, cx, &l, right - w, y_mid, size_px);
}

pub fn text_center(window: &mut Window, cx: &mut App, s: impl Into<SharedString>, cx_: f32, y_mid: f32, size_px: f32, color: Hsla) -> f32 {
    let l = shape(window, s, size_px, color);
    let w = f32::from(l.width());
    paint_line(window, cx, &l, cx_ - w / 2.0, y_mid, size_px);
    w
}

pub fn rgba_to_hsla(c: Rgba) -> Hsla {
    c.into()
}

pub fn pt(q: P) -> Point<Pixels> {
    point(px(q.x), px(q.y))
}
