//! 走勢圖序列＋增量指標。每筆 tick 只改最後一根與各 MA 的最後一個值（O(期數)），
//! 新增一根時只 append，不整段重算。

use xq_feed::Bar as FeedBar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    /// 分時（引擎分鐘線）
    Intraday,
    /// 日 K（合成，最後一根跟著現價）
    Daily,
}

impl Period {
    pub fn code(self) -> &'static str {
        match self {
            Period::Intraday => "T",
            Period::Daily => "D",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Period::Intraday => "分時",
            Period::Daily => "日線",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct K {
    /// 分時：當日分鐘數（540 = 09:00）；日線：yyyymmdd
    pub t: u32,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

#[derive(Clone, Debug)]
pub struct Ma {
    pub n: usize,
    pub vals: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Series {
    pub symbol: String,
    pub period: Period,
    pub bars: Vec<K>,
    pub mas: Vec<Ma>,
    /// 昨收（分時參考線）
    pub prev: f64,
    /// true = 來自引擎；false = 合成
    pub live: bool,
}

pub const MA_PERIODS_INTRADAY: [usize; 2] = [5, 20];
pub const MA_PERIODS_DAILY: [usize; 3] = [5, 20, 60];

impl Series {
    pub fn new(symbol: &str, period: Period, bars: Vec<K>, prev: f64, live: bool) -> Self {
        let periods: &[usize] = match period {
            Period::Intraday => &MA_PERIODS_INTRADAY,
            Period::Daily => &MA_PERIODS_DAILY,
        };
        let mut s = Series {
            symbol: symbol.to_string(),
            period,
            bars,
            mas: periods.iter().map(|&n| Ma { n, vals: Vec::new() }).collect(),
            prev,
            live,
        };
        s.rebuild_mas();
        s
    }

    pub fn empty(symbol: &str, period: Period) -> Self {
        Series::new(symbol, period, Vec::new(), 0.0, false)
    }

    /// 只在換代號／重設時跑一次（O(n)，滑動和）。
    fn rebuild_mas(&mut self) {
        for ma in &mut self.mas {
            ma.vals.clear();
            ma.vals.reserve(self.bars.len());
            let mut sum = 0.0;
            for (i, b) in self.bars.iter().enumerate() {
                sum += b.c;
                if i >= ma.n {
                    sum -= self.bars[i - ma.n].c;
                }
                ma.vals.push(if i + 1 >= ma.n { sum / ma.n as f64 } else { f64::NAN });
            }
        }
    }

    fn ma_at(&self, n: usize, i: usize) -> f64 {
        if i + 1 < n {
            return f64::NAN;
        }
        let mut sum = 0.0;
        for b in &self.bars[i + 1 - n..=i] {
            sum += b.c;
        }
        sum / n as f64
    }

    fn refresh_last_ma(&mut self) {
        let Some(i) = self.bars.len().checked_sub(1) else { return };
        for k in 0..self.mas.len() {
            let v = self.ma_at(self.mas[k].n, i);
            let vals = &mut self.mas[k].vals;
            if vals.len() == i + 1 {
                vals[i] = v;
            } else {
                vals.push(v);
            }
        }
    }

    /// 現價 tick：只動最後一根。
    pub fn apply_price(&mut self, price: f64, day_volume: Option<i64>) -> bool {
        let Some(last) = self.bars.last_mut() else { return false };
        if last.c == price && day_volume.map_or(true, |v| last.v == v as f64) {
            return false;
        }
        last.c = price;
        last.h = last.h.max(price);
        last.l = last.l.min(price);
        if let Some(v) = day_volume {
            last.v = v as f64;
        }
        self.refresh_last_ma();
        true
    }

    /// 引擎分鐘線：同一分鐘改最後一根，新分鐘才 append。
    pub fn apply_minute(&mut self, b: &FeedBar) {
        let k = K { t: b.t as u32, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v as f64 };
        match self.bars.last_mut() {
            Some(last) if last.t == k.t => {
                last.h = last.h.max(k.h);
                last.l = last.l.min(k.l);
                last.c = k.c;
                last.v = k.v;
            }
            Some(last) if last.t > k.t => return, // 舊分鐘，忽略
            _ => self.bars.push(k),
        }
        self.live = true;
        self.refresh_last_ma();
    }
}

pub fn from_feed(symbol: &str, prev: f64, bars: &[FeedBar]) -> Series {
    let ks = bars
        .iter()
        .map(|b| K { t: b.t as u32, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v as f64 })
        .collect();
    Series::new(symbol, Period::Intraday, ks, prev, true)
}

// ---- 合成日線（引擎不給日 K；與 wry 版相同做法：最後一根對上現價）----

struct Rng(u64);
impl Rng {
    fn seed(s: &str, salt: u64) -> Self {
        let mut h = 1469598103934665603u64 ^ salt;
        for b in s.bytes() {
            h = (h ^ b as u64).wrapping_mul(1099511628211);
        }
        Rng(h | 1)
    }
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let u = self.next().max(1e-12);
        let v = self.next();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// 往回推 n 個交易日（跳過週末），回傳 yyyymmdd，舊→新。
fn trading_days(n: usize) -> Vec<u32> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let mut day = secs.div_euclid(86_400);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let wd = (day + 4).rem_euclid(7); // 0 = 週日
        if wd != 0 && wd != 6 {
            out.push(civil(day));
        }
        day -= 1;
    }
    out.reverse();
    out
}

fn civil(days: i64) -> u32 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y * 10_000 + m * 100 + d) as u32
}

pub const N_DAILY: usize = 260;

pub fn synth_daily(symbol: &str, price: f64, change: f64, day_volume: i64) -> Series {
    let mut r = Rng::seed(symbol, 11);
    let dates = trading_days(N_DAILY);
    let base_vol = 3000.0 + r.next() * 30000.0;
    let drift = (r.next() - 0.35) * 0.0012;
    let mut bars = Vec::with_capacity(N_DAILY);
    let mut prev = 100.0;
    for &t in &dates {
        let o = prev * (1.0 + r.gauss() * 0.004);
        let c = prev * (1.0 + drift + r.gauss() * 0.016);
        let h = o.max(c) * (1.0 + r.gauss().abs() * 0.005);
        let l = o.min(c) * (1.0 - r.gauss().abs() * 0.005);
        let v = base_vol * (0.5 + r.next()) * (1.0 + 12.0 * (c / prev - 1.0).abs());
        bars.push(K { t, o, h, l, c, v });
        prev = c;
    }
    let prev_close = if price > 0.0 { price - change } else { 100.0 };
    let f = prev_close / bars[N_DAILY - 2].c;
    for b in &mut bars {
        b.o = round2(b.o * f);
        b.h = round2(b.h * f);
        b.l = round2(b.l * f);
        b.c = round2(b.c * f);
        b.v = b.v.round();
    }
    if day_volume > 0 {
        // 合成量縮放到跟引擎總量同一個量級，最後一根才不會把量圖壓扁
        let mean = bars.iter().map(|b| b.v).sum::<f64>() / bars.len() as f64;
        let k = day_volume as f64 / mean.max(1.0);
        for b in &mut bars {
            b.v = (b.v * k).round();
        }
        bars.last_mut().unwrap().v = day_volume as f64;
    }
    if price > 0.0 {
        let last = bars.last_mut().unwrap();
        last.o = round2(prev_close * (1.0 + r.gauss() * 0.002));
        last.c = price;
        last.h = round2(last.o.max(price) * (1.0 + r.next() * 0.004));
        last.l = round2(last.o.min(price) * (1.0 - r.next() * 0.004));
    }
    Series::new(symbol, Period::Daily, bars, prev_close, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_ma_matches_rebuild() {
        let mut s = synth_daily("2330", 1035.0, 12.0, 0);
        s.apply_price(1040.0, Some(123));
        let inc: Vec<f64> = s.mas.iter().map(|m| *m.vals.last().unwrap()).collect();
        s.rebuild_mas();
        let full: Vec<f64> = s.mas.iter().map(|m| *m.vals.last().unwrap()).collect();
        for (a, b) in inc.iter().zip(full) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn civil_known() {
        assert_eq!(civil(0), 19700101);
        assert_eq!(civil(20_000), 20241004);
    }
}
