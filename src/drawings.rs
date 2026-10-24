//! 畫線工具（與 wry 版相同 8 種）：幾何、空間索引 hit-test、畫線層快取。
//!
//! 效能：
//! - 畫線層只在「尺度」改變（價格軸 lo/hi、每根寬、版面、畫線內容／選取）時重建；
//!   純平移（view.right 改變）沿用快取，只把頂點整批水平位移（不重新三角化、不重新排字）。
//! - 重建時先用 bounding box 剔除不在可視區（左右各多留半個寬度給平移）的畫線。
//! - hit-test 用 64px 格子的空間索引，只測游標附近 3×3 格的候選。
//! - 圖層／上下順序（z-order，見 layers.rs）：嚴格依（圖層順位, 圖層內順位）由下往上畫。
//!   幾何（填色＋線）全部用 Path，依 z 順序排，相鄰同色合併成一條 Path；字（標籤）在每一段幾何之上。
//! - 選取的那條不進快取，在「它自己的位置」即時畫：畫線層切成 下面（快取）／選取（每幀）／上面（快取）三明治，
//!   拖曳時只重畫中間那片，上下兩段只在選取改變、順序或圖層改變時重建。控制點在 live 層（最上面）。

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
    /// 圖層 id（layers.rs；舊存檔沒有 → 0，0 不寫出，與舊版／wry 相容）
    #[serde(default, skip_serializing_if = "is_zero")]
    pub layer: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl Drawing {
    pub fn new(kind: Tool, pts: Vec<Pt>, text: String) -> Drawing {
        Drawing { kind, pts, text, layer: 0 }
    }
}

pub const FIB: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
// 畫線配色：來自設計 token（color.drawing.*）
pub fn fib_color(k: usize) -> u32 { let f = &xq_tokens::t().drawing.fib; f[k % f.len()] }
pub fn draw_color() -> u32 { xq_tokens::t().drawing.stroke }
pub fn sel_color() -> u32 { xq_tokens::t().drawing.selected }

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
    /// hidden(k)：隱藏圖層的畫線不建幾何（不畫、點不到）
    pub fn build(list: &[Drawing], xf: &Xf, key: CacheKey, hidden: impl Fn(usize) -> bool) -> GeomCache {
        let w = xf.plot_r - xf.plot_l;
        // 左右各多留半個寬度：平移快取時從邊緣移入的畫線也已經在快取裡
        let (cl, cr) = (xf.plot_l - w * 0.5, xf.plot_r + w * 0.5);
        let (ct, cb) = (xf.pane_top, xf.pane_top + xf.pane_h);
        let mut visible = 0;
        let geoms: Vec<Option<Geom>> = list
            .iter()
            .enumerate()
            .map(|(k, d)| {
                if hidden(k) {
                    return None;
                }
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

/// 畫線層一段（下面或上面）的已建好圖元（重建後，平移只位移）
#[derive(Default)]
pub struct PaintCache {
    /// 依 z 順序的幾何（填色＋線），相鄰同色合併；每段＝軸向矩形（quad）＋其餘三角形（path）
    paths: Vec<Run>,
    /// 字（fib 標籤、文字註記）：畫在這一段的幾何之上
    labels: Vec<(ShapedLine, f32, f32, f32)>,
    /// 水平線的價格標籤（右側價格軸上，不位移）
    h_tags: Vec<(f32, ShapedLine)>,
    /// A3 v2：分好的組（Grouped／Seq 模式；其他模式是空的）
    groups: Vec<Group<(ShapedLine, f32, f32, f32)>>,
    pub stats: GroupStats,
    pub built_us: u64,
}

/// 字的外框（gfx::paint_line：y 是行中線，行高 size+3）
fn label_bb(x: f32, y: f32, size: f32, w: f32) -> [f32; 4] {
    let lh = size + 3.0;
    [x - 1.0, y - lh / 2.0 - 1.0, x + w + 2.0, y + lh / 2.0 + 1.0]
}

fn fmt_p(v: f64) -> String {
    if v.is_finite() { format!("{v:.2}") } else { "--".into() }
}

/// 畫好的一段：同一個顏色的軸向矩形（paint_quad）＋斜線／曲線（path）
pub struct Run {
    quads: Vec<[f32; 4]>,
    path: Option<Path<Pixels>>,
    color: Hsla,
}

/// A3（2026-10-24，實驗、預設關）：水平線、費波納契各價位線、矩形邊框／填色、文字框這些軸向的東西改用 paint_quad，
/// 只有斜線與多邊形才走 path。v1 用 XQ_DRAW_QUADS=v1 開啟；XQ_DRAW_QUADS=1 現在是 A3 v2（分組，見 [`pack`]）。
///
/// 為什麼預設關：path 數從 313 降到 61，但 GPUI 依「重疊」給每個圖元排序，quad 夾在 path 中間會把原本「整段一批」的
/// path 切成很多批，每一批都要清一次整窗 MSAA 中介貼圖再合成。這台（軟體 Vulkan）交錯實測重圖表 FPS 反而掉一半
/// （跳價段 path 版 13.5–20.7、quad 版 6.9–10.4）。走勢圖本身的格線、K 棒、量、柱早就是 quad，不受影響。
fn quads_on() -> bool {
    draw_mode() != DrawMode::Paths
}

/// 畫線層怎麼畫（XQ_DRAW_QUADS）：
/// - `0`／不設＝Paths：全部走 path（舊做法）
/// - `1`＝Grouped（A3 v2）：軸向圖形用 quad，依 z 順序把圖元分進 paint_layer 群組（見 [`pack`]）
/// - `v1`＝A3 v1：quad＋path 直接交錯（批次會被切碎，只留著對照）
/// - `seq`＝Seq：每個圖元自己一個 layer、嚴格依序畫（正確性參考，最慢）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawMode {
    Paths,
    QuadsV1,
    Grouped,
    Seq,
}

static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);

impl DrawMode {
    pub fn name(self) -> &'static str {
        match self {
            DrawMode::Paths => "paths",
            DrawMode::QuadsV1 => "v1",
            DrawMode::Grouped => "grouped",
            DrawMode::Seq => "seq",
        }
    }
    fn from_u8(v: u8) -> DrawMode {
        match v {
            1 => DrawMode::QuadsV1,
            2 => DrawMode::Grouped,
            3 => DrawMode::Seq,
            _ => DrawMode::Paths,
        }
    }
}

