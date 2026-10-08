//! 畫線工具（與 wry 版相同 8 種）：幾何、空間索引 hit-test、畫線層快取。
//!
//! 效能：
//! - 畫線層只在「尺度」改變（價格軸 lo/hi、每根寬、版面、畫線內容／選取）時重建；
//!   純平移（view.right 改變）沿用快取，只把頂點整批水平位移（不重新三角化、不重新排字）。
//! - 重建時先用 bounding box 剔除不在可視區（左右各多留半個寬度給平移）的畫線。
//! - hit-test 用 64px 格子的空間索引，只測游標附近 3×3 格的候選。
//! - 被選取／繪製中的那條不進快取，在 live 層每幀即時畫（含控制點）。

use gpui::{px, App, Bounds, ContentMask, Hsla, Path, Pixels, ShapedLine, Window};
use serde::{Deserialize, Serialize};

use crate::gfx::{self, p, Tris, P};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Trend,
    Ray,
    Hline,
    Vline,
    Channel,
    Fib,
    Rect,
    Text,
}

impl Tool {
    pub const ALL: [Tool; 8] = [Tool::Trend, Tool::Ray, Tool::Hline, Tool::Vline, Tool::Channel, Tool::Fib, Tool::Rect, Tool::Text];
    pub fn need(self) -> usize {
        match self {
            Tool::Hline | Tool::Vline | Tool::Text => 1,
            Tool::Channel => 3,
            _ => 2,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tool::Trend => "趨勢線",
            Tool::Ray => "射線",
            Tool::Hline => "水平線",
            Tool::Vline => "垂直線",
            Tool::Channel => "平行通道",
            Tool::Fib => "費波納契回撤",
            Tool::Rect => "矩形",
            Tool::Text => "文字註記",
        }
    }
}

/// 錨點：i = K 棒索引（可帶小數），p = 價格
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub i: f64,
    pub p: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    #[serde(rename = "type")]
    pub kind: Tool,
    pub pts: Vec<Pt>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

pub const FIB: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
pub const FIB_COLORS: [u32; 7] = [0x9e9e9e, 0xe5484d, 0xf5a623, 0x4caf50, 0x26a69a, 0x4fc3f7, 0x9e9e9e];
pub const DRAW_COLOR: u32 = 0xffb000;
pub const SEL_COLOR: u32 = 0xffffff;

/// 價格窗格的座標轉換（與 wry xOf / yOf 相同）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub plot_l: f32,
    pub plot_r: f32,
    pub right: f64,
    pub bar_w: f64,
    /// 窗格頂（含圖例列）與高
    pub pane_top: f32,
    pub pane_h: f32,
    /// 繪圖區（扣掉圖例）
    pub y0: f32,
    pub ph: f32,
    pub lo: f64,
    pub hi: f64,
}

impl Xf {
    #[inline]
    pub fn x(&self, i: f64) -> f32 {
        (self.plot_r as f64 - (self.right - i) * self.bar_w - self.bar_w / 2.0) as f32
    }
    #[inline]
    pub fn y(&self, v: f64) -> f32 {
        (self.y0 as f64 + self.ph as f64 * (1.0 - (v - self.lo) / (self.hi - self.lo))) as f32
    }
    pub fn i_at(&self, x: f32) -> f64 {
        self.right - (self.plot_r as f64 - self.bar_w / 2.0 - x as f64) / self.bar_w
    }
    pub fn p_at(&self, y: f32) -> f64 {
        self.lo + (1.0 - (y - self.y0) as f64 / self.ph as f64) * (self.hi - self.lo)
    }
    pub fn pt(&self, a: Pt) -> P {
        p(self.x(a.i), self.y(a.p))
    }
    pub fn clip(&self) -> Bounds<Pixels> {
        Bounds::new(gpui::point(px(self.plot_l), px(self.pane_top)), gpui::size(px(self.plot_r - self.plot_l), px(self.pane_h)))
    }
}

#[derive(Clone, Debug)]
pub struct Level {
    pub k: usize,
    pub p: f64,
    pub y: f32,
    pub x1: f32,
    pub x2: f32,
}

/// 一條畫線的螢幕幾何（與 wry geom() 相同）
#[derive(Clone, Debug, Default)]
pub struct Geom {
    pub segs: Vec<[P; 2]>,
    pub fill: Option<[P; 4]>,
    pub mid: Option<[P; 2]>,
    pub levels: Vec<Level>,
    pub diag: Option<[P; 2]>,
    pub fill_box: Option<[f32; 4]>,
    pub text_box: Option<[f32; 4]>,
    pub handles: Vec<P>,
    pub bbox: [f32; 4],
}

