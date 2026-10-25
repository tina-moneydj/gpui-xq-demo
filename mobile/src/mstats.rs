//! 手機版（Android／iOS 共用）的量測小工具：每秒 FRAMES 行、指標計算時間差分、XQ_IND_BENCH 微基準。
//! 原本在 droid.rs，iOS 加進來後搬出來共用（egui 手機版有同一份）。

/// 手機版每秒一行：FPS、超過 32ms 的幀數、最長幀距（實際觸控壓測用，XQ_METRICS=1 才印）
#[derive(Default)]
pub struct FrameLog {
    last: Option<std::time::Instant>,
    pub frames: u32,
    pub jank: u32,
    pub max_ms: f64,
}

impl FrameLog {
    pub fn frame(&mut self) {
        let now = std::time::Instant::now();
        if let Some(p) = self.last.replace(now) {
            let dt = now.duration_since(p).as_secs_f64() * 1000.0;
            // 只算「連續在畫」的幀距（>250ms 視為閒置，不算 jank）
            if dt > 32.0 && dt < 250.0 {
                self.jank += 1;
            }
            if dt < 250.0 && dt > self.max_ms {
                self.max_ms = dt;
            }
        }
        self.frames += 1;
    }
    pub fn report(&mut self, extra: &str) {
        eprintln!(
            "FRAMES|{{\"t\":{},\"fps\":{},\"jank\":{},\"maxMs\":{:.1}{extra}}}",
            xq_feed::unix_us(),
            self.frames,
            self.jank,
            self.max_ms
        );
        self.frames = 0;
        self.jank = 0;
        self.max_ms = 0.0;
    }
}

/// 指標計算時間：每秒差分（整段重算次數／平均毫秒、補最後一根次數／平均微秒）
#[derive(Default)]
pub struct IndDelta(u64, u64, u64, u64);
impl IndDelta {
    pub fn json(&mut self, now: (u64, u64, u64, u64)) -> String {
        let (fn_, fu, pn, pns) = (now.0 - self.0, now.1 - self.1, now.2 - self.2, now.3 - self.3);
        *self = IndDelta(now.0, now.1, now.2, now.3);
        format!(
            ",\"indFull\":{},\"indFullMs\":{:.2},\"indPatch\":{},\"indPatchUs\":{:.2}",
            fn_,
            if fn_ > 0 { fu as f64 / fn_ as f64 / 1000.0 } else { 0.0 },
            pn,
            if pn > 0 { pns as f64 / pn as f64 / 1000.0 } else { 0.0 }
        )
    }
}

/// XQ_IND_BENCH=N：啟動時用壓測指標組整段重算 N 次，印 INDPERF（中位數／最小／最大，毫秒）
pub fn ind_bench_report(bars: usize, mut v: Vec<f64>) {
    if v.is_empty() {
        return;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    eprintln!(
        "INDPERF|{{\"bars\":{},\"reps\":{},\"medianMs\":{:.3},\"minMs\":{:.3},\"maxMs\":{:.3}}}",
        bars,
        v.len(),
        v[v.len() / 2],
        v[0],
        v[v.len() - 1]
    );
}

pub fn ind_bench_reps() -> usize {
    std::env::var("XQ_IND_BENCH").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}