/// 預設（沒設 XQ_DRAW_QUADS 時）。2026-10-24 這台（軟體 Vulkan）實測 A3 v2 重圖表 FPS 20.8→10.3、截圖也不是逐點相同
/// （只差在半透明 path 重疊處的反鋸齒邊緣），所以維持 Paths；等 GTX 1650／M4 實機 A/B 再決定。
const DEFAULT_MODE: DrawMode = DrawMode::Paths;

pub fn draw_mode() -> DrawMode {
    use std::sync::atomic::Ordering::Relaxed;
    let v = MODE.load(Relaxed);
    if v != 255 {
        return DrawMode::from_u8(v);
    }
    let m = match crate::plat::var("XQ_DRAW_QUADS").ok().as_deref().map(str::trim) {
        Some("0") => DrawMode::Paths,
        Some("v1") => DrawMode::QuadsV1,
        Some("seq") => DrawMode::Seq,
        Some(x) if !x.is_empty() => DrawMode::Grouped,
        _ => DEFAULT_MODE,
    };
    set_draw_mode(m);
    m
}

/// 執行中切換（正確性測試用；呼叫端要讓畫線快取重建）
pub fn set_draw_mode(m: DrawMode) {
    MODE.store(match m { DrawMode::Paths => 0, DrawMode::QuadsV1 => 1, DrawMode::Grouped => 2, DrawMode::Seq => 3 }, std::sync::atomic::Ordering::Relaxed);
}

fn grouped_on() -> bool {
    matches!(draw_mode(), DrawMode::Grouped | DrawMode::Seq)
}

// ───────────── A3 v2：z 順序分組 ─────────────
//
// GPUI 的 paint_layer 裡所有圖元拿同一個 draw order；同一個 order 內，批次依種類排：quad → path → 字（sprite），
// 同種類依加入順序。所以把依 z 順序排好的圖元分組：每個圖元放進「不會改變畫面上下關係」的最早一組。
// 跟它重疊、比它早的圖元 j：j 的種類 ≤ 它的種類 → 可以同組（同組內 j 先畫）；j 的種類 > 它（例如 quad 要蓋在 path 上）→ 至少下一組。
// 重疊用外框＋線寬＋反鋸齒餘量（保守），用格子空間索引；只在畫線快取重建時算（畫線改變、縮放；平移不算）。

/// 圖元種類（同一組內的畫出順序）
const K_QUAD: usize = 0;
const K_PATH: usize = 1;
const K_TEXT: usize = 2;
/// 反鋸齒餘量（px）
const AA_PAD: f32 = 1.5;

pub enum EK {
    Quad([f32; 4]),
    Path(Tris),
    Text(usize),
}

