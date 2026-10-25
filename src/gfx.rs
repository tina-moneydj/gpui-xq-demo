//! 低階繪圖小工具：自己把線段展開成三角形（不經 lyon），一個顏色一條 Path，
//! 可整條平移（畫線層平移快取用）。軸向 1px 線一律用 quad。

use gpui::{
    font, point, px, quad, size, App, Background, Bounds, Hsla, Path, Pixels, Point, Rgba, SharedString, ShapedLine,
    TextAlign, TextRun, Window,
};

use crate::theme::mono_font;

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

    pub fn from_path(path: Path<Pixels>) -> Tris {
        Tris { path: Some(path) }
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
    TextRun { len, font: font(mono_font()), color, background_color: None, underline: None, strikethrough: None }
}

// ---------------------------------------------------------------- 等寬 ASCII 快速路徑（2026-10-25，Windows CPU）
// 價格、漲跌、量、座標軸數字每幀都在變，走 shape_line 每個新字串都要平台排字一次（Windows 是 DirectWrite：
// CreateTextFormat＋CreateTextLayout＋Draw，CI 取樣主執行緒約 10% 在 DWrite／TextShaping；Linux cosmic-text 約 16%），
// 而且每行文字各包一個 paint_layer。等寬字的 ASCII 每個字的字形和寬度是固定的：每個字級只排一次 0x20..0x7E，
// 之後直接 paint_glyph，不經排字、不建 ShapedLine。字型不是單一字型（有後援字型）或字級太多就退回 shape_line。
// XQ_MONO_FAST=0 關掉（對照用）。

const ASCII0: u8 = 0x20;
const NASCII: usize = 95;
/// 最多記幾種字級（畫面上只有 3–4 種；超過就整個清掉重來，不會一直長大）
const MAX_FACES: usize = 8;

#[derive(Clone, Copy)]
struct MonoFace {
    size_bits: u32,
    /// None＝這個字級排出來不是單一等寬字型，不走快速路徑
    ok: Option<(gpui::FontId, f32, f32)>,
    gid: [Option<gpui::GlyphId>; NASCII],
    adv: [f32; NASCII],
}

thread_local! {
    static MONO: std::cell::RefCell<Vec<MonoFace>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn mono_fast() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::plat::var("XQ_MONO_FAST").map_or(true, |v| v != "0"))
}

fn build_face(window: &mut Window, size_px: f32) -> MonoFace {
    let mut f = MonoFace { size_bits: size_px.to_bits(), ok: None, gid: [None; NASCII], adv: [0.0; NASCII] };
    let all: String = (ASCII0..ASCII0 + NASCII as u8).map(char::from).collect();
    let line = shape(window, all, size_px, gpui::black());
    let lay: &gpui::LineLayout = &line;
    let [run] = lay.runs.as_slice() else { return f };
    if run.glyphs.len() != NASCII {
        return f;
    }
    for (k, g) in run.glyphs.iter().enumerate() {
        // 每個字剛好一個字形、照順序（等寬 ASCII 不會有連字／重排）
        if g.index != k || g.is_emoji {
            return f;
        }
        let next = run.glyphs.get(k + 1).map_or(lay.width, |n| n.position.x);
        f.adv[k] = f32::from(next - g.position.x);
        f.gid[k] = Some(g.id);
    }
    f.ok = Some((run.font_id, f32::from(lay.ascent), f32::from(lay.descent)));
    f
}

fn with_face<R>(window: &mut Window, size_px: f32, g: impl FnOnce(&MonoFace) -> R) -> Option<R> {
    if !mono_fast() {
        return None;
    }
    let bits = size_px.to_bits();
    let hit = MONO.with(|m| m.borrow().iter().position(|f| f.size_bits == bits));
    let ix = match hit {
        Some(ix) => ix,
        None => {
            let face = build_face(window, size_px);
            MONO.with(|m| {
                let mut m = m.borrow_mut();
                if m.len() >= MAX_FACES {
                    m.clear();
                }
                m.push(face);
                m.len() - 1
            })
        }
    };
    MONO.with(|m| m.borrow().get(ix).copied()).filter(|f| f.ok.is_some()).map(|f| g(&f))
}

fn face_width(f: &MonoFace, s: &str) -> Option<f32> {
    let mut w = 0.0;
    for b in s.bytes() {
        let k = b.wrapping_sub(ASCII0) as usize;
        if k >= NASCII || f.gid[k].is_none() {
            return None;
        }
        w += f.adv[k];
    }
    Some(w)
}

/// 等寬 ASCII 字串寬度；None＝不能走快速路徑（有中文或其他字、字型不支援）
pub fn mono_width(window: &mut Window, s: &str, size_px: f32) -> Option<f32> {
    with_face(window, size_px, |f| face_width(f, s)).flatten()
}

/// 在 (x, 列頂 y_top) 畫一行等寬 ASCII，垂直置中於 line_h（同 ShapedLine::paint 的基線算法）。
/// 回傳寬度；None＝沒畫，呼叫端改走 shape_line。
pub fn mono_paint(window: &mut Window, s: &str, x: f32, y_top: f32, line_h: f32, size_px: f32, color: Hsla) -> Option<f32> {
    let f = with_face(window, size_px, |f| *f)?;
    let (font_id, ascent, descent) = f.ok?;
    let w = face_width(&f, s)?;
    let base = y_top + (line_h - ascent - descent) / 2.0 + ascent;
    let mut gx = x;
    for b in s.bytes() {
        let k = (b - ASCII0) as usize;
        if b != b' ' {
            if let Some(gid) = f.gid[k] {
                let _ = window.paint_glyph(point(px(gx), px(base)), font_id, gid, px(size_px), color);
            }
        }
        gx += f.adv[k];
    }
    Some(w)
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
    let s: SharedString = s.into();
    let lh = size_px + 3.0;
    if let Some(w) = mono_paint(window, &s, x, y_mid - lh / 2.0, lh, size_px, color) {
        return w;
    }
    let l = shape(window, s, size_px, color);
    paint_line(window, cx, &l, x, y_mid, size_px);
    f32::from(l.width())
}

pub fn text_right(window: &mut Window, cx: &mut App, s: impl Into<SharedString>, right: f32, y_mid: f32, size_px: f32, color: Hsla) {
    let s: SharedString = s.into();
    let lh = size_px + 3.0;
    if let Some(w) = mono_width(window, &s, size_px) {
        mono_paint(window, &s, right - w, y_mid - lh / 2.0, lh, size_px, color);
        return;
    }
    let l = shape(window, s, size_px, color);
    let w = f32::from(l.width());
    paint_line(window, cx, &l, right - w, y_mid, size_px);
}

pub fn text_center(window: &mut Window, cx: &mut App, s: impl Into<SharedString>, cx_: f32, y_mid: f32, size_px: f32, color: Hsla) -> f32 {
    let s: SharedString = s.into();
    let lh = size_px + 3.0;
    if let Some(w) = mono_width(window, &s, size_px) {
        mono_paint(window, &s, cx_ - w / 2.0, y_mid - lh / 2.0, lh, size_px, color);
        return w;
    }
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
