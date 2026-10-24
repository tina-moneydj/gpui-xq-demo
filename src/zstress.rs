//! 圖層／上下順序壓測（XQ_CHART_STRESS 的後四段，接在 ticks／crosshair／pan 之後）。
//! gpui／egui 兩版同一份檔案，流程與 wry `ui/index.html` 的 zPhases 相同。
//!
//! - drag：1000 條畫線分在 XQ_CHART_LAYERS（預設 100）層，抓住疊放順序正中間的半透明矩形連續拖 10 秒（同時跳價）。
//!   預期上下兩段快取 0 次重建，每幀只重畫「選取那片」＋live。
//! - select：選取／取消選取不同的畫線，每次都要重建上下兩段 → 量每次重建 ms 與「操作到下一幀」ms 的 p50／p99。
//! - reorder：選取中間那個矩形，輪流 上移一層／下移一層／移到最上／移到最下／隱藏某層／顯示某層／圖層上移／圖層下移。
//! - zshot：正確性截圖用的小場景：底層矩形＋斜線、中層半透明矩形（選取並拖曳中）、上層文字框＋費波納契＋斜線。
//!   拖曳中的矩形必須被上層蓋住（白色選取框穿過文字框時要被文字框遮住）。

use crate::plat::Instant;

use crate::drawings::{self, Drawing, Pt, Tool};
use crate::gfx::{p, P};
use crate::layers::{Layer, ZOp};
use crate::model::Model;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZMode {
    Drag,
    Select,
    Reorder,
    ZShot,
}

impl ZMode {
    pub fn name(self) -> &'static str {
        match self {
            ZMode::Drag => "drag",
            ZMode::Select => "select",
            ZMode::Reorder => "reorder",
            ZMode::ZShot => "zshot",
        }
    }
}

const K_SELECT: u8 = 0;
const K_DESELECT: u8 = 1;
const K_ZOP: u8 = 2;
const K_LAYER_VIS: u8 = 3;
const K_LAYER_MOVE: u8 = 4;

pub fn env_layers() -> usize {
    crate::plat::var("XQ_CHART_LAYERS").ok().and_then(|v| v.parse().ok()).unwrap_or(100).clamp(1, crate::layers::MAX_LAYERS)
}