pub struct Elem {
    k: EK,
    c: Hsla,
}

/// 一組（一個 paint_layer）：quad → path → 字
pub struct Group<L> {
    quads: Vec<([f32; 4], Hsla)>,
    runs: Vec<(Path<Pixels>, Hsla)>,
    labels: Vec<L>,
}

impl<L> Default for Group<L> {
    fn default() -> Self {
        Group { quads: Vec::new(), runs: Vec::new(), labels: Vec::new() }
    }
}

/// 分組統計
#[derive(Clone, Copy, Default, Debug)]
pub struct GroupStats {
    pub elems: usize,
    pub groups: usize,
    pub paths: usize,
    /// path 批次數（依 GPUI 的批次規則推算：quad 或字夾在兩組 path 之間就斷一批）
    pub path_batches: usize,
}

fn path_bb(path: &Path<Pixels>) -> [f32; 4] {
    let mut bb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
    for v in &path.vertices {
        let (x, y) = (f32::from(v.xy_position.x), f32::from(v.xy_position.y));
        bb = [bb[0].min(x), bb[1].min(y), bb[2].max(x), bb[3].max(y)];
    }
    bb
}

fn pad(b: [f32; 4], d: f32) -> [f32; 4] {
    [b[0] - d, b[1] - d, b[2] + d, b[3] + d]
}

/// 依 z 順序把圖元分組。text_bb：第 i 個字的外框。seq＝每個圖元自己一組（參考答案）。
pub fn pack<L>(elems: Vec<Elem>, mut labels: Vec<Option<L>>, text_bb: impl Fn(usize) -> [f32; 4], seq: bool) -> (Vec<Group<L>>, GroupStats) {
    let mut st = GroupStats { elems: elems.len(), ..Default::default() };
    // 1. 每個圖元的種類＋保守外框
    let mut items: Vec<(usize, [f32; 4], Elem)> = Vec::with_capacity(elems.len());
    for mut e in elems {
        let (k, bb) = match &mut e.k {
            EK::Quad(q) => (K_QUAD, pad([q[0], q[1], q[0] + q[2], q[1] + q[3]], AA_PAD)),
            EK::Path(t) => {
                if t.is_empty() {
                    continue;
                }
                let path = t.take().unwrap();
                let bb = pad(path_bb(&path), AA_PAD);
                e.k = EK::Path(Tris::from_path(path));
                (K_PATH, bb)
            }
            EK::Text(i) => (K_TEXT, pad(text_bb(*i), AA_PAD)),
        };
        if bb.iter().all(|v| v.is_finite()) {
            items.push((k, bb, e));
        }
    }
    // 2. 分組（格子空間索引：每格記各種類目前最高的組號）
    let mut gid = vec![0usize; items.len()];
    if seq {
        for (i, g) in gid.iter_mut().enumerate() {
            *g = i;
        }
    } else if !items.is_empty() {
        let mut u = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for (_, b, _) in &items {
            u = [u[0].min(b[0]), u[1].min(b[1]), u[2].max(b[2]), u[3].max(b[3])];
        }
        let span = (u[2] - u[0]).max(u[3] - u[1]).max(1.0);
        let cell = (span / 256.0).max(16.0);
        let nx = (((u[2] - u[0]) / cell) as usize + 1).min(4096);
        let ny = (((u[3] - u[1]) / cell) as usize + 1).min(4096);
        let mut grid = vec![[-1i64; 3]; nx * ny];
        let ix = |v: f32, o: f32, n: usize| (((v - o) / cell).floor().max(0.0) as usize).min(n - 1);
        for (i, (k, b, _)) in items.iter().enumerate() {
            let (x0, x1, y0, y1) = (ix(b[0], u[0], nx), ix(b[2], u[0], nx), ix(b[1], u[1], ny), ix(b[3], u[1], ny));
            let mut g = 0i64;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = &grid[y * nx + x];
                    for kk in 0..3 {
                        if c[kk] >= 0 {
                            g = g.max(if kk <= *k { c[kk] } else { c[kk] + 1 });
                        }
                    }
                }
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = &mut grid[y * nx + x][*k];
                    *c = (*c).max(g);
                }
            }
            gid[i] = g as usize;
        }
    }
    let ng = gid.iter().copied().max().map_or(0, |g| g + 1);
    let mut groups: Vec<Group<L>> = (0..ng).map(|_| Group::default()).collect();
    // 每組 path 段的外框（同色往回併用）
    let mut run_bb: Vec<Vec<[f32; 4]>> = vec![Vec::new(); ng];
    for (i, (_, bb, e)) in items.into_iter().enumerate() {
        let g = gid[i];
        let grp = &mut groups[g];
        match e.k {
            EK::Quad(q) => grp.quads.push((q, e.c)),
            EK::Text(t) => {
                if let Some(l) = labels.get_mut(t).and_then(Option::take) {
                    grp.labels.push(l);
                }
            }
            EK::Path(mut t) => {
                let Some(path) = t.take() else { continue };
                // 同組內往回找同色段：中間隔著的段都不跟它重疊才併（畫面不變）
                let rb = &mut run_bb[g];
                let mut hit = None;
                if !seq {
                    for j in (0..grp.runs.len()).rev().take(MERGE_LOOKBACK) {
                        if grp.runs[j].1 == e.c {
                            hit = Some(j);
                            break;
                        }
                        if overlaps(&rb[j], &bb) {
                            break;
                        }
                    }
                }
                match hit {
                    Some(j) => {
                        let dst = &mut grp.runs[j].0;
                        dst.bounds = dst.bounds.union(&path.bounds);
                        dst.vertices.extend(path.vertices);
                        let r = &mut rb[j];
                        *r = [r[0].min(bb[0]), r[1].min(bb[1]), r[2].max(bb[2]), r[3].max(bb[3])];
                    }
                    None => {
                        grp.runs.push((path, e.c));
                        rb.push(bb);
                    }
                }
            }
        }
    }
    // path 外框外擴 1px：GPUI 合成中介貼圖時只貼外框範圍（＝頂點外框），反鋸齒在外框外的那半個像素會被切掉；
    // 外擴後線邊緣的反鋸齒完整（中介貼圖外框外是透明的，多貼不會改變畫面）
    for g in &mut groups {
        for (path, _) in &mut g.runs {
            path.bounds.origin.x -= px(1.0);
            path.bounds.origin.y -= px(1.0);
            path.bounds.size.width += px(2.0);
            path.bounds.size.height += px(2.0);
        }
    }
    groups.retain(|g| !(g.quads.is_empty() && g.runs.is_empty() && g.labels.is_empty()));
    st.groups = groups.len();
    st.paths = groups.iter().map(|g| g.runs.len()).sum();
    // path 批次：依序 [quad][path][字]，連續的 path 算一批
    let mut in_p = false;
    for g in &groups {
        if !g.quads.is_empty() {
            in_p = false;
        }
        if !g.runs.is_empty() {
            if !in_p {
                st.path_batches += 1;
            }
            in_p = true;
        }
        if !g.labels.is_empty() {
            in_p = false;
        }
    }
    (groups, st)
}