/// 字寬估計（等寬字型：ASCII 0.6em、CJK 1.2em）——幾何／hit-test 不必碰排字系統
pub fn text_width(s: &str, size: f32) -> f32 {
    s.chars().map(|c| if c.is_ascii() { 0.6 } else { 1.2 }).sum::<f32>() * size
}

fn price_line(a: Pt, b: Pt, i: f64) -> f64 {
    if b.i == a.i { a.p } else { a.p + (b.p - a.p) * (i - a.i) / (b.i - a.i) }
}

pub fn geom(d: &Drawing, xf: &Xf) -> Geom {
    let pts: Vec<P> = d.pts.iter().map(|&a| xf.pt(a)).collect();
    let mut g = Geom { handles: pts.clone(), ..Default::default() };
    match d.kind {
        Tool::Trend => g.segs.push([pts[0], pts[1]]),
        Tool::Ray => {
            let (dx, dy) = (pts[1].x - pts[0].x, pts[1].y - pts[0].y);
            let t = 20000.0 / dx.abs().max(dy.abs()).max(1.0);
            g.segs.push([pts[0], p(pts[0].x + dx * t, pts[0].y + dy * t)]);
        }
        Tool::Hline => {
            g.segs.push([p(xf.plot_l, pts[0].y), p(xf.plot_r, pts[0].y)]);
            g.handles = vec![p(pts[0].x.clamp(xf.plot_l + 6.0, xf.plot_r - 6.0), pts[0].y)];
        }
        Tool::Vline => {
            g.segs.push([p(pts[0].x, xf.pane_top), p(pts[0].x, xf.pane_top + xf.pane_h)]);
            g.handles = vec![p(pts[0].x, pts[0].y.clamp(xf.pane_top + 6.0, xf.pane_top + xf.pane_h - 6.0))];
        }
        Tool::Channel => {
            let (a, b) = (d.pts[0], d.pts[1]);
            let c = *d.pts.get(2).unwrap_or(&b);
            let off = c.p - price_line(a, b, c.i);
            let a2 = xf.pt(Pt { i: a.i, p: a.p + off });
            let b2 = xf.pt(Pt { i: b.i, p: b.p + off });
            let am = xf.pt(Pt { i: a.i, p: a.p + off / 2.0 });
            let bm = xf.pt(Pt { i: b.i, p: b.p + off / 2.0 });
            g.segs.push([pts[0], pts[1]]);
            if d.pts.len() >= 3 {
                g.segs.push([a2, b2]);
            }
            g.mid = Some([am, bm]);
            g.fill = Some([pts[0], pts[1], b2, a2]);
        }
        Tool::Fib => {
            let (a, b) = (d.pts[0], d.pts[1]);
            let x1 = pts[0].x.min(pts[1].x);
            let x2 = pts[0].x.max(pts[1].x).max(x1 + 40.0);
            for (k, lv) in FIB.iter().enumerate() {
                let pr = b.p + (a.p - b.p) * lv;
                let y = xf.y(pr);
                g.levels.push(Level { k, p: pr, y, x1, x2 });
                g.segs.push([p(x1, y), p(x2, y)]);
            }
            g.diag = Some([pts[0], pts[1]]);
            g.fill_box = Some([x1, pts[0].y.min(pts[1].y), x2, pts[0].y.max(pts[1].y)]);
        }
        Tool::Rect => {
            let (a, b) = (pts[0], pts[1]);
            let r = p(b.x, a.y);
            let s = p(a.x, b.y);
            g.segs.extend([[a, r], [r, b], [b, s], [s, a]]);
            g.fill = Some([a, r, b, s]);
            g.handles = vec![a, b, r, s];
        }
        Tool::Text => {
            let w = text_width(&d.text, 13.0) + 12.0;
            g.text_box = Some([pts[0].x, pts[0].y - 11.0, w, 22.0]);
        }
    }
    g.bbox = bbox(&g, 8.0);
    g
}