pub struct ZState {
    pub nlayers: usize,
    /// 拖曳／reorder 的目標（疊放順序正中間的半透明矩形）
    pub pick: Option<usize>,
    q0: P,
    cand: Vec<usize>,
    /// (種類, 這次操作造成的重建 ms, 操作到下一幀開始 ms)
    samples: Vec<(u8, f64, f64)>,
    pend: Option<(Instant, u64, u8)>,
    last_q: P,
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

fn center(m: &mut Model, k: usize) -> Option<P> {
    let xf = m.geo_now()?.xf();
    let d = m.drawings.get(k)?;
    let g = drawings::geom(d, &xf);
    let b = g.bbox;
    Some(p((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0))
}

impl ZState {
    pub fn new(nlayers: usize) -> ZState {
        ZState { nlayers, pick: None, q0: p(0.0, 0.0), cand: Vec::new(), samples: Vec::new(), pend: None, last_q: p(0.0, 0.0) }
    }

    /// 每一幀開頭：上一個操作到這一幀為止的重建時間與延遲
    pub fn record(&mut self, rebuild_us: u64) {
        if let Some((t, rb0, kind)) = self.pend.take() {
            self.samples.push((kind, rebuild_us.saturating_sub(rb0) as f64 / 1000.0, t.elapsed().as_secs_f64() * 1000.0));
        }
    }

    pub fn begin(&mut self, m: &mut Model, mode: ZMode) -> u8 {
        self.samples.clear();
        self.pend = None;
        match mode {
            ZMode::Drag => {
                let Some(k) = self.pick else { return 0 };
                let Some(q) = center(m, k) else { return 0 };
                self.q0 = q;
                self.last_q = q;
                m.begin_drag(k, q)
            }
            ZMode::Select => {
                let mut f = m.select(None);
                m.ensure_gc();
                let ord = m.ensure_order().clone();
                let vis: Vec<usize> = match &m.gc {
                    Some(gc) => ord.order.iter().map(|&k| k as usize).filter(|&k| gc.geoms.get(k).is_some_and(|g| g.is_some()) && ord.pickable(k)).collect(),
                    None => Vec::new(),
                };
                self.cand = vis;
                f |= crate::model::DRAW;
                f
            }
            ZMode::Reorder => m.select(self.pick),
            ZMode::ZShot => {
                // 截圖要看得清楚：關掉疊加與副圖，只留 K 棒＋畫線
                let mut c = crate::model::IndCfg::default();
                c.overlays.clear();
                c.subs.iter_mut().for_each(|s| s.on = false);
                c.volma.0 = false;
                m.set_cfg(c);
                if let Some(b) = m.bounds {
                    m.geo(b);
                }
                let Some(xf) = m.geo_now().map(|g| g.xf()) else { return 0 };
                let (i0, i1) = (xf.i_at(xf.plot_l), xf.i_at(xf.plot_r));
                let at = |fx: f64, fy: f64| Pt { i: i0 + fx * (i1 - i0), p: xf.lo + fy * (xf.hi - xf.lo) };
                let mk = |kind: Tool, pts: Vec<Pt>, text: &str, layer: u32| Drawing { kind, pts, text: text.into(), layer };
                let list = vec![
                    mk(Tool::Rect, vec![at(0.08, 0.80), at(0.42, 0.30)], "", 0),
                    mk(Tool::Trend, vec![at(0.04, 0.22), at(0.96, 0.78)], "", 0),
                    mk(Tool::Rect, vec![at(0.28, 0.68), at(0.66, 0.18)], "", 1),
                    mk(Tool::Text, vec![at(0.24, 0.68)], "上層文字框 ABOVE ABOVE ABOVE", 2),
                    mk(Tool::Text, vec![at(0.24, 0.18)], "上層文字框 ABOVE ABOVE ABOVE", 2),
                    mk(Tool::Fib, vec![at(0.56, 0.74), at(0.86, 0.26)], "", 2),
                    mk(Tool::Trend, vec![at(0.04, 0.72), at(0.96, 0.12)], "", 2),
                ];
                let mut layers = vec![Layer::new(0), Layer::new(1), Layer::new(2)];
                layers[0].name = "底層".into();
                layers[1].name = "中層（拖曳中）".into();
                layers[2].name = "上層".into();
                m.set_drawings(list, layers);
                let Some(q) = center(m, 2) else { return crate::model::ALL };
                self.q0 = q;
                self.last_q = q;
                crate::model::ALL | m.begin_drag(2, q)
            }
        }
    }

    pub fn step(&mut self, m: &mut Model, mode: ZMode, f: u64, rebuild_us: u64) -> u8 {
        match mode {
            ZMode::Drag => {
                let mut fl = m.synth_tick(f);
                let t = f as f32;
                let q = p(self.q0.x + (t / 9.0).sin() * 40.0, self.q0.y + (t / 11.0).cos() * 25.0 - 25.0);
                self.last_q = q;
                fl |= m.mouse_move(q, true);
                fl
            }
            ZMode::ZShot => {
                let q = p(self.q0.x + (f as f32 / 10.0).sin() * 12.0, self.q0.y);
                self.last_q = q;
                m.mouse_move(q, true)
            }
            ZMode::Select => {
                if self.cand.is_empty() {
                    return 0;
                }
                let (kind, fl) = if f % 2 == 0 {
                    let k = self.cand[(f as usize / 2 * 37) % self.cand.len()];
                    (K_SELECT, m.select(Some(k)))
                } else {
                    (K_DESELECT, m.select(None))
                };
                self.pend = Some((Instant::now(), rebuild_us, kind));
                fl
            }
            ZMode::Reorder => {
                let layered = self.nlayers > 1;
                let n = if layered { 8 } else { 4 };
                // 動的圖層：選取那條所在層的下面四分之一處（不會是選取那條自己的層）
                let other = (self.nlayers / 4) as u32;
                let t0 = Instant::now();
                let (kind, fl) = match f % n {
                    0 => (K_ZOP, m.zop(ZOp::Forward)),
                    1 => (K_ZOP, m.zop(ZOp::Backward)),
                    2 => (K_ZOP, m.zop(ZOp::Front)),
                    3 => (K_ZOP, m.zop(ZOp::Back)),
                    4 => (K_LAYER_VIS, m.layer_toggle_visible(other)),
                    5 => (K_LAYER_VIS, m.layer_toggle_visible(other)),
                    6 => (K_LAYER_MOVE, m.layer_move(other, true)),
                    _ => (K_LAYER_MOVE, m.layer_move(other, false)),
                };
                self.pend = Some((t0, rebuild_us, kind));
                fl
            }
        }
    }

    pub fn end(&mut self, m: &mut Model, mode: ZMode) -> u8 {
        match mode {
            ZMode::Drag => {
                let q = self.last_q;
                m.mouse_up(q) | m.select(None) | m.mouse_move(p(-1.0, -1.0), false)
            }
            ZMode::Select | ZMode::Reorder => {
                self.pend = None;
                m.select(None) | crate::model::DRAW
            }
            ZMode::ZShot => m.mouse_up(self.last_q),
        }
    }

    /// 這一段的額外 JSON 欄位（逗號開頭）
    pub fn report(&mut self, m: &Model) -> String {
        let mut s = format!(",\"layers\":{},\"drawingsTotal\":{}", m.layers.len(), m.drawings.len());
        if let Some(k) = self.pick {
            s += &format!(",\"pick\":{k}");
        }
        if self.samples.is_empty() {
            return s;
        }
        let mut stat = |name: &str, kinds: &[u8]| {
            let mut rb: Vec<f64> = self.samples.iter().filter(|x| kinds.contains(&x.0)).map(|x| x.1).collect();
            let mut tf: Vec<f64> = self.samples.iter().filter(|x| kinds.contains(&x.0)).map(|x| x.2).collect();
            if rb.is_empty() {
                return;
            }
            s += &format!(
                ",\"{name}Ops\":{},\"{name}RebuildP50\":{:.3},\"{name}RebuildP99\":{:.3},\"{name}TtfP50\":{:.3},\"{name}TtfP99\":{:.3}",
                rb.len(),
                pct(&mut rb, 0.5),
                pct(&mut rb, 0.99),
                pct(&mut tf, 0.5),
                pct(&mut tf, 0.99)
            );
        };
        stat("all", &[K_SELECT, K_DESELECT, K_ZOP, K_LAYER_VIS, K_LAYER_MOVE]);
        stat("select", &[K_SELECT]);
        stat("deselect", &[K_DESELECT]);
        stat("zop", &[K_ZOP]);
        stat("layerVis", &[K_LAYER_VIS]);
        stat("layerMove", &[K_LAYER_MOVE]);
        s
    }
}

// ───────────── A3 v2 正確性測試（XQ_ZTEST=N）─────────────
// 同一個程式、同一份（不跳價的）K 棒，輪流用 Grouped／Seq／Paths 畫同一組隨機畫線，外部腳本每個情境截一張圖比對。

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn n(&mut self, k: usize) -> usize {
        (self.next() % k.max(1) as u64) as usize
    }
}

/// 隨機場景：seed 0＝壓測用 1000 條／100 層；其他＝30–150 條隨機工具、隨機位置（集中在畫面中間，重疊多）、1–8 層、隨機順序；
/// seed % 3 == 2 時再隨機選取一條（測三明治上下段＋選取那片）。回傳 (畫線數, 圖層數, 選取)
pub fn ztest_scene(m: &mut Model, seed: u64) -> (usize, usize, Option<usize>) {
    let mut c = crate::model::IndCfg::default();
    c.overlays.clear();
    c.subs.iter_mut().for_each(|s| s.on = false);
    c.volma.0 = false;
    m.set_cfg(c);
    if let Some(b) = m.bounds {
        m.geo(b);
    }
    if seed == 0 {
        m.load_perf_drawings(1000, 100);
        return (m.drawings.len(), m.layers.len(), None);
    }
    let Some(xf) = m.geo_now().map(|g| g.xf()) else { return (0, 0, None) };
    let (i0, i1) = (xf.i_at(xf.plot_l), xf.i_at(xf.plot_r));
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let nl = 1 + r.n(8);
    let n = 30 + r.n(121);
    let tools = [Tool::Trend, Tool::Ray, Tool::Hline, Tool::Vline, Tool::Channel, Tool::Fib, Tool::Rect, Tool::Text];
    let texts = ["文字框", "ABOVE 上層", "支撐 Support", "T", "壓力區 resistance zone", "注意"];
    let mut list = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = tools[r.n(tools.len())];
        let mut at = |r: &mut Rng| {
            let (fx, fy) = (0.1 + 0.8 * r.f(), 0.1 + 0.8 * r.f());
            Pt { i: i0 + fx * (i1 - i0), p: xf.lo + fy * (xf.hi - xf.lo) }
        };
        let pts = match kind {
            Tool::Hline | Tool::Vline | Tool::Text => vec![at(&mut r)],
            Tool::Channel => vec![at(&mut r), at(&mut r), at(&mut r)],
            _ => vec![at(&mut r), at(&mut r)],
        };
        let text = if kind == Tool::Text { texts[r.n(texts.len())].to_string() } else { String::new() };
        list.push(Drawing { kind, pts, text, layer: r.n(nl) as u32 });
    }
    let layers: Vec<Layer> = (0..nl as u32).map(Layer::new).collect();
    m.set_drawings(list, layers);
    let sel = (seed % 3 == 2).then(|| r.n(n));
    m.select(sel);
    m.mouse = None;
    (n, nl, sel)
}