impl<L> Group<L> {
    /// 一組：layer（quad → path）→ 透明的 1px 隔板 path → 字（另一個 layer）。
    ///
    /// 隔板的原因（GPUI 0.3.7 wgpu／Metal 的 draw_paths_from_intermediate）：一批 path 若全部同一個 order（＝都在同一個
    /// layer 裡），合成時「每條 path 用自己的外框各貼一次」中介貼圖；外框重疊的地方會被貼兩次，半透明就變深。
    /// 只要同一批裡有不同 order 的 path，就改成貼一張聯集外框（只貼一次）。所以在這組的 path 後面補一條 order 較高、
    /// 完全透明的小 path，讓這一批一定跨兩個 order。字放到隔板之後的 layer，才不會把這一批切斷。
    fn paint(&self, window: &mut Window, lb: Bounds<Pixels>, dx: f32, mut text: impl FnMut(&mut Window, &L)) {
        window.paint_layer(lb, |window| {
            for (q, c) in &self.quads {
                gfx::rect(window, q[0] + dx, q[1], q[2], q[3], *c);
            }
            for (path, c) in &self.runs {
                if dx == 0.0 {
                    window.paint_path(path.clone(), *c);
                } else {
                    window.paint_path(gfx::shifted(path, dx), *c);
                }
            }
        });
        if !self.runs.is_empty() {
            let o = lb.origin;
            let mut sep = Path::new(o);
            let s = gpui::point(0.0f32, 1.0f32);
            sep.push_triangle((o, gpui::point(o.x + px(1.0), o.y), gpui::point(o.x, o.y + px(1.0))), (s, s, s));
            window.paint_path(sep, gpui::transparent_black());
        }
        if !self.labels.is_empty() {
            window.paint_layer(lb, |window| {
                for l in &self.labels {
                    text(window, l);
                }
            });
        }
    }
}