fn bbox(g: &Geom, pad: f32) -> [f32; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut add = |q: P| {
        if q.x.is_finite() && q.y.is_finite() {
            x0 = x0.min(q.x);
            y0 = y0.min(q.y);
            x1 = x1.max(q.x);
            y1 = y1.max(q.y);
        }
    };
    for s in &g.segs {
        add(s[0]);
        add(s[1]);
    }
    for &h in &g.handles {
        add(h);
    }
    if let Some(f) = &g.fill {
        f.iter().for_each(|&q| add(q));
    }
    if let Some(m) = &g.mid {
        add(m[0]);
        add(m[1]);
    }
    if let Some(b) = g.fill_box {
        add(p(b[0], b[1]));
        add(p(b[2], b[3]));
    }
    if let Some(b) = g.text_box {
        add(p(b[0], b[1]));
        add(p(b[0] + b[2], b[1] + b[3]));
    }
    [x0 - pad, y0 - pad, x1 + pad, y1 + pad]
}

fn dist_seg(q: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    ((q.x - (a.x + t * dx)).powi(2) + (q.y - (a.y + t * dy)).powi(2)).sqrt()
}

fn in_poly(q: P, poly: &[P]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn hit_geom(q: P, g: &Geom) -> bool {
    if q.x < g.bbox[0] || q.x > g.bbox[2] || q.y < g.bbox[1] || q.y > g.bbox[3] {
        return false;
    }
    if g.segs.iter().any(|s| dist_seg(q, s[0], s[1]) <= 6.0) {
        return true;
    }
    if let Some(f) = &g.fill {
        if in_poly(q, f) {
            return true;
        }
    }
    let in_box = |b: [f32; 4]| q.x >= b[0] && q.x <= b[2] && q.y >= b[1] && q.y <= b[3];
    if let Some(b) = g.fill_box {
        if in_box(b) {
            return true;
        }
    }
    if let Some(b) = g.text_box {
        if in_box([b[0], b[1], b[0] + b[2], b[1] + b[3]]) {
            return true;
        }
    }
    false
}

/// 空間索引：64px 格子，每格存畫線索引
pub struct Grid {
    cell: f32,
    ox: f32,
    oy: f32,
    cols: usize,
    rows: usize,
    buckets: Vec<Vec<u32>>,
}

impl Grid {
    pub fn build(area: Bounds<Pixels>, geoms: &[Option<Geom>]) -> Grid {
        let cell = 64.0;
        let (ox, oy) = (f32::from(area.origin.x), f32::from(area.origin.y));
        let cols = ((f32::from(area.size.width) / cell).ceil() as usize).max(1);
        let rows = ((f32::from(area.size.height) / cell).ceil() as usize).max(1);
        let mut buckets = vec![Vec::new(); cols * rows];
        for (k, g) in geoms.iter().enumerate() {
            let Some(g) = g else { continue };
            let b = g.bbox;
            if b[2] < ox || b[0] > ox + cols as f32 * cell || b[3] < oy || b[1] > oy + rows as f32 * cell {
                continue;
            }
            let c0 = (((b[0] - ox) / cell).floor().max(0.0) as usize).min(cols - 1);
            let c1 = (((b[2] - ox) / cell).floor().max(0.0) as usize).min(cols - 1);
            let r0 = (((b[1] - oy) / cell).floor().max(0.0) as usize).min(rows - 1);
            let r1 = (((b[3] - oy) / cell).floor().max(0.0) as usize).min(rows - 1);
            for r in r0..=r1 {
                for c in c0..=c1 {
                    buckets[r * cols + c].push(k as u32);
                }
            }
        }
        Grid { cell, ox, oy, cols, rows, buckets }
    }

    /// 游標所在格與周圍 3×3 格的候選（由新到舊）
    pub fn candidates(&self, q: P, out: &mut Vec<u32>) {
        out.clear();
        let c = (((q.x - self.ox) / self.cell).floor().max(0.0) as usize).min(self.cols - 1);
        let r = (((q.y - self.oy) / self.cell).floor().max(0.0) as usize).min(self.rows - 1);
        for rr in r.saturating_sub(1)..=(r + 1).min(self.rows - 1) {
            for cc in c.saturating_sub(1)..=(c + 1).min(self.cols - 1) {
                out.extend_from_slice(&self.buckets[rr * self.cols + cc]);
            }
        }
        out.sort_unstable_by(|a, b| b.cmp(a));
        out.dedup();
    }
}

/// 快取鍵：不含 view.right（純平移沿用快取）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheKey {
    pub plot_l: f32,
    pub plot_r: f32,
    pub bar_w: f64,
    pub pane_top: f32,
    pub pane_h: f32,
    pub y0: f32,
    pub ph: f32,
    pub lo: f64,
    pub hi: f64,
    pub gen: u64,
    pub selected: Option<usize>,
}

