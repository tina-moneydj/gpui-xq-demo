//! 技術指標（與 wry 版 chart.js 的 CALC 公式一致）。
//!
//! 每個指標都寫成「第 i 根只依賴第 i-1 根狀態」的 step 函式：
//! - 全量 = 對 0..n 逐根 step（換代號／換參數時一次 O(n)）
//! - tick（最後一根變）= 只 step(n-1)，O(週期) 或 O(1)
//! - 新增一根 = push 後 step(n-1)
//! 遞迴型指標（EMA、SAR、KD、MACD、RSI、DMI、ATR、OBV）把每根的內部狀態存成陣列，
//! 所以增量結果與全量重算逐位相同（見 tests）。

use crate::series::K;
use serde::{Deserialize, Serialize};

pub const MAX_OVERLAYS: usize = 20;
pub const MAX_SUBS: usize = 10;

#[inline]
fn set(v: &mut Vec<f64>, i: usize, x: f64) {
    if i < v.len() {
        v[i] = x;
    } else {
        debug_assert_eq!(i, v.len());
        v.push(x);
    }
}

fn sma_at(bars: &[K], i: usize, n: usize, f: impl Fn(&K) -> f64) -> f64 {
    if n == 0 || i + 1 < n {
        return f64::NAN;
    }
    let mut s = 0.0;
    for b in &bars[i + 1 - n..=i] {
        s += f(b);
    }
    s / n as f64
}

fn high_low(bars: &[K], i: usize, n: usize) -> (f64, f64) {
    let from = (i + 1).saturating_sub(n.max(1));
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    for b in &bars[from..=i] {
        hi = hi.max(b.h);
        lo = lo.min(b.l);
    }
    (hi, lo)
}

fn true_range(b: &[K], i: usize) -> f64 {
    if i == 0 {
        b[0].h - b[0].l
    } else {
        (b[i].h - b[i].l).max((b[i].h - b[i - 1].c).abs()).max((b[i].l - b[i - 1].c).abs())
    }
}

// ───────────────────────── 主圖疊加 ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OvSpec {
    Ma { period: usize },
    Ema { period: usize },
    Bb { n: usize, k: f64 },
    Sar { step: f64, max: f64 },
}