/// 舊做法（Paths／v1）的 path 批次數：模擬 GPUI 依重疊給 order（BoundsTree：重疊者最大 order＋1），再依（order, 種類）排
fn legacy_batches(runs: &[Run]) -> usize {
    let mut prims: Vec<(usize, [f32; 4])> = Vec::new();
    for r in runs {
        for q in &r.quads {
            prims.push((K_QUAD, [q[0], q[1], q[0] + q[2], q[1] + q[3]]));
        }
        if let Some(p) = &r.path {
            prims.push((K_PATH, path_bb(p)));
        }
    }
    let mut ord: Vec<(u32, usize)> = Vec::with_capacity(prims.len());
    for (i, (k, b)) in prims.iter().enumerate() {
        let o = (0..i).filter(|&j| overlaps(&prims[j].1, b)).map(|j| ord[j].0).max().map_or(1, |m| m + 1);
        ord.push((o, *k));
    }
    ord.sort();
    let mut n = 0;
    let mut in_p = false;
    for (_, k) in ord {
        if k == K_PATH {
            if !in_p {
                n += 1;
            }
            in_p = true;
        } else {
            in_p = false;
        }
    }
    n
}

/// 依 z 順序收集圖元：每條畫線的填色、線依序加入，顏色和上一段相同就併進同一條 Path
/// （GPUI 一批 path 內的頂點依序光柵化＋混色，所以順序就是畫面上的上下關係）。
#[derive(Default)]
pub struct ZEmit {
    /// (三角形, 顏色, 這段的外框, 軸向矩形)
    runs: Vec<(Tris, Hsla, [f32; 4], Vec<[f32; 4]>)>,
    pub labels: Vec<(String, f32, f32, f32, Hsla)>,
    /// 目前這條畫線的外框（t() 合併用）
    cur_bb: [f32; 4],
    /// A3 v2（Grouped／Seq）：依 z 順序的圖元，之後由 pack 分組
    elems: Vec<Elem>,
}

/// 往回找同色的段最多看幾段
const MERGE_LOOKBACK: usize = 64;

fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

impl ZEmit {
    /// 拿顏色 c 的三角形段。往回找同色的段：中間隔著的段只要跟這條畫線的外框都不重疊，
    /// 把這條的幾何併到那段，畫出來一模一樣（不重疊的圖元先畫後畫結果相同）——
    /// path 數從「每條畫線好幾段」降到接近「每種顏色幾段」，GPUI 每個 path 的固定成本跟著少。
    #[inline]
    fn t(&mut self, c: Hsla) -> &mut Tris {
        if grouped_on() {
            self.elems.push(Elem { k: EK::Path(Tris::default()), c });
            let Some(Elem { k: EK::Path(t), .. }) = self.elems.last_mut() else { unreachable!() };
            return t;
        }
        let i = self.run_ix(c);
        &mut self.runs[i].0
    }

    fn label(&mut self, s: String, x: f32, y: f32, size: f32, c: Hsla) {
        if grouped_on() {
            self.elems.push(Elem { k: EK::Text(self.labels.len()), c });
        }
        self.labels.push((s, x, y, size, c));
    }

