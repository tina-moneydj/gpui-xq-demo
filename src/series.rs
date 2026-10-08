//! 走勢圖序列（K 棒）。指標在 indicators.rs，tick 時只改最後一根。

use xq_feed::Bar as FeedBar;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    /// 分時（引擎分鐘線）
    Intraday,
    /// 日 K（合成，最後一根跟著現價）
    Daily,
    /// 週 K／月 K：由合成日 K 聚合
    Weekly,
    Monthly,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::Intraday, Period::Daily, Period::Weekly, Period::Monthly];
    pub fn code(self) -> &'static str {
        match self {
            Period::Intraday => "T",
            Period::Daily => "D",
            Period::Weekly => "W",
            Period::Monthly => "M",
        }
    }
    pub fn from_code(s: &str) -> Option<Period> {
        Period::ALL.into_iter().find(|p| p.code().eq_ignore_ascii_case(s))
    }
    pub fn label(self) -> &'static str {
        match self {
            Period::Intraday => "分時",
            Period::Daily => "日K",
            Period::Weekly => "週K",
            Period::Monthly => "月K",
        }
    }
    /// 引擎訂閱用的週期（W/M 由日線聚合，跟引擎要日線現價即可）
    pub fn feed_code(self) -> &'static str {
        match self {
            Period::Intraday => "T",
            _ => "D",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct K {
    /// 分時：當日分鐘數（540 = 09:00）；日／週／月：yyyymmdd（該期最後一個交易日）
    pub t: u32,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

#[derive(Clone, Debug)]
pub struct Series {
    pub symbol: String,
    pub period: Period,
    pub bars: Vec<K>,
    /// 昨收（分時參考線）
    pub prev: f64,
    /// true = 來自引擎；false = 合成
    pub live: bool,
}

impl Series {
    pub fn empty(symbol: &str, period: Period) -> Self {
        Series { symbol: symbol.to_string(), period, bars: Vec::new(), prev: 0.0, live: false }
    }

    /// 現價 tick：只動最後一根。`vol_delta`：這筆 tick 的成交量增量（日／週／月累加）。
    pub fn apply_price(&mut self, price: f64, vol_delta: f64) -> bool {
        let Some(last) = self.bars.last_mut() else { return false };
        if last.c == price && vol_delta == 0.0 {
            return false;
        }
        last.c = price;
        last.h = last.h.max(price);
        last.l = last.l.min(price);
        last.v += vol_delta;
        true
    }

    /// 引擎分鐘線：同一分鐘改最後一根（回傳 false），新分鐘才 append（回傳 true）。
    pub fn apply_minute(&mut self, b: &FeedBar) -> Option<bool> {
        let k = K { t: b.t as u32, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v as f64 };
        let appended = match self.bars.last_mut() {
            Some(last) if last.t == k.t => {
                last.h = last.h.max(k.h);
                last.l = last.l.min(k.l);
                last.c = k.c;
                last.v = k.v;
                false
            }
            Some(last) if last.t > k.t => return None, // 舊分鐘，忽略
            _ => {
                self.bars.push(k);
                true
            }
        };
        self.live = true;
        Some(appended)
    }
}

pub fn from_feed(symbol: &str, prev: f64, bars: &[FeedBar]) -> Series {
    let ks = bars
        .iter()
        .map(|b| K { t: b.t as u32, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v as f64 })
        .collect();
    Series { symbol: symbol.to_string(), period: Period::Intraday, bars: ks, prev, live: true }
}

/// 日 K → 週 K／月 K（與 wry aggregate 相同：同一期合併，t 取該期最後一天）
pub fn aggregate(daily: &[K], period: Period) -> Vec<K> {
    let key = |t: u32| -> i64 {
        match period {
            Period::Monthly => (t / 100) as i64,
            _ => {
                // 週：以該日所屬週一的 days-from-epoch 當 key
                let d = days_from_civil((t / 10_000) as i64, (t / 100 % 100) as i64, (t % 100) as i64);
                d - (d + 3).rem_euclid(7)
            }
        }
    };
    let mut out: Vec<K> = Vec::new();
    let mut cur_key = i64::MIN;
    for b in daily {
        let k = key(b.t);
        if k != cur_key {
            out.push(*b);
            cur_key = k;
        } else {
            let c = out.last_mut().unwrap();
            c.t = b.t;
            c.h = c.h.max(b.h);
            c.l = c.l.min(b.l);
            c.c = b.c;
            c.v += b.v;
        }
    }
    out
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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

/// 與 wry 版 chart.js 相同：合成 1200 根日 K
pub const N_DAILY: usize = 1200;

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
    Series { symbol: symbol.to_string(), period: Period::Daily, bars, prev: prev_close, live: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_known() {
        assert_eq!(civil(0), 19700101);
        assert_eq!(civil(20_000), 20241004);
        assert_eq!(days_from_civil(2024, 10, 4), 20_000);
    }

    #[test]
    fn weekly_monthly() {
        let d = synth_daily("2330", 1000.0, 5.0, 0);
        let w = aggregate(&d.bars, Period::Weekly);
        let m = aggregate(&d.bars, Period::Monthly);
        assert!(w.len() > 230 && w.len() < 260, "{}", w.len());
        assert!(m.len() > 50 && m.len() < 60, "{}", m.len());
        assert_eq!(w.last().unwrap().c, 1000.0);
        let vsum: f64 = d.bars.iter().map(|b| b.v).sum();
        let wsum: f64 = w.iter().map(|b| b.v).sum();
        assert!((vsum - wsum).abs() < 1e-6);
    }
}