impl CacheKey {
    pub fn of(xf: &Xf, gen: u64, selected: Option<usize>) -> CacheKey {
        CacheKey {
            plot_l: xf.plot_l,
            plot_r: xf.plot_r,
            bar_w: xf.bar_w,
            pane_top: xf.pane_top,
            pane_h: xf.pane_h,
            y0: xf.y0,
            ph: xf.ph,
            lo: xf.lo,
            hi: xf.hi,
            gen,
            selected,
        }
    }
}

/// 幾何＋空間索引（不需要 window，hit-test 可以單獨重建）
pub struct GeomCache {
    pub key: CacheKey,
    pub right: f64,
    pub geoms: Vec<Option<Geom>>,
    pub grid: Grid,
    /// 有進可視範圍（含平移邊界）的畫線數
    pub visible: usize,
}

impl GeomCache {
    pub fn build(list: &[Drawing], xf: &Xf, key: CacheKey) -> GeomCache {
        let w = xf.plot_r - xf.plot_l;
        // 左右各多留半個寬度：平移快取時從邊緣移入的畫線也已經在快取裡
        let (cl, cr) = (xf.plot_l - w * 0.5, xf.plot_r + w * 0.5);
        let (ct, cb) = (xf.pane_top, xf.pane_top + xf.pane_h);
        let mut visible = 0;
        let geoms: Vec<Option<Geom>> = list
            .iter()
            .map(|d| {
                let g = geom(d, xf);
                let b = g.bbox;
                if b[2] < cl || b[0] > cr || b[3] < ct || b[1] > cb {
                    None
                } else {
                    visible += 1;
                    Some(g)
                }
            })
            .collect();
        let area = Bounds::new(gpui::point(px(cl), px(ct)), gpui::size(px(cr - cl), px(cb - ct)));
        let grid = Grid::build(area, &geoms);
        GeomCache { key, right: xf.right, geoms, grid, visible }
    }
}

/// 畫線層的已建好圖元（重建後，平移只位移）
#[derive(Default)]
pub struct PaintCache {
    /// 填色矩形（fib 帶、矩形底色、文字框）
    quads: Vec<([f32; 4], Hsla, Option<Hsla>)>,
    paths: Vec<(Path<Pixels>, Hsla)>,
    labels: Vec<(ShapedLine, f32, f32, f32)>,
    /// 水平線：橫跨整個寬度，平移時不位移（與 wry 一樣即時畫的效果）
    h_paths: Vec<(Path<Pixels>, Hsla)>,
    h_tags: Vec<(f32, ShapedLine)>,
    pub built_us: u64,
}

fn fmt_p(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "--".into() }
}

/// 一條畫線的所有圖元（快取重建與 live 層共用）
pub struct Emit {
    pub quads: Vec<([f32; 4], Hsla, Option<Hsla>)>,
    pub main: Tris,
    pub main_sel: Tris,
    pub dash: Tris,
    pub fib: [Tris; 7],
    pub fill_ch: Tris,
    pub fill_rect: Tris,
    pub labels: Vec<(String, f32, f32, f32, Hsla)>,
    pub handles: Vec<P>,
}

impl Emit {
    pub fn new() -> Emit {
        Emit {
            quads: Vec::new(),
            main: Tris::default(),
            main_sel: Tris::default(),
            dash: Tris::default(),
            fib: Default::default(),
            fill_ch: Tris::default(),
            fill_rect: Tris::default(),
            labels: Vec::new(),
            handles: Vec::new(),
        }
    }