    /// 軸向矩形（x, y, w, h）：和三角形同一套合併規則（同色段、外框不重疊才往回併）
    #[inline]
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Hsla) {
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        if !quads_on() {
            self.t(c).poly(&[p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h)]);
            return;
        }
        if grouped_on() {
            self.elems.push(Elem { k: EK::Quad([x, y, w, h]), c });
            return;
        }
        let i = self.run_ix(c);
        self.runs[i].3.push([x, y, w, h]);
    }

    /// 線段：水平／垂直就用矩形（和 Tris::seg 同樣的範圍：線寬 w、兩端各外推 w/4），其他走三角形
    #[inline]
    fn seg(&mut self, a: P, b: P, w: f32, c: Hsla) {
        if quads_on() && (a.y == b.y || a.x == b.x) && (a.x != b.x || a.y != b.y) {
            let e = w * 0.25;
            if a.y == b.y {
                let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
                self.rect(x0 - e, a.y - w * 0.5, x1 - x0 + 2.0 * e, w, c);
            } else {
                let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
                self.rect(a.x - w * 0.5, y0 - e, w, y1 - y0 + 2.0 * e, c);
            }
            return;
        }
        self.t(c).seg(a, b, w);
    }

    /// 填色多邊形：四點軸向矩形就用 quad
    fn fill(&mut self, f: &[P], c: Hsla) {
        if quads_on() && f.len() == 4 {
            let (xs, ys) = ([f[0].x, f[1].x, f[2].x, f[3].x], [f[0].y, f[1].y, f[2].y, f[3].y]);
            let axis = (0..4).all(|k| {
                let (q, r) = (f[k], f[(k + 1) % 4]);
                q.x == r.x || q.y == r.y
            });
            if axis {
                let x0 = xs.iter().cloned().fold(f32::INFINITY, f32::min);
                let x1 = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let y0 = ys.iter().cloned().fold(f32::INFINITY, f32::min);
                let y1 = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                self.rect(x0, y0, x1 - x0, y1 - y0, c);
                return;
            }
        }
        self.t(c).poly(f);
    }

    #[inline]
    fn run_ix(&mut self, c: Hsla) -> usize {
        let bb = self.cur_bb;
        let mut hit = None;
        for i in (0..self.runs.len()).rev().take(MERGE_LOOKBACK) {
            let (_, rc, rb, _) = &self.runs[i];
            if *rc == c {
                hit = Some(i);
                break;
            }
            if overlaps(rb, &bb) {
                break;
            }
        }
        let i = match hit {
            Some(i) => i,
            None => {
                self.runs.push((Tris::default(), c, bb, Vec::new()));
                self.runs.len() - 1
            }
        };
        let r = &mut self.runs[i];
        r.2 = [r.2[0].min(bb[0]), r.2[1].min(bb[1]), r.2[2].max(bb[2]), r.2[3].max(bb[3])];
        i
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, c: Hsla) {
        self.rect(x, y, w, h, c);
    }

    /// 一條畫線。hline_ext：水平線往左右各多畫一個寬度（快取平移時不會斷）
    pub fn drawing(&mut self, d: &Drawing, g: &Geom, sel: bool, hline_ext: f32) {
        let w: f32 = if sel { 2.0 } else { 1.4 };
        let main = gfx::hex(if sel { sel_color() } else { draw_color() });
        // 外框（含線寬餘量；水平線含左右延伸）
        let ext = if d.kind == Tool::Hline { hline_ext } else { 0.0 };
        self.cur_bb = [g.bbox[0] - ext - 4.0, g.bbox[1] - 4.0, g.bbox[2] + ext + 4.0, g.bbox[3] + 4.0];
        if let Some(f) = &g.fill {
            let c = gfx::hexa(draw_color(), if d.kind == Tool::Rect { 0.10 } else { 0.07 });
            self.fill(f, c);
        }
        match d.kind {
            Tool::Fib => {
                for n in 1..g.levels.len() {
                    let (lv, prev) = (&g.levels[n], &g.levels[n - 1]);
                    let (ya, yb) = (lv.y.min(prev.y), lv.y.max(prev.y));
                    self.quad(lv.x1, ya, lv.x2 - lv.x1, yb - ya, gfx::hexa(fib_color(lv.k), 0x14 as f32 / 255.0));
                }
                for lv in &g.levels {
                    let c = if sel { main } else { gfx::hex(fib_color(lv.k)) };
                    self.seg(p(lv.x1, lv.y), p(lv.x2, lv.y), w.min(1.4), c);
                    self.label(format!("{} ({})", FIB[lv.k], fmt_p(lv.p)), lv.x1 + 3.0, lv.y - 7.0, xq_tokens::t().font.sm, gfx::hex(fib_color(lv.k)));
                }
                if let Some([a, b]) = g.diag {
                    self.t(gfx::hex(draw_color())).dashed(a, b, 1.0, 4.0, 4.0);
                }
            }
            Tool::Text => {
                if let Some([x, y, bw, bh]) = g.text_box {
                    self.quad(x, y, bw, bh, gfx::hexa(xq_tokens::t().drawing.text_bg.rgb, xq_tokens::t().drawing.text_bg.a));
                    let bwid = if sel { 2.0 } else { 1.0 };
                    self.rect(x, y, bw, bwid, main);
                    self.rect(x, y + bh - bwid, bw, bwid, main);
                    self.rect(x, y, bwid, bh, main);
                    self.rect(x + bw - bwid, y, bwid, bh, main);
                    self.label(d.text.clone(), x + 6.0, y + bh / 2.0 + 1.0, xq_tokens::t().font.lg, gfx::hex(draw_color()));
                }
            }
            Tool::Hline if hline_ext > 0.0 => {
                let [a, b] = g.segs[0];
                self.seg(p(a.x - hline_ext, a.y), p(b.x + hline_ext, b.y), w, main);
            }
            _ => {
                for s in &g.segs {
                    self.seg(s[0], s[1], w, main);
                }
                if let Some([a, b]) = g.mid {
                    self.t(gfx::hex(draw_color())).dashed(a, b, 1.0, 4.0, 4.0);
                }
            }
        }
    }

    pub fn paths(&mut self) -> Vec<Run> {
        self.runs
            .drain(..)
            .filter_map(|(mut t, color, _, quads)| {
                let path = t.take();
                (path.is_some() || !quads.is_empty()).then_some(Run { quads, path, color })
            })
            .collect()
    }

    /// 即時畫（選取段／繪製中）：幾何 → 字（字包一個整窗格的 layer，順序一定在這段幾何之上）
    pub fn paint(mut self, window: &mut Window, cx: &mut App, lb: Bounds<Pixels>) {
        if grouped_on() {
            let labels: Vec<_> = std::mem::take(&mut self.labels);
            let bbs: Vec<[f32; 4]> = labels.iter().map(|(s, x, y, size, _)| label_bb(*x, *y, *size, text_width(s, *size))).collect();
            let (groups, _) = pack(std::mem::take(&mut self.elems), labels.into_iter().map(Some).collect(), |i| bbs[i], draw_mode() == DrawMode::Seq);
            for g in &groups {
                g.paint(window, lb, 0.0, |window, (s, x, y, size, c)| {
                    gfx::text(window, cx, s.clone(), *x, *y, *size, *c);
                });
            }
            return;
        }
        for r in self.paths() {
            r.paint(window, 0.0);
        }
        if !self.labels.is_empty() {
            window.paint_layer(lb, |window| {
                for (s, x, y, size, c) in self.labels.drain(..) {
                    gfx::text(window, cx, s, x, y, size, c);
                }
            });
        }
    }
}