impl OvSpec {
    pub fn label(&self) -> String {
        match *self {
            OvSpec::Ma { period } => format!("MA{period}"),
            OvSpec::Ema { period } => format!("EMA{period}"),
            OvSpec::Bb { n, k } => format!("BB({n},{k})"),
            OvSpec::Sar { step, max } => format!("SAR({step},{max})"),
        }
    }
    pub fn kind_label(&self) -> &'static str {
        match self {
            OvSpec::Ma { .. } => "MA",
            OvSpec::Ema { .. } => "EMA",
            OvSpec::Bb { .. } => "布林",
            OvSpec::Sar { .. } => "SAR",
        }
    }
    /// 「重」指標：平移／拖曳時延後（與 wry HEAVY_IND 同分類，但這裡增量本身就是 O(週期)，不會整段重算）
    pub fn defaults(kind: &str) -> OvSpec {
        match kind {
            "ema" => OvSpec::Ema { period: 12 },
            "bb" => OvSpec::Bb { n: 20, k: 2.0 },
            "sar" => OvSpec::Sar { step: 0.02, max: 0.2 },
            _ => OvSpec::Ma { period: 20 },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OvOut {
    /// MA / EMA：線；BB：中軌
    pub line: Vec<f64>,
    pub up: Vec<f64>,
    pub lo: Vec<f64>,
    /// SAR：點位與方向（1 = 多）
    pub sar: Vec<f64>,
    pub sar_up: Vec<bool>,
    // SAR 內部狀態
    ep: Vec<f64>,
    af: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub spec: OvSpec,
    pub out: OvOut,
}

impl Overlay {
    pub fn new(spec: OvSpec, bars: &[K]) -> Self {
        let mut o = Overlay { spec, out: OvOut::default() };
        o.rebuild(bars);
        o
    }

    pub fn rebuild(&mut self, bars: &[K]) {
        self.out = OvOut::default();
        for i in 0..bars.len() {
            self.step(bars, i);
        }
    }

    /// 只重算第 i 根（i 必須 ≤ 目前長度）
    pub fn step(&mut self, bars: &[K], i: usize) {
        let o = &mut self.out;
        match self.spec {
            OvSpec::Ma { period } => set(&mut o.line, i, sma_at(bars, i, period.max(1), |b| b.c)),
            OvSpec::Ema { period } => {
                let a = 2.0 / (period.max(1) as f64 + 1.0);
                let v = if i == 0 { bars[0].c } else { o.line[i - 1] + a * (bars[i].c - o.line[i - 1]) };
                set(&mut o.line, i, v);
            }
            OvSpec::Bb { n, k } => {
                let n = n.max(1);
                let mid = sma_at(bars, i, n, |b| b.c);
                let (u, l) = if mid.is_finite() {
                    let mut s = 0.0;
                    for b in &bars[i + 1 - n..=i] {
                        s += (b.c - mid) * (b.c - mid);
                    }
                    let sd = (s / n as f64).sqrt();
                    (mid + k * sd, mid - k * sd)
                } else {
                    (f64::NAN, f64::NAN)
                };
                set(&mut o.line, i, mid);
                set(&mut o.up, i, u);
                set(&mut o.lo, i, l);
            }
            OvSpec::Sar { step, max } => {
                let n = bars.len();
                // 與 wry 相同：n < 3 全部 NaN；第 0 根的初始方向看第 1 根
                let (s, ep, af, up, out) = if n < 3 {
                    (f64::NAN, f64::NAN, step, true, f64::NAN)
                } else if i == 0 {
                    let up = bars[1].c >= bars[0].c;
                    let ep = if up { bars[0].h } else { bars[0].l };
                    let s = if up { bars[0].l } else { bars[0].h };
                    (s, ep, step, up, f64::NAN)
                } else {
                    let (mut s, mut ep, mut af, mut up) = (o.sar[i - 1], o.ep[i - 1], o.af[i - 1], o.sar_up[i - 1]);
                    if !s.is_finite() || !ep.is_finite() {
                        // 前一根是 n<3 時的占位狀態：從頭初始化
                        up = bars[1].c >= bars[0].c;
                        ep = if up { bars[0].h } else { bars[0].l };
                        s = if up { bars[0].l } else { bars[0].h };
                        af = step;
                    }
                    s += af * (ep - s);
                    let p2 = i.saturating_sub(2);
                    if up {
                        s = s.min(bars[i - 1].l).min(bars[p2].l);
                        if bars[i].l < s {
                            up = false;
                            s = ep;
                            ep = bars[i].l;
                            af = step;
                        } else if bars[i].h > ep {
                            ep = bars[i].h;
                            af = max.min(af + step);
                        }
                    } else {
                        s = s.max(bars[i - 1].h).max(bars[p2].h);
                        if bars[i].h > s {
                            up = true;
                            s = ep;
                            ep = bars[i].h;
                            af = step;
                        } else if bars[i].l < ep {
                            ep = bars[i].l;
                            af = max.min(af + step);
                        }
                    }
                    (s, ep, af, up, s)
                };
                // sar[] 存「狀態 s」，畫圖時第 0 根不畫（out = NaN 只影響第 0 根）
                let _ = out;
                set(&mut o.sar, i, s);
                set(&mut o.ep, i, ep);
                set(&mut o.af, i, af);
                if i < o.sar_up.len() {
                    o.sar_up[i] = up;
                } else {
                    o.sar_up.push(up);
                }
            }
        }
    }

    /// 第 i 根要畫的 SAR 值（第 0 根與 n<3 不畫）
    pub fn sar_at(&self, i: usize) -> f64 {
        if i == 0 { f64::NAN } else { self.out.sar.get(i).copied().unwrap_or(f64::NAN) }
    }

    pub fn truncate(&mut self, n: usize) {
        let o = &mut self.out;
        for v in [&mut o.line, &mut o.up, &mut o.lo, &mut o.sar, &mut o.ep, &mut o.af] {
            v.truncate(n);
        }
        o.sar_up.truncate(n);
    }

    /// 可視範圍 min/max（價格軸）
    pub fn extend_range(&self, i: usize, lo: &mut f64, hi: &mut f64) {
        let mut take = |v: f64| {
            if v.is_finite() {
                *lo = lo.min(v);
                *hi = hi.max(v);
            }
        };
        match self.spec {
            OvSpec::Ma { .. } | OvSpec::Ema { .. } => take(self.out.line[i]),
            OvSpec::Bb { .. } => {
                take(self.out.line[i]);
                take(self.out.up[i]);
                take(self.out.lo[i]);
            }
            OvSpec::Sar { .. } => take(self.sar_at(i)),
        }
    }
}

// ───────────────────────── 副圖 ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubKind {
    Vol,
    Kd,
    Macd,
    Rsi,
    Wr,
    Dmi,
    Atr,
    Obv,
}

pub const SUB_ORDER: [SubKind; 8] =
    [SubKind::Vol, SubKind::Kd, SubKind::Macd, SubKind::Rsi, SubKind::Wr, SubKind::Dmi, SubKind::Atr, SubKind::Obv];

impl SubKind {
    pub fn label(self) -> &'static str {
        match self {
            SubKind::Vol => "成交量",
            SubKind::Kd => "KD 隨機指標",
            SubKind::Macd => "MACD",
            SubKind::Rsi => "RSI",
            SubKind::Wr => "威廉指標 %R",
            SubKind::Dmi => "DMI / ADX",
            SubKind::Atr => "ATR 真實波幅",
            SubKind::Obv => "OBV 能量潮",
        }
    }
    /// 參數名稱與預設值（與 wry DEFAULT_CFG 相同）
    pub fn params(self) -> &'static [(&'static str, f64)] {
        match self {
            SubKind::Vol | SubKind::Obv => &[],
            SubKind::Kd => &[("週期", 9.0), ("K", 3.0), ("D", 3.0)],
            SubKind::Macd => &[("快", 12.0), ("慢", 26.0), ("訊號", 9.0)],
            SubKind::Rsi | SubKind::Wr | SubKind::Dmi | SubKind::Atr => &[("週期", 14.0)],
        }
    }
}