    pub fn drawing(&mut self, d: &Drawing, g: &Geom, sel: bool, with_handles: bool) {
        let w: f32 = if sel { 2.0 } else { 1.4 };
        if let Some(f) = &g.fill {
            if d.kind == Tool::Rect {
                self.fill_rect.poly(f);
            } else {
                self.fill_ch.poly(f);
            }
        }
        match d.kind {
            Tool::Fib => {
                for (n, lv) in g.levels.iter().enumerate() {
                    let c = FIB_COLORS[lv.k];
                    if n > 0 {
                        let prev = &g.levels[n - 1];
                        let (ya, yb) = (lv.y.min(prev.y), lv.y.max(prev.y));
                        self.quads.push(([lv.x1, ya, lv.x2 - lv.x1, yb - ya], gfx::hexa(c, 0x14 as f32 / 255.0), None));
                    }
                    let t = if sel { &mut self.main_sel } else { &mut self.fib[lv.k] };
                    t.seg(p(lv.x1, lv.y), p(lv.x2, lv.y), w.min(1.4));
                    self.labels.push((format!("{} ({})", FIB[lv.k], fmt_p(lv.p)), lv.x1 + 3.0, lv.y - 7.0, 11.0, gfx::hex(c)));
                }
                if let Some([a, b]) = g.diag {
                    self.dash.dashed(a, b, 1.0, 4.0, 4.0);
                }
            }
            Tool::Text => {
                let [x, y, bw, bh] = g.text_box.unwrap();
                let border = gfx::hex(if sel { SEL_COLOR } else { DRAW_COLOR });
                self.quads.push(([x, y, bw, bh], gfx::hexa(0x1e1e1e, 0.85), Some(border)));
                self.labels.push((d.text.clone(), x + 6.0, y + bh / 2.0 + 1.0, 13.0, gfx::hex(DRAW_COLOR)));
            }
            _ => {
                let t = if sel { &mut self.main_sel } else { &mut self.main };
                for s in &g.segs {
                    t.seg(s[0], s[1], w);
                }
                if let Some([a, b]) = g.mid {
                    self.dash.dashed(a, b, 1.0, 4.0, 4.0);
                }
            }
        }
        if with_handles {
            self.handles.extend_from_slice(&g.handles);
        }
    }

    /// 依序：quad → path → 字（減少 GPU 批次切換）
    pub fn paint(mut self, window: &mut Window, cx: &mut App, sel_color: u32) {
        for (b, c, border) in self.quads.drain(..) {
            match border {
                Some(bc) => gfx::boxed(window, b[0], b[1], b[2], b[3], c, bc, 1.0),
                None => gfx::rect(window, b[0], b[1], b[2], b[3], c),
            }
        }
        for (t, c) in self.paths() {
            window.paint_path(t, c);
        }
        let _ = sel_color;
        for (s, x, y, size, c) in self.labels.drain(..) {
            gfx::text(window, cx, s, x, y, size, c);
        }
        for h in self.handles.drain(..) {
            let c = gfx::hex(SEL_COLOR);
            gfx::boxed(window, h.x - 4.5, h.y - 4.5, 9.0, 9.0, c, gfx::hex(0x000000), 1.0);
        }
    }

    pub fn paths(&mut self) -> Vec<(Path<Pixels>, Hsla)> {
        let mut out = Vec::new();
        let mut push = |t: &mut Tris, c: Hsla| {
            if let Some(path) = t.take() {
                out.push((path, c));
            }
        };
        push(&mut self.fill_rect, gfx::hexa(DRAW_COLOR, 0.10));
        push(&mut self.fill_ch, gfx::hexa(DRAW_COLOR, 0.07));
        for k in 0..7 {
            push(&mut self.fib[k], gfx::hex(FIB_COLORS[k]));
        }
        push(&mut self.dash, gfx::hex(DRAW_COLOR));
        push(&mut self.main, gfx::hex(DRAW_COLOR));
        push(&mut self.main_sel, gfx::hex(SEL_COLOR));
        out
    }
}

impl PaintCache {
    /// 由幾何快取建出圖元：只處理可見畫線；選取的那條與水平線分開
    pub fn build(window: &mut Window, list: &[Drawing], gc: &GeomCache, xf: &Xf) -> PaintCache {
        let t0 = std::time::Instant::now();
        let mut em = Emit::new();
        let mut hl = Tris::default();
        let mut h_tags = Vec::new();
        for (k, d) in list.iter().enumerate() {
            if Some(k) == gc.key.selected {
                continue;
            }
            let Some(g) = &gc.geoms[k] else { continue };
            if d.kind == Tool::Hline {
                hl.seg(g.segs[0][0], g.segs[0][1], 1.4);
                let y = g.segs[0][0].y;
                if y > xf.pane_top + 8.0 && y < xf.pane_top + xf.pane_h - 8.0 {
                    h_tags.push((y, gfx::shape(window, fmt_p(d.pts[0].p), 11.0, gfx::hex(0xffffff))));
                }
                continue;
            }
            em.drawing(d, g, false, false);
        }
        let quads = std::mem::take(&mut em.quads);
        let labels = std::mem::take(&mut em.labels)
            .into_iter()
            .map(|(s, x, y, size, c)| (gfx::shape(window, s, size, c), x, y, size))
            .collect();
        let paths = em.paths();
        let h_paths = hl.take().map(|p| vec![(p, gfx::hex(DRAW_COLOR))]).unwrap_or_default();
        PaintCache { quads, paths, labels, h_paths, h_tags, built_us: t0.elapsed().as_micros() as u64 }
    }