/// 選取畫線的控制點（live 層，永遠在最上面）
pub fn paint_handles(window: &mut Window, hs: &[P]) {
    for h in hs {
        gfx::boxed(window, h.x - 4.5, h.y - 4.5, 9.0, 9.0, gfx::hex(sel_color()), gfx::hex(xq_tokens::t().drawing.handle_border), 1.0);
    }
}

impl PaintCache {
    /// 由幾何快取建出一段（idxs = 由下到上的畫線索引）的圖元：只處理可見畫線
    pub fn build(window: &mut Window, list: &[Drawing], gc: &GeomCache, xf: &Xf, idxs: &[u32]) -> PaintCache {
        let t0 = crate::plat::Instant::now();
        let mut em = ZEmit::default();
        let mut h_tags = Vec::new();
        let ext = xf.plot_r - xf.plot_l;
        for &k in idxs {
            let k = k as usize;
            let (Some(d), Some(Some(g))) = (list.get(k), gc.geoms.get(k)) else { continue };
            if d.kind == Tool::Hline {
                let y = g.segs[0][0].y;
                if y > xf.pane_top + 8.0 && y < xf.pane_top + xf.pane_h - 8.0 {
                    h_tags.push((y, gfx::shape(window, fmt_p(d.pts[0].p), xq_tokens::t().font.sm, gfx::hex(xq_tokens::t().chart.tag_text))));
                }
            }
            em.drawing(d, g, false, ext);
        }
        let shaped: Vec<(ShapedLine, f32, f32, f32)> = std::mem::take(&mut em.labels)
            .into_iter()
            .map(|(s, x, y, size, c)| (gfx::shape(window, s, size, c), x, y, size))
            .collect();
        if grouped_on() {
            let bbs: Vec<[f32; 4]> = shaped.iter().map(|(l, x, y, size)| label_bb(*x, *y, *size, f32::from(l.width()))).collect();
            let (groups, stats) = pack(std::mem::take(&mut em.elems), shaped.into_iter().map(Some).collect(), |i| bbs[i], draw_mode() == DrawMode::Seq);
            return PaintCache { paths: Vec::new(), labels: Vec::new(), h_tags, groups, stats, built_us: t0.elapsed().as_micros() as u64 };
        }
        let paths = em.paths();
        let stats = GroupStats {
            elems: 0,
            groups: 0,
            paths: paths.iter().filter(|r| r.path.is_some()).count(),
            path_batches: legacy_batches(&paths),
        };
        PaintCache { paths, labels: shaped, h_tags, groups: Vec::new(), stats, built_us: t0.elapsed().as_micros() as u64 }
    }