/// 副圖設定（存檔用）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubCfg {
    pub kind: SubKind,
    pub on: bool,
    pub p: [f64; 3],
}

impl SubCfg {
    pub fn default_for(kind: SubKind) -> SubCfg {
        let mut p = [0.0; 3];
        for (k, (_, v)) in kind.params().iter().enumerate() {
            p[k] = *v;
        }
        SubCfg { kind, on: kind == SubKind::Vol, p }
    }
}

/// 一個副圖的計算結果。`lines[k]` 是第 k 條線；`hist` 是柱（MACD OSC）。
#[derive(Clone, Debug, Default)]
pub struct SubOut {
    pub lines: Vec<Vec<f64>>,
    pub hist: Vec<f64>,
    /// 內部遞迴狀態（每根一格）
    st: Vec<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct Sub {
    pub cfg: SubCfg,
    pub out: SubOut,
    /// 成交量副圖附帶的均量線（volma）
    pub volma: Option<[usize; 2]>,
}

impl Sub {
    pub fn new(cfg: SubCfg, volma: Option<[usize; 2]>, bars: &[K]) -> Self {
        let mut s = Sub { cfg, out: SubOut::default(), volma };
        s.rebuild(bars);
        s
    }

    fn n_lines(&self) -> (usize, usize) {
        match self.cfg.kind {
            SubKind::Vol => (if self.volma.is_some() { 2 } else { 0 }, 0),
            SubKind::Kd => (2, 2),          // lines: K D；st: pk pd
            SubKind::Macd => (2, 2),        // lines: DIF DEA；st: emaFast emaSlow；hist: OSC
            SubKind::Rsi => (1, 2),         // st: ag al
            SubKind::Wr => (1, 0),
            SubKind::Dmi => (3, 6),         // +DI -DI ADX；st: trS pS mS adxV dxSum dxCount
            SubKind::Atr => (1, 1),         // st: sum(tr) 前 n 根
            SubKind::Obv => (1, 0),
        }
    }