    /// 貼上快取（dx = 平移量）。clip 到主圖窗格。
    pub fn paint(&self, window: &mut Window, cx: &mut App, xf: &Xf, dx: f32) {
        let mask = ContentMask { bounds: xf.clip() };
        let lb = xf.clip();
        window.with_content_mask(Some(mask), |window| {
            crate::chart::maybe_layer(window, lb, |window| {
                for (b, c, border) in &self.quads {
                    match border {
                        Some(bc) => gfx::boxed(window, b[0] + dx, b[1], b[2], b[3], *c, *bc, 1.0),
                        None => gfx::rect(window, b[0] + dx, b[1], b[2], b[3], *c),
                    }
                }
            });
            for (path, c) in &self.paths {
                window.paint_path(gfx::shifted(path, dx), *c);
            }
            for (path, c) in &self.h_paths {
                window.paint_path(path.clone(), *c);
            }
            crate::chart::maybe_layer(window, lb, |window| {
                for (l, x, y, size) in &self.labels {
                    gfx::paint_line(window, cx, l, x + dx, *y, *size);
                }
            });
        });
        // 水平線的價格標籤畫在右側價格軸上
        for (y, l) in &self.h_tags {
            gfx::rect(window, xf.plot_r + 1.0, y - 8.0, 66.0 - 2.0, 16.0, gfx::hex(0x8a6a00));
            gfx::paint_line(window, cx, l, xf.plot_r + 5.0, *y, 11.0);
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.paths.iter().map(|(p, _)| p.vertices.len()).sum::<usize>() + self.h_paths.iter().map(|(p, _)| p.vertices.len()).sum::<usize>()
    }
}

/// wry `__perfAddN` 同一套產生器：同樣的索引公式與工具混合比例
pub fn perf_mix(bars: &[crate::series::K], n: usize) -> Vec<Drawing> {
    let plan = [
        (Tool::Trend, n * 40 / 100),
        (Tool::Ray, n * 8 / 100),
        (Tool::Hline, n * 8 / 100),
        (Tool::Vline, n * 8 / 100),
        (Tool::Channel, n * 12 / 100),
        (Tool::Fib, n * 12 / 100),
        (Tool::Rect, n * 6 / 100),
        (Tool::Text, n * 6 / 100),
    ];
    let mut out = Vec::with_capacity(n);
    let add = |count: usize, kind: Tool, out: &mut Vec<Drawing>| {
        let nb = bars.len();
        for k in 0..count {
            let i = ((k * 37 + 11) % (nb.saturating_sub(40)).max(2)) as usize;
            let j = (nb - 1).min(i + 20 + (k % 30));
            let (a, b) = (bars[i], bars[j]);
            let p0 = a.c;
            let p1 = b.c * (1.0 + ((k % 7) as f64 - 3.0) * 0.002);
            let (fi, fj) = (i as f64, j as f64);
            let d = match kind {
                Tool::Hline | Tool::Vline => Drawing { kind, pts: vec![Pt { i: fi, p: p0 }], text: String::new() },
                Tool::Fib => Drawing { kind, pts: vec![Pt { i: fi, p: p0.max(p1) }, Pt { i: fj, p: p0.min(p1) }], text: String::new() },
                Tool::Channel => Drawing {
                    kind,
                    pts: vec![Pt { i: fi, p: p0 }, Pt { i: fj, p: p1 }, Pt { i: ((i + j) / 2) as f64, p: (p0 + p1) / 2.0 * 1.01 }],
                    text: String::new(),
                },
                Tool::Text => Drawing { kind, pts: vec![Pt { i: fi, p: p0 }], text: format!("T{k}") },
                _ => Drawing { kind, pts: vec![Pt { i: fi, p: p0 }, Pt { i: fj, p: p1 }], text: String::new() },
            };
            out.push(d);
        }
    };
    let mut placed = 0;
    for (kind, c) in plan {
        add(c, kind, &mut out);
        placed += c;
    }
    if placed < n {
        add(n - placed, Tool::Trend, &mut out);
    }
    out
}