    /// 貼上快取（dx = 平移量）。clip 到主圖窗格。b = 整個圖面（order 隔板用）。
    ///
    /// GPUI 的 path 是「一批一次」：每一批都要把整窗大小的 MSAA 中介貼圖清掉、光柵化、再合成，軟體 Vulkan 上很貴。
    /// 一批 path 只要中間插進別種圖元（字、quad）的 order 就會被切斷，所以：
    /// 1. 價格軸標籤整組包一個 layer（同一個 order，不會一個標籤佔兩個 order 夾在 path 中間）；
    /// 2. path 前先放一個空的整圖 layer 當隔板 → 之後每條 path 的 order 都比圖上已有的東西高，不會跟主圖／標籤交錯；
    /// 3. Path 本身不包 layer：依重疊關係拿遞增 order（z 順序正確），整段一批、合成時一張聯集貼圖；
    /// 4. 字包一個整窗格的 layer → order 在整段幾何之上。
    pub fn paint(&self, window: &mut Window, cx: &mut App, xf: &Xf, dx: f32, b: Bounds<Pixels>) {
        if !self.h_tags.is_empty() {
            let tb = Bounds::new(gpui::point(px(xf.plot_r), px(xf.pane_top)), gpui::size(px(66.0), px(xf.pane_h)));
            window.paint_layer(tb, |window| {
                for (y, l) in &self.h_tags {
                    gfx::rect(window, xf.plot_r + 1.0, y - 8.0, 66.0 - 2.0, 16.0, gfx::hex(xq_tokens::t().chart.label_bg_hline));
                    gfx::paint_line(window, cx, l, xf.plot_r + 5.0, *y, 11.0);
                }
            });
        }
        let lb = xf.clip();
        if !self.groups.is_empty() {
            window.paint_layer(b, |_| {});
            window.with_content_mask(Some(ContentMask { bounds: lb }), |window| {
                for g in &self.groups {
                    g.paint(window, lb, dx, |window, (l, x, y, size)| gfx::paint_line(window, cx, l, x + dx, *y, *size));
                }
            });
            return;
        }
        if !self.paths.is_empty() {
            window.paint_layer(b, |_| {});
        }
        window.with_content_mask(Some(ContentMask { bounds: lb }), |window| {
            for r in &self.paths {
                r.paint(window, dx);
            }
            if !self.labels.is_empty() {
                window.paint_layer(lb, |window| {
                    for (l, x, y, size) in &self.labels {
                        gfx::paint_line(window, cx, l, x + dx, *y, *size);
                    }
                });
            }
        });
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.labels.is_empty() && self.h_tags.is_empty() && self.groups.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.paths.iter().map(|r| r.path.as_ref().map_or(0, |p| p.vertices.len()) + r.quads.len() * 4).sum::<usize>()
            + self.groups.iter().map(|g| g.runs.iter().map(|(p, _)| p.vertices.len()).sum::<usize>() + g.quads.len() * 4).sum::<usize>()
    }
    pub fn path_count(&self) -> usize {
        self.stats.paths
    }
}

impl Run {
    /// 先 quad 再 path（同色、同一段，彼此順序不影響畫面；跟其他段的上下關係由 GPUI 依重疊排序）
    fn paint(&self, window: &mut Window, dx: f32) {
        for q in &self.quads {
            gfx::rect(window, q[0] + dx, q[1], q[2], q[3], self.color);
        }
        if let Some(path) = &self.path {
            if dx == 0.0 {
                window.paint_path(path.clone(), self.color);
            } else {
                window.paint_path(gfx::shifted(path, dx), self.color);
            }
        }
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
                Tool::Hline | Tool::Vline => Drawing::new(kind, vec![Pt { i: fi, p: p0 }], String::new()),
                Tool::Fib => Drawing::new(kind, vec![Pt { i: fi, p: p0.max(p1) }, Pt { i: fj, p: p0.min(p1) }], String::new()),
                Tool::Channel => Drawing::new(
                    kind,
                    vec![Pt { i: fi, p: p0 }, Pt { i: fj, p: p1 }, Pt { i: ((i + j) / 2) as f64, p: (p0 + p1) / 2.0 * 1.01 }],
                    String::new(),
                ),
                Tool::Text => Drawing::new(kind, vec![Pt { i: fi, p: p0 }], format!("T{k}")),
                _ => Drawing::new(kind, vec![Pt { i: fi, p: p0 }, Pt { i: fj, p: p1 }], String::new()),
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

/// 壓測用：拿哪一條當「拖曳／選取」的目標——perf_mix 裡第 31 個矩形（半透明填色，落在預設可視範圍內）。
/// wry `__perfPick` 同一個公式。
pub fn perf_pick(n: usize) -> Option<usize> {
    let rect_start = n * 40 / 100 + n * 8 / 100 * 3 + n * 12 / 100 * 2;
    let rects = n * 6 / 100;
    (rects > 0).then(|| rect_start + 30.min(rects - 1))
}
