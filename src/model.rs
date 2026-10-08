//! 走勢圖狀態（序列、指標、視圖、畫線、互動），不碰 GPUI 視圖——
//! 三個繪圖層（plot／draw／live）共用同一份 Model（Rc<RefCell>），各自只在需要時重畫。

use gpui::{px, Bounds, Pixels};

use crate::drawings::{self, CacheKey, Drawing, GeomCache, PaintCache, Pt, Tool, Xf};
use crate::gfx::{p, P};
use crate::indicators::{OvSpec, Overlay, Sub, SubCfg, SubKind, MAX_OVERLAYS, MAX_SUBS, SUB_ORDER};
use crate::series::{self, Period, Series, K};

pub const AXIS_W: f32 = 66.0;
pub const TIME_H: f32 = 18.0;
pub const PLOT_L: f32 = 4.0;

/// 要重畫哪幾層
pub const PLOT: u8 = 1;
pub const DRAW: u8 = 2;
pub const LIVE: u8 = 4;
pub const HEAD: u8 = 8;
pub const SAVE: u8 = 16;
pub const ALL: u8 = PLOT | DRAW | LIVE | HEAD;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub bar_w: f64,
    pub right: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneKind {
    Price,
    Sub(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Pane {
    pub kind: PaneKind,
    pub top: f32,
    pub h: f32,
    pub hdr: f32,
    pub y0: f32,
    pub ph: f32,
    pub lo: f64,
    pub hi: f64,
}

impl Pane {
    #[inline]
    pub fn y(&self, v: f64) -> f32 {
        (self.y0 as f64 + self.ph as f64 * (1.0 - (v - self.lo) / (self.hi - self.lo))) as f32
    }
    pub fn val_at(&self, y: f32) -> f64 {
        self.lo + (1.0 - (y - self.y0) as f64 / self.ph as f64) * (self.hi - self.lo)
    }
}

#[derive(Clone, Debug)]
pub struct Geo {
    pub b: Bounds<Pixels>,
    pub plot_l: f32,
    pub plot_r: f32,
    pub plot_w: f32,
    /// 時間軸頂端（y）
    pub bottom: f32,
    pub i0: usize,
    pub i1: usize,
    pub n: usize,
    pub bar_w: f64,
    pub right: f64,
    pub panes: Vec<Pane>,
    pub ticks: Vec<usize>,
}

impl Geo {
    #[inline]
    pub fn x(&self, i: f64) -> f32 {
        (self.plot_r as f64 - (self.right - i) * self.bar_w - self.bar_w / 2.0) as f32
    }
    pub fn idx_at(&self, x: f32) -> f64 {
        self.right - (self.plot_r as f64 - self.bar_w / 2.0 - x as f64) / self.bar_w
    }
    pub fn xf(&self) -> Xf {
        let pr = &self.panes[0];
        Xf {
            plot_l: self.plot_l,
            plot_r: self.plot_r,
            right: self.right,
            bar_w: self.bar_w,
            pane_top: pr.top,
            pane_h: pr.h,
            y0: pr.y0,
            ph: pr.ph,
            lo: pr.lo,
            hi: pr.hi,
        }
    }
    pub fn pane_at(&self, q: P) -> Option<usize> {
        if q.x < self.plot_l || q.x >= self.plot_r {
            return None;
        }
        self.panes.iter().position(|pn| q.y >= pn.top && q.y < pn.top + pn.h)
    }
    pub fn in_price(&self, q: P) -> bool {
        self.pane_at(q) == Some(0)
    }
    pub fn hover_idx(&self, q: Option<P>) -> Option<usize> {
        let q = q?;
        self.pane_at(q)?;
        if self.n == 0 {
            return None;
        }
        Some(self.idx_at(q.x.clamp(self.plot_l, self.plot_r)).round().clamp(0.0, (self.n - 1) as f64) as usize)
    }
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub d: Drawing,
    pub dragging: bool,
    pub sx: f32,
    pub sy: f32,
}

#[derive(Clone, Debug)]
pub struct DragState {
    pub idx: usize,
    pub handle: Option<usize>,
    pub start: Pt,
    pub orig: Vec<Pt>,
    pub moved: bool,
}

#[derive(Clone, Debug)]
pub struct TextEdit {
    pub anchor: Pt,
    pub edit_idx: Option<usize>,
    pub text: String,
    pub at: P,
}

/// 指標設定（存檔、選單用）
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndCfg {
    pub overlays: Vec<OvSpec>,
    pub subs: Vec<SubCfg>,
    pub volma: (bool, [usize; 2]),
}

impl Default for IndCfg {
    fn default() -> Self {
        IndCfg {
            overlays: vec![OvSpec::Ma { period: 5 }, OvSpec::Ma { period: 20 }, OvSpec::Ma { period: 60 }],
            subs: SUB_ORDER.iter().map(|&k| SubCfg::default_for(k)).collect(),
            volma: (true, [5, 20]),
        }
    }
}

impl IndCfg {
    /// 與 wry `__perfSetupStress` 完全相同的 20 疊加＋全部副圖
    pub fn stress() -> IndCfg {
        let mut ovs = Vec::new();
        for p in [5, 10, 15, 20, 25, 30, 40, 50, 60, 120] {
            ovs.push(OvSpec::Ma { period: p });
        }
        for p in [8, 12, 21, 26, 55] {
            ovs.push(OvSpec::Ema { period: p });
        }
        ovs.push(OvSpec::Bb { n: 20, k: 2.0 });
        ovs.push(OvSpec::Bb { n: 50, k: 2.5 });
        ovs.push(OvSpec::Sar { step: 0.02, max: 0.2 });
        ovs.push(OvSpec::Sar { step: 0.04, max: 0.3 });
        let mut p = 7;
        while ovs.len() < MAX_OVERLAYS {
            ovs.push(OvSpec::Ma { period: p });
            p += 3;
        }
        let subs = SUB_ORDER.iter().map(|&k| SubCfg { on: true, ..SubCfg::default_for(k) }).collect();
        IndCfg { overlays: ovs, subs, volma: (true, [5, 20]) }
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct LayerStats {
    pub paints: [u64; 3],
    pub paint_us: [u64; 3],
    pub rebuilds: u64,
    pub rebuild_us: u64,
}

pub struct Model {
    pub s: Series,
    pub name: String,
    pub price: f64,
    pub change: f64,
    pub volume: i64,
    pub anchored: bool,
    /// 週／月 K：最後一根量 = vol_base + 當日總量
    vol_base: f64,
    pub cfg: IndCfg,
    pub ovs: Vec<Overlay>,
    pub subs: Vec<Sub>,
    pub view: Option<View>,
    pub drawings: Vec<Drawing>,
    pub selected: Option<usize>,
    pub draw_gen: u64,
    pub pending: Option<Pending>,
    pub tool: Option<Tool>,
    pub drag: Option<DragState>,
    pub pan: Option<(f32, f64)>,
    pub busy: bool,
    frozen: Option<(f64, f64)>,
    pub mouse: Option<P>,
    pub bounds: Option<Bounds<Pixels>>,
    geo: Option<Geo>,
    geo_key: (Bounds<Pixels>, u64),
    pub layout_gen: u64,
    pub gc: Option<GeomCache>,
    pub pc: Option<(CacheKey, f64, PaintCache)>,
    pub text_edit: Option<TextEdit>,
    pub stats: LayerStats,
    cand: Vec<u32>,
}

impl Model {
    pub fn new(symbol: &str, name: &str, period: Period) -> Model {
        let s = match period {
            Period::Intraday => Series::empty(symbol, period),
            _ => synth(symbol, period, 0.0, 0.0, 0),
        };
        let mut m = Model {
            s,
            name: name.into(),
            price: 0.0,
            change: 0.0,
            volume: 0,
            anchored: false,
            vol_base: 0.0,
            cfg: IndCfg::default(),
            ovs: Vec::new(),
            subs: Vec::new(),
            view: None,
            drawings: Vec::new(),
            selected: None,
            draw_gen: 0,
            pending: None,
            tool: None,
            drag: None,
            pan: None,
            busy: false,
            frozen: None,
            mouse: None,
            bounds: None,
            geo: None,
            geo_key: (Bounds::default(), u64::MAX),
            layout_gen: 0,
            gc: None,
            pc: None,
            text_edit: None,
            stats: LayerStats::default(),
            cand: Vec::new(),
        };
        m.rebuild_indicators();
        m
    }

    pub fn symbol(&self) -> &str {
        &self.s.symbol
    }
    pub fn period(&self) -> Period {
        self.s.period
    }
    pub fn key(&self) -> String {
        format!("{}|{}", self.s.symbol, self.s.period.code())
    }

    fn touch(&mut self) {
        self.layout_gen = self.layout_gen.wrapping_add(1);
    }
    pub fn drawings_changed(&mut self) {
        self.draw_gen = self.draw_gen.wrapping_add(1);
        self.touch();
    }

    // ───────────── 資料 ─────────────

    pub fn load(&mut self, symbol: &str, name: &str, period: Period, quote: Option<(f64, f64, i64)>) {
        let (price, change, volume) = quote.unwrap_or((0.0, 0.0, 0));
        self.name = name.into();
        self.price = price;
        self.change = change;
        self.volume = volume;
        self.anchored = price > 0.0;
        self.s = match period {
            Period::Intraday => Series::empty(symbol, period),
            _ => synth(symbol, period, price, change, volume),
        };
        self.reset_vol_base();
        self.view = None;
        self.selected = None;
        self.pending = None;
        self.drag = None;
        self.pan = None;
        self.busy = false;
        self.frozen = None;
        self.text_edit = None;
        self.gc = None;
        self.pc = None;
        self.rebuild_indicators();
        self.drawings_changed();
    }

    fn reset_vol_base(&mut self) {
        self.vol_base = match (self.s.period, self.s.bars.last()) {
            (Period::Weekly | Period::Monthly, Some(l)) => l.v - self.volume.max(0) as f64,
            _ => 0.0,
        };
    }

    pub fn set_cfg(&mut self, cfg: IndCfg) {
        self.cfg = cfg;
        self.rebuild_indicators();
    }

    /// 換代號／週期／指標設定時一次 O(n)
    pub fn rebuild_indicators(&mut self) {
        let bars = &self.s.bars;
        self.ovs = self.cfg.overlays.iter().take(MAX_OVERLAYS).map(|&sp| Overlay::new(sp, bars)).collect();
        self.subs = self
            .cfg
            .subs
            .iter()
            .filter(|c| c.on)
            .take(MAX_SUBS)
            .map(|c| {
                let volma = (c.kind == SubKind::Vol && self.cfg.volma.0).then_some(self.cfg.volma.1);
                Sub::new(c.clone(), volma, bars)
            })
            .collect();
        self.gc = None;
        self.pc = None;
        self.touch();
    }

    /// 最後一根變了：每個指標只重算最後一根
    fn patch_last(&mut self) {
        let Some(i) = self.s.bars.len().checked_sub(1) else { return };
        let bars = &self.s.bars;
        for o in &mut self.ovs {
            o.step(bars, i);
        }
        for s in &mut self.subs {
            s.step(bars, i);
        }
    }

    /// 引擎報價（現價＋當日總量）
    pub fn apply_quote(&mut self, price: f64, change: f64, volume: i64, name: &str) -> u8 {
        if !(price > 0.0 && price < 10_000_000.0) {
            return 0;
        }
        if self.price == price && self.change == change && self.volume == volume {
            return 0;
        }
        self.price = price;
        self.change = change;
        self.volume = volume;
        if !name.is_empty() && self.name != name {
            self.name = name.into();
        }
        if self.s.period == Period::Intraday {
            return HEAD;
        }
        if !self.anchored {
            // 第一筆真實價：重鋪合成 K 讓最後一根對上實價（只發生一次），指標整段重算
            self.anchored = true;
            let sym = self.s.symbol.clone();
            self.s = synth(&sym, self.s.period, price, change, volume);
            self.reset_vol_base();
            self.view = None;
            self.rebuild_indicators();
            self.drawings_changed();
            return ALL;
        }
        let new_v = if self.s.period == Period::Daily { volume as f64 } else { self.vol_base + volume as f64 };
        let dv = self.s.bars.last().map_or(0.0, |l| new_v - l.v);
        if self.s.apply_price(price, dv) {
            self.patch_last();
            self.touch();
            return PLOT | LIVE | HEAD;
        }
        HEAD
    }

    /// 壓測用合成 tick（與 wry `__perfContinuous` 相同：last.c + sin(f/7)·0.15、量 +1）
    pub fn synth_tick(&mut self, frame: u64) -> u8 {
        let Some(last) = self.s.bars.last() else { return 0 };
        let price = last.c + (frame as f64 / 7.0).sin() * 0.15;
        self.s.apply_price(price, 1.0);
        self.price = price;
        self.patch_last();
        self.touch();
        PLOT | LIVE | HEAD
    }

    pub fn set_intraday(&mut self, prev: f64, bars: &[xq_feed::Bar]) -> u8 {
        self.s = series::from_feed(&self.s.symbol.clone(), prev, bars);
        self.view = None;
        self.rebuild_indicators();
        self.drawings_changed();
        ALL
    }

    pub fn apply_minute(&mut self, bar: &xq_feed::Bar) -> u8 {
        match self.s.apply_minute(bar) {
            None => 0,
            Some(false) => {
                self.patch_last();
                self.touch();
                PLOT | LIVE
            }
            Some(true) => {
                let n = self.s.bars.len();
                if n <= 3 {
                    self.rebuild_indicators();
                } else {
                    self.patch_last();
                }
                if let Some(v) = &mut self.view {
                    if v.right >= (n as f64) - 4.0 {
                        v.right = (n - 1) as f64;
                    }
                }
                self.touch();
                ALL
            }
        }
    }

    // ───────────── 版面 ─────────────

    fn init_view(&self, plot_w: f64) -> View {
        let n = self.s.bars.len().max(1) as f64;
        if self.s.period == Period::Intraday {
            View { bar_w: plot_w / n, right: n - 1.0 }
        } else {
            View { bar_w: if n * 8.0 < plot_w { plot_w / (n + 3.0) } else { 8.0 }, right: n - 1.0 + 2.0 }
        }
    }

    fn clamp_view(v: &mut View, n: usize, plot_w: f64) {
        let n = n.max(1) as f64;
        let plot_w = plot_w.max(50.0);
        v.bar_w = (2.0f64.min(plot_w / n)).max(60.0f64.min(v.bar_w));
        let visible = plot_w / v.bar_w;
        v.right = (n - 1.0).min(visible * 0.2).max((n - 1.0 + visible * 0.5).min(v.right));
    }

    /// 取得（必要時重算）版面。bounds 不同或資料／視圖改過才重算。
    pub fn geo(&mut self, b: Bounds<Pixels>) -> &Geo {
        if self.geo.is_none() || self.geo_key != (b, self.layout_gen) {
            self.bounds = Some(b);
            let g = self.compute_geo(b);
            self.geo = Some(g);
            self.geo_key = (b, self.layout_gen);
        }
        self.geo.as_ref().unwrap()
    }

    /// 用上一幀的 bounds 重算（互動／tick 後決定要不要通知畫線層）
    pub fn geo_now(&mut self) -> Option<&Geo> {
        let b = self.bounds?;
        Some(self.geo(b))
    }

    fn compute_geo(&mut self, b: Bounds<Pixels>) -> Geo {
        let (bx, by, bw, bh) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
        let plot_l = bx + PLOT_L;
        let plot_r = (plot_l + 80.0).max(bx + bw - AXIS_W);
        let plot_w = plot_r - plot_l;
        let n = self.s.bars.len();
        let mut v = self.view.unwrap_or_else(|| self.init_view(plot_w as f64));
        Self::clamp_view(&mut v, n, plot_w as f64);
        self.view = Some(v);
        let (bar_w, right) = (v.bar_w, v.right);
        let idx_at = |x: f32| right - (plot_r as f64 - bar_w / 2.0 - x as f64) / bar_w;
        let (i0, i1) = if n == 0 {
            (0, 0)
        } else {
            ((idx_at(plot_l).floor().max(0.0) as usize).min(n - 1), (idx_at(plot_r).ceil().max(0.0) as usize).min(n - 1))
        };
        let avail = (bh - TIME_H).max(60.0);
        let nb = self.subs.len();
        let (mut price_h, mut below_h) = (avail, 0.0);
        if nb > 0 {
            let share = if nb <= 1 { 0.78 } else if nb == 2 { 0.64 } else if nb <= 4 { 0.54 } else { 0.48 };
            price_h = (avail * share).round();
            below_h = (avail - price_h) / nb as f32;
            let min_sub = 16.0;
            if below_h < min_sub {
                price_h = (min_sub * 2.0).max(avail - nb as f32 * min_sub);
                below_h = (avail - price_h) / nb as f32;
            } else if nb <= 2 && below_h > 110.0 {
                below_h = 110.0f32.min(avail * if nb == 1 { 0.22 } else { 0.18 });
                price_h = avail - below_h * nb as f32;
            }
        }
        let price_hdr = 16.0 + 14.0 * (((self.ovs.len() + 1) as f32 / 4.0).ceil().clamp(1.0, 6.0));
        let mut panes = Vec::with_capacity(1 + nb);
        panes.push(Pane { kind: PaneKind::Price, top: by, h: price_h, hdr: price_hdr, y0: 0.0, ph: 0.0, lo: 0.0, hi: 1.0 });
        for k in 0..nb {
            panes.push(Pane { kind: PaneKind::Sub(k), top: by + price_h + below_h * k as f32, h: below_h, hdr: 13.0, y0: 0.0, ph: 0.0, lo: 0.0, hi: 1.0 });
        }
        let bars = &self.s.bars;
        let intraday = self.s.period == Period::Intraday;
        for pane in &mut panes {
            pane.y0 = pane.top + pane.hdr;
            pane.ph = (pane.h - pane.hdr - 3.0).max(8.0);
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            let mut take = |v: f64| {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            };
            let mut fixed = false;
            let mut zero_floor = false;
            if n > 0 {
                match pane.kind {
                    PaneKind::Price => {
                        for i in i0..=i1 {
                            if intraday {
                                take(bars[i].c);
                            } else {
                                take(bars[i].l);
                                take(bars[i].h);
                            }
                        }
                        for o in &self.ovs {
                            for i in i0..=i1 {
                                o.extend_range(i, &mut lo, &mut hi);
                            }
                        }
                        if intraday && self.s.prev > 0.0 {
                            lo = lo.min(self.s.prev);
                            hi = hi.max(self.s.prev);
                        }
                    }
                    PaneKind::Sub(k) => {
                        let sub = &self.subs[k];
                        if let Some(((a, b), _)) = sub.fixed() {
                            lo = a;
                            hi = b;
                            fixed = true;
                        } else {
                            for i in i0..=i1 {
                                if sub.cfg.kind == SubKind::Vol {
                                    take(bars[i].v);
                                }
                                if let Some(&hv) = sub.out.hist.get(i) {
                                    take(hv);
                                }
                                for l in &sub.out.lines {
                                    take(l[i]);
                                }
                            }
                            if matches!(sub.cfg.kind, SubKind::Vol | SubKind::Dmi) {
                                take(0.0);
                                zero_floor = true;
                            }
                            if sub.cfg.kind == SubKind::Macd {
                                let a = lo.abs().max(hi.abs());
                                let a = if a > 0.0 { a } else { 1.0 };
                                lo = -a;
                                hi = a;
                            }
                        }
                    }
                }
            }
            if !lo.is_finite() {
                lo = 0.0;
                hi = 1.0;
            }
            if !(hi > lo) {
                hi = lo + 1.0;
            }
            if !fixed {
                let m = (hi - lo) * 0.07;
                if !(zero_floor && lo == 0.0) {
                    lo -= m;
                }
                hi += m;
            }
            if pane.kind == PaneKind::Price {
                if self.busy {
                    // 平移／拖曳中鎖住價格軸（畫線層快取鍵才穩定，與 wry 相同）
                    if let Some((a, b)) = self.frozen {
                        lo = a;
                        hi = b;
                    } else {
                        self.frozen = Some((lo, hi));
                    }
                } else {
                    self.frozen = None;
                }
            }
            pane.lo = lo;
            pane.hi = hi;
        }
        let mut ticks = Vec::new();
        if n > 0 {
            if intraday {
                let mut i = 0;
                while i < n {
                    if i >= i0 && i <= i1 {
                        ticks.push(i);
                    }
                    i += 30;
                }
            } else {
                let step = ((80.0 / bar_w).ceil() as usize).max(1);
                let mut i = i0.div_ceil(step) * step;
                while i <= i1 {
                    ticks.push(i);
                    i += step;
                }
            }
        }
        Geo { b, plot_l, plot_r, plot_w, bottom: by + avail, i0, i1, n, bar_w, right, panes, ticks }
    }

    // ───────────── 畫線快取 ─────────────

    /// 目前視圖下的畫線快取鍵
    pub fn draw_key(&mut self) -> Option<(CacheKey, f64, f32)> {
        let sel = self.selected;
        let gen = self.draw_gen;
        let g = self.geo_now()?;
        let xf = g.xf();
        Some((CacheKey::of(&xf, gen, sel), xf.right, xf.plot_r - xf.plot_l))
    }

    /// 畫線層要不要重畫（含平移位移）
    pub fn draw_layer_stale(&mut self) -> bool {
        let Some((key, right, _)) = self.draw_key() else { return true };
        match &self.pc {
            Some((k, r, _)) => *k != key || *r != right,
            None => true,
        }
    }

    /// 幾何快取（hit-test 與畫線層共用）：鍵不同或平移超出預留邊界才重建
    pub fn ensure_gc(&mut self) -> f32 {
        let Some((key, right, w)) = self.draw_key() else { return 0.0 };
        let need = match &self.gc {
            Some(gc) => gc.key != key || ((right - gc.right) * key.bar_w).abs() as f32 > w * 0.45,
            None => true,
        };
        if need {
            let xf = self.geo.as_ref().unwrap().xf();
            self.gc = Some(GeomCache::build(&self.drawings, &xf, key));
            self.pc = None;
        }
        let gc = self.gc.as_ref().unwrap();
        (-(right - gc.right) * key.bar_w) as f32
    }

    /// 命中：(索引, 控制點)
    pub fn hit_test(&mut self, q: P) -> Option<(usize, Option<usize>)> {
        let dx = self.ensure_gc();
        let xf = self.geo.as_ref()?.xf();
        if let Some(sel) = self.selected {
            if let Some(d) = self.drawings.get(sel) {
                let g = drawings::geom(d, &xf);
                if let Some(h) = g.handles.iter().position(|h| (h.x - q.x).abs() <= 6.0 && (h.y - q.y).abs() <= 6.0) {
                    return Some((sel, Some(h)));
                }
                if drawings::hit_geom(q, &g) {
                    return Some((sel, None));
                }
            }
        }
        let gc = self.gc.as_ref()?;
        let qq = p(q.x - dx, q.y);
        let mut cand = std::mem::take(&mut self.cand);
        gc.grid.candidates(qq, &mut cand);
        let mut hit = None;
        for &k in &cand {
            let k = k as usize;
            if Some(k) == self.selected {
                continue;
            }
            let Some(g) = &gc.geoms[k] else { continue };
            // 水平線不跟著平移位移
            let probe = if self.drawings[k].kind == Tool::Hline { q } else { qq };
            if drawings::hit_geom(probe, g) {
                hit = Some((k, None));
                break;
            }
        }
        self.cand = cand;
        hit
    }

    fn anchor(&mut self, q: P) -> Option<Pt> {
        let g = self.geo_now()?;
        let xf = g.xf();
        Some(Pt { i: xf.i_at(q.x), p: xf.p_at(q.y) })
    }

    // ───────────── 互動 ─────────────

    fn finish(&mut self, mut d: Drawing) -> u8 {
        d.pts.truncate(d.kind.need());
        self.drawings.push(d);
        self.selected = Some(self.drawings.len() - 1);
        self.pending = None;
        self.tool = None;
        self.drawings_changed();
        DRAW | LIVE | SAVE | HEAD
    }

    pub fn mouse_down(&mut self, q: P, clicks: usize) -> u8 {
        if self.text_edit.is_some() {
            return self.commit_text(true);
        }
        let Some(g) = self.geo_now() else { return 0 };
        if q.x >= g.plot_r || q.y >= g.bottom || q.x < g.plot_l {
            return 0;
        }
        let in_price = g.in_price(q);
        if let Some(tool) = self.tool {
            if !in_price {
                return 0;
            }
            let Some(a) = self.anchor(q) else { return 0 };
            if tool == Tool::Text {
                self.text_edit = Some(TextEdit { anchor: a, edit_idx: None, text: String::new(), at: q });
                return LIVE;
            }
            if let Some(mut pd) = self.pending.take() {
                let last = pd.d.pts.len() - 1;
                pd.d.pts[last] = a;
                if pd.d.pts.len() >= tool.need() {
                    return self.finish(pd.d);
                }
                pd.d.pts.push(a);
                pd.dragging = true;
                pd.sx = q.x;
                pd.sy = q.y;
                self.pending = Some(pd);
                return LIVE;
            }
            if tool.need() == 1 {
                return self.finish(Drawing { kind: tool, pts: vec![a], text: String::new() });
            }
            self.pending = Some(Pending { d: Drawing { kind: tool, pts: vec![a, a], text: String::new() }, dragging: true, sx: q.x, sy: q.y });
            return LIVE;
        }
        // 游標：命中 → 選取／拖曳；沒命中 → 平移
        let prev = self.selected;
        if let Some((idx, handle)) = if in_price { self.hit_test(q) } else { None } {
            if clicks >= 2 && self.drawings[idx].kind == Tool::Text {
                let d = &self.drawings[idx];
                self.text_edit = Some(TextEdit { anchor: d.pts[0], edit_idx: Some(idx), text: d.text.clone(), at: q });
                self.selected = Some(idx);
                self.drawings_changed();
                return DRAW | LIVE;
            }
            let start = self.anchor(q).unwrap();
            self.selected = Some(idx);
            self.drag = Some(DragState { idx, handle, start, orig: self.drawings[idx].pts.clone(), moved: false });
            self.busy = true;
        } else {
            self.selected = None;
            let right = self.view.map_or(0.0, |v| v.right);
            self.pan = Some((q.x, right));
            self.busy = true;
        }
        if self.selected != prev {
            self.drawings_changed();
        }
        DRAW | LIVE | HEAD
    }

    /// 回傳要重畫哪幾層
    pub fn mouse_move(&mut self, q: P, inside: bool) -> u8 {
        let mut f = 0;
        let active = self.pan.is_some() || self.drag.is_some() || self.pending.as_ref().is_some_and(|p| p.dragging);
        let new_mouse = if inside || active { Some(q) } else { None };
        if new_mouse != self.mouse {
            self.mouse = new_mouse;
            f |= LIVE;
        }
        if let Some((x0, r0)) = self.pan {
            if let Some(v) = &mut self.view {
                v.right = r0 + (x0 - q.x) as f64 / v.bar_w;
            }
            self.touch();
            f |= PLOT | DRAW | LIVE;
        }
        if self.pending.is_some() {
            if let Some(a) = self.anchor(q) {
                let pd = self.pending.as_mut().unwrap();
                let last = pd.d.pts.len() - 1;
                pd.d.pts[last] = a;
                f |= LIVE;
            }
        }
        if let Some(dr) = self.drag.clone() {
            if let Some(a) = self.anchor(q) {
                let d = &mut self.drawings[dr.idx];
                match dr.handle {
                    Some(h) => apply_handle(d, h, a, &dr.orig),
                    None => {
                        let (di, dp) = (a.i - dr.start.i, a.p - dr.start.p);
                        d.pts = dr.orig.iter().map(|o| Pt { i: o.i + di, p: o.p + dp }).collect();
                    }
                }
                self.drag.as_mut().unwrap().moved = true;
                // 選取的那條在 live 層即時畫，不必重建畫線層
                f |= LIVE;
            }
        }
        f
    }

    pub fn mouse_up(&mut self, q: P) -> u8 {
        let mut f = 0;
        let was_busy = self.busy || self.pan.is_some() || self.drag.is_some();
        self.pan = None;
        if let Some(dr) = self.drag.take() {
            if dr.moved {
                f |= SAVE;
            }
        }
        if was_busy {
            self.busy = false;
            self.frozen = None;
            // 平移結束：重建畫線層（補齊移入視窗的邊緣），價格軸恢復自動
            self.drawings_changed();
            f |= ALL;
        }
        if let Some(pd) = &mut self.pending {
            if pd.dragging {
                pd.dragging = false;
                if ((q.x - pd.sx).powi(2) + (q.y - pd.sy).powi(2)).sqrt() > 4.0 {
                    let need = pd.d.kind.need();
                    let a = self.anchor(q).unwrap();
                    let pd = self.pending.as_mut().unwrap();
                    let last = pd.d.pts.len() - 1;
                    pd.d.pts[last] = a;
                    if pd.d.pts.len() >= need {
                        let d = self.pending.take().unwrap().d;
                        return f | self.finish(d);
                    }
                    pd.d.pts.push(a); // 平行通道：再點一下決定寬度
                }
                f |= LIVE;
            }
        }
        f
    }

    pub fn wheel(&mut self, dy: f32) -> u8 {
        if dy == 0.0 {
            return 0;
        }
        let n = self.s.bars.len();
        let Some(g) = self.geo_now() else { return 0 };
        let plot_w = g.plot_w as f64;
        if let Some(v) = &mut self.view {
            v.bar_w *= if dy > 0.0 { 1.15 } else { 1.0 / 1.15 };
            Self::clamp_view(v, n, plot_w);
        }
        self.touch();
        ALL
    }

    pub fn reset_view(&mut self) -> u8 {
        self.view = None;
        self.touch();
        ALL
    }

    pub fn delete_selected(&mut self) -> u8 {
        if let Some(k) = self.selected.take() {
            if k < self.drawings.len() {
                self.drawings.remove(k);
            }
            self.drawings_changed();
            return DRAW | LIVE | SAVE | HEAD;
        }
        0
    }

    pub fn clear_drawings(&mut self) -> u8 {
        self.drawings.clear();
        self.selected = None;
        self.drawings_changed();
        DRAW | LIVE | SAVE | HEAD
    }

    pub fn escape(&mut self) -> u8 {
        if self.text_edit.is_some() {
            return self.commit_text(false);
        }
        self.pending = None;
        self.tool = None;
        if self.selected.take().is_some() {
            self.drawings_changed();
        }
        DRAW | LIVE | HEAD
    }

    pub fn commit_text(&mut self, commit: bool) -> u8 {
        let Some(te) = self.text_edit.take() else { return 0 };
        self.tool = None;
        let text = te.text.trim().to_string();
        if !commit {
            return LIVE | HEAD;
        }
        match te.edit_idx {
            Some(k) => {
                if text.is_empty() {
                    self.drawings.remove(k);
                } else {
                    self.drawings[k].text = text;
                }
                self.selected = None;
                self.drawings_changed();
                DRAW | LIVE | SAVE | HEAD
            }
            None if !text.is_empty() => self.finish(Drawing { kind: Tool::Text, pts: vec![te.anchor], text }),
            None => LIVE | HEAD,
        }
    }

    pub fn set_tool(&mut self, tool: Option<Tool>) -> u8 {
        self.tool = tool;
        self.pending = None;
        LIVE | HEAD
    }

    /// 壓測：1000 筆混合畫線
    pub fn load_perf_drawings(&mut self, n: usize) {
        self.drawings = drawings::perf_mix(&self.s.bars, n);
        self.selected = None;
        self.drawings_changed();
    }

    /// 壓測平移：與 wry `__perfContinuous(pan)` 相同的 right = base + sin(f/6)·12
    pub fn set_right(&mut self, right: f64) {
        if let Some(v) = &mut self.view {
            v.right = right;
        }
        self.touch();
    }

    pub fn set_busy(&mut self, busy: bool) {
        if self.busy != busy {
            self.busy = busy;
            if !busy {
                self.frozen = None;
                self.drawings_changed();
            }
            self.touch();
        }
    }

    pub fn last_bar(&self) -> Option<&K> {
        self.s.bars.last()
    }
}

fn apply_handle(d: &mut Drawing, h: usize, a: Pt, orig: &[Pt]) {
    if d.kind == Tool::Rect {
        let (p0, q0) = (orig[0], orig[1]);
        match h {
            0 => d.pts[0] = a,
            1 => d.pts[1] = a,
            2 => {
                d.pts[0] = Pt { i: p0.i, p: a.p };
                d.pts[1] = Pt { i: a.i, p: q0.p };
            }
            _ => {
                d.pts[0] = Pt { i: a.i, p: p0.p };
                d.pts[1] = Pt { i: q0.i, p: a.p };
            }
        }
    } else if h < d.pts.len() {
        d.pts[h] = a;
    }
}

/// 合成序列：日 K（1200 根）；週／月由日 K 聚合
pub fn synth(symbol: &str, period: Period, price: f64, change: f64, volume: i64) -> Series {
    let mut s = series::synth_daily(symbol, price, change, volume);
    if matches!(period, Period::Weekly | Period::Monthly) {
        s.bars = series::aggregate(&s.bars, period);
        s.period = period;
    }
    s
}

#[allow(dead_code)]
pub fn bounds_of(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(w), px(h)))
}