    pub fn rebuild(&mut self, bars: &[K]) {
        let (nl, ns) = self.n_lines();
        self.out = SubOut {
            lines: vec![Vec::with_capacity(bars.len()); nl],
            hist: Vec::new(),
            st: vec![Vec::with_capacity(bars.len()); ns],
        };
        for i in 0..bars.len() {
            self.step(bars, i);
        }
    }

    pub fn truncate(&mut self, n: usize) {
        for v in self.out.lines.iter_mut().chain(self.out.st.iter_mut()) {
            v.truncate(n);
        }
        self.out.hist.truncate(n);
    }

    pub fn step(&mut self, b: &[K], i: usize) {
        let p = self.cfg.p;
        let o = &mut self.out;
        let nan = f64::NAN;
        match self.cfg.kind {
            SubKind::Vol => {
                if let Some([a, c]) = self.volma {
                    set(&mut o.lines[0], i, sma_at(b, i, a, |x| x.v));
                    set(&mut o.lines[1], i, sma_at(b, i, c, |x| x.v));
                }
            }
            SubKind::Kd => {
                let (n, kk, dd) = (p[0].max(1.0) as usize, p[1].max(1.0), p[2].max(1.0));
                let (hi, lo) = high_low(b, i, n);
                let rsv = if hi > lo { (b[i].c - lo) / (hi - lo) * 100.0 } else { 50.0 };
                let (pk0, pd0) = if i == 0 { (50.0, 50.0) } else { (o.st[0][i - 1], o.st[1][i - 1]) };
                let pk = ((kk - 1.0) * pk0 + rsv) / kk;
                let pd = ((dd - 1.0) * pd0 + pk) / dd;
                set(&mut o.st[0], i, pk);
                set(&mut o.st[1], i, pd);
                let show = i + 1 >= n;
                set(&mut o.lines[0], i, if show { pk } else { nan });
                set(&mut o.lines[1], i, if show { pd } else { nan });
            }
            SubKind::Macd => {
                let ema = |prev: f64, v: f64, n: f64| prev + 2.0 / (n + 1.0) * (v - prev);
                let (f, s) = if i == 0 {
                    (b[0].c, b[0].c)
                } else {
                    (ema(o.st[0][i - 1], b[i].c, p[0].max(1.0)), ema(o.st[1][i - 1], b[i].c, p[1].max(1.0)))
                };
                let dif = f - s;
                let dea = if i == 0 { dif } else { ema(o.lines[1][i - 1], dif, p[2].max(1.0)) };
                set(&mut o.st[0], i, f);
                set(&mut o.st[1], i, s);
                set(&mut o.lines[0], i, dif);
                set(&mut o.lines[1], i, dea);
                set(&mut o.hist, i, dif - dea);
            }
            SubKind::Rsi => {
                let n = p[0].max(1.0) as usize;
                let nf = n as f64;
                let (ag, al, out) = if i == 0 {
                    (0.0, 0.0, nan)
                } else {
                    let ch = b[i].c - b[i - 1].c;
                    let (g, l) = (ch.max(0.0), (-ch).max(0.0));
                    let (pa, pl) = (o.st[0][i - 1], o.st[1][i - 1]);
                    let (ag, al) = if i <= n { (pa + g / nf, pl + l / nf) } else { ((pa * (nf - 1.0) + g) / nf, (pl * (nf - 1.0) + l) / nf) };
                    let out = if i >= n { if al == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + ag / al) } } else { nan };
                    (ag, al, out)
                };
                set(&mut o.st[0], i, ag);
                set(&mut o.st[1], i, al);
                set(&mut o.lines[0], i, out);
            }
            SubKind::Wr => {
                let n = p[0].max(1.0) as usize;
                let v = if i + 1 < n {
                    nan
                } else {
                    let (hi, lo) = high_low(b, i, n);
                    if hi > lo { (hi - b[i].c) / (hi - lo) * -100.0 } else { -50.0 }
                };
                set(&mut o.lines[0], i, v);
            }
            SubKind::Dmi => {
                let n = p[0].max(1.0) as usize;
                let nf = n as f64;
                // st: 0 trS 1 pS 2 mS 3 adxV 4 dxSum 5 dxCount
                let (mut tr_s, mut p_s, mut m_s, mut adx_v, mut dx_sum, mut dx_cnt) = if i == 0 {
                    (0.0, 0.0, 0.0, nan, 0.0, 0.0)
                } else {
                    (o.st[0][i - 1], o.st[1][i - 1], o.st[2][i - 1], o.st[3][i - 1], o.st[4][i - 1], o.st[5][i - 1])
                };
                let (mut pdi, mut mdi, mut adx) = (nan, nan, nan);
                if i >= 1 {
                    let up_m = b[i].h - b[i - 1].h;
                    let dn_m = b[i - 1].l - b[i].l;
                    let pdm = if up_m > dn_m && up_m > 0.0 { up_m } else { 0.0 };
                    let mdm = if dn_m > up_m && dn_m > 0.0 { dn_m } else { 0.0 };
                    let tr = true_range(b, i);
                    if i <= n {
                        tr_s += tr;
                        p_s += pdm;
                        m_s += mdm;
                    } else {
                        tr_s += tr - tr_s / nf;
                        p_s += pdm - p_s / nf;
                        m_s += mdm - m_s / nf;
                    }
                    if i >= n && tr_s > 0.0 {
                        let pp = 100.0 * p_s / tr_s;
                        let mm = 100.0 * m_s / tr_s;
                        pdi = pp;
                        mdi = mm;
                        let dx = if pp + mm != 0.0 { 100.0 * (pp - mm).abs() / (pp + mm) } else { 0.0 };
                        if dx_cnt < nf {
                            dx_sum += dx;
                            dx_cnt += 1.0;
                            if dx_cnt == nf {
                                adx_v = dx_sum / nf;
                            }
                        } else {
                            adx_v = (adx_v * (nf - 1.0) + dx) / nf;
                        }
                        if dx_cnt >= nf {
                            adx = adx_v;
                        }
                    }
                }
                for (k, v) in [tr_s, p_s, m_s, adx_v, dx_sum, dx_cnt].into_iter().enumerate() {
                    set(&mut o.st[k], i, v);
                }
                set(&mut o.lines[0], i, pdi);
                set(&mut o.lines[1], i, mdi);
                set(&mut o.lines[2], i, adx);
            }
            SubKind::Atr => {
                let n = p[0].max(1.0) as usize;
                let tr = true_range(b, i);
                let acc = if i == 0 { tr } else { o.st[0][i - 1] + if i < n { tr } else { 0.0 } };
                set(&mut o.st[0], i, acc);
                let v = if i + 1 < n {
                    nan
                } else if i + 1 == n {
                    acc / n as f64
                } else {
                    (o.lines[0][i - 1] * (n as f64 - 1.0) + tr) / n as f64
                };
                set(&mut o.lines[0], i, v);
            }
            SubKind::Obv => {
                let v = if i == 0 {
                    0.0
                } else {
                    let prev = o.lines[0][i - 1];
                    prev + if b[i].c > b[i - 1].c { b[i].v } else if b[i].c < b[i - 1].c { -b[i].v } else { 0.0 }
                };
                set(&mut o.lines[0], i, v);
            }
        }
    }

    /// 標題（圖例第一段）
    pub fn title(&self) -> String {
        let p = self.cfg.p;
        match self.cfg.kind {
            SubKind::Vol => "成交量".into(),
            SubKind::Kd => format!("KD({},{},{})", p[0], p[1], p[2]),
            SubKind::Macd => format!("MACD({},{},{})", p[0], p[1], p[2]),
            SubKind::Rsi => format!("RSI({})", p[0]),
            SubKind::Wr => format!("%R({})", p[0]),
            SubKind::Dmi => format!("DMI({})", p[0]),
            SubKind::Atr => format!("ATR({})", p[0]),
            SubKind::Obv => "OBV".into(),
        }
    }

    /// 線名
    pub fn line_names(&self) -> &'static [&'static str] {
        match self.cfg.kind {
            SubKind::Vol => &["MA", "MA"],
            SubKind::Kd => &["K", "D"],
            SubKind::Macd => &["DIF", "MACD"],
            SubKind::Dmi => &["+DI", "-DI", "ADX"],
            _ => &[""],
        }
    }

    /// 固定刻度（KD／RSI／%R）與參考線
    pub fn fixed(&self) -> Option<((f64, f64), &'static [f64])> {
        match self.cfg.kind {
            SubKind::Kd => Some(((0.0, 100.0), &[20.0, 50.0, 80.0])),
            SubKind::Rsi => Some(((0.0, 100.0), &[30.0, 50.0, 70.0])),
            SubKind::Wr => Some(((-100.0, 0.0), &[-80.0, -50.0, -20.0])),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::synth_daily;

    fn all_specs() -> Vec<OvSpec> {
        vec![
            OvSpec::Ma { period: 20 },
            OvSpec::Ema { period: 12 },
            OvSpec::Bb { n: 20, k: 2.0 },
            OvSpec::Sar { step: 0.02, max: 0.2 },
        ]
    }

    fn close(a: f64, b: f64) -> bool {
        (a.is_nan() && b.is_nan()) || (a - b).abs() < 1e-9 * (1.0 + a.abs())
    }

    #[test]
    fn incremental_equals_full() {
        let s = synth_daily("2330", 1035.0, 12.0, 100_000);
        let mut bars = s.bars.clone();
        let mut ovs: Vec<Overlay> = all_specs().into_iter().map(|sp| Overlay::new(sp, &bars)).collect();
        let mut subs: Vec<Sub> = SUB_ORDER
            .iter()
            .map(|&k| Sub::new(SubCfg { on: true, ..SubCfg::default_for(k) }, (k == SubKind::Vol).then_some([5, 20]), &bars))
            .collect();
        // 30 筆 tick + 3 根新棒
        for t in 0..60 {
            if t % 20 == 19 {
                let last = *bars.last().unwrap();
                bars.push(K { t: last.t + 1, o: last.c, h: last.c, l: last.c, c: last.c, v: 0.0 });
            }
            let n = bars.len();
            let last = bars.last_mut().unwrap();
            last.c += ((t as f64) * 0.7).sin() * 3.0;
            last.h = last.h.max(last.c);
            last.l = last.l.min(last.c);
            last.v += 17.0;
            for o in &mut ovs {
                o.step(&bars, n - 1);
            }
            for s in &mut subs {
                s.step(&bars, n - 1);
            }
        }
        for o in &ovs {
            let full = Overlay::new(o.spec, &bars);
            for (a, b) in [(&o.out.line, &full.out.line), (&o.out.up, &full.out.up), (&o.out.lo, &full.out.lo), (&o.out.sar, &full.out.sar)] {
                assert_eq!(a.len(), b.len());
                for i in 0..a.len() {
                    assert!(close(a[i], b[i]), "{:?} i={i} {} vs {}", o.spec, a[i], b[i]);
                }
            }
        }
        for s in &subs {
            let full = Sub::new(s.cfg.clone(), s.volma, &bars);
            for (la, lb) in s.out.lines.iter().zip(&full.out.lines) {
                for i in 0..la.len() {
                    assert!(close(la[i], lb[i]), "{:?} i={i} {} vs {}", s.cfg.kind, la[i], lb[i]);
                }
            }
        }
    }
}
