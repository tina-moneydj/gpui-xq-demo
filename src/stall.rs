//! A4 第二版（2026-10-25）：主執行緒卡頓偵測（原生版；網頁版不編）。
//!
//! 實機 Mac 每輪 GPUI 都有一次約 0.3–0.5 秒的停頓。舊的 LONGFRAME 只在「行情到了、隔很久才畫」時印，
//! 主執行緒整個卡住（連行情喚醒都排不進來）時看不到；也分不出是「整個程序被暫停」（例如壓測包自己的 vmmap）
//! 還是「只有主執行緒卡住」（GPUI 繪製、字型、AppKit）。這裡改成：
//! - 主執行緒在幾個固定地方打卡（root render 開頭、這一幀我們的 paint 結束、行情喚醒、每秒統計、後備計時器），
//!   後備計時器每 25ms 一次，所以正常時打卡間隔 ≤ 25ms。
//! - 背景看門狗每 5ms 看一次：超過 XQ_STALL_MS（預設 120ms）沒打卡＝卡住。恢復後印一行
//!   `STALL|{"t":開始秒數,"ms":長度,"phase":卡住前最後一個打卡點,"feedFrames":卡住期間背景執行緒收到的封包數,"procSuspended":…}`。
//!   - feedFrames > 0：背景收行情的執行緒照常在跑 → 只有主執行緒卡住。
//!   - procSuspended＝看門狗自己也睡過頭（> 60ms）→ 整個程序被暫停（vmmap、除錯器、系統）。
//!   - phase＝painted：我們這一幀已經畫完，卡在 GPUI 送出／present（Metal nextDrawable）或系統事件；
//!     phase＝render：卡在這一幀的版面／排字／paint 裡（含 GPUI 排字、字型後援）。
//! - macOS 且設了 XQ_STALL_SAMPLE_DIR：第一次（最多 2 次）偵測到「只有主執行緒卡住」時，立刻在背景跑
//!   `/usr/bin/sample <pid> 1 1`，把卡住中的呼叫堆疊存成 `stall-<pid>-<n>.txt`（壓測包會一起打包回傳）。

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Phase {
    Start = 0,
    Render = 1,
    Painted = 2,
    Wake = 3,
    Timer = 4,
    Fallback = 5,
}

fn phase_name(p: u8) -> &'static str {
    match p {
        1 => "render",
        2 => "painted",
        3 => "wake",
        4 => "timer",
        5 => "fallback",
        _ => "start",
    }
}

static BEAT_US: AtomicU64 = AtomicU64::new(0);
static PHASE: AtomicU8 = AtomicU8::new(0);
static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn now_us() -> u64 {
    T0.get_or_init(Instant::now).elapsed().as_micros() as u64
}

/// 程式啟動到現在的秒數（LONGFRAME／STALL 共用同一個起點）
pub fn since_start_s() -> f64 {
    now_us() as f64 / 1e6
}

#[inline]
pub fn beat(p: Phase) {
    BEAT_US.store(now_us(), Ordering::Relaxed);
    PHASE.store(p as u8, Ordering::Relaxed);
}

/// 開看門狗（只開一次）。`feed_frames`：背景執行緒收到的封包累計數。
pub fn start(feed_frames: Arc<dyn Fn() -> u64 + Send + Sync>) {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let off = std::env::var("XQ_STALL_MS").ok().as_deref() == Some("0");
    if off {
        return;
    }
    let thr_us = std::env::var("XQ_STALL_MS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(120.0) * 1000.0;
    let sample_dir = std::env::var("XQ_STALL_SAMPLE_DIR").ok().filter(|d| !d.is_empty());
    beat(Phase::Start);
    let _ = std::thread::Builder::new().name("xq-stall-watchdog".into()).spawn(move || {
        let mut last_loop = now_us();
        // (開始時間, 開始時的 phase, 開始時的封包數, 程序是否被暫停, 是否取樣)
        let mut cur: Option<(u64, u8, u64, bool, bool)> = None;
        let mut n_stalls = 0u32;
        let mut n_samples = 0u32;
        loop {
            std::thread::sleep(Duration::from_millis(5));
            let now = now_us();
            let overslept = now.saturating_sub(last_loop) > 60_000;
            last_loop = now;
            let beat_at = BEAT_US.load(Ordering::Relaxed);
            match cur.as_mut() {
                None => {
                    if (now.saturating_sub(beat_at) as f64) > thr_us {
                        let phase = PHASE.load(Ordering::Relaxed);
                        let mut sampled = false;
                        #[cfg(target_os = "macos")]
                        if !overslept && n_samples < 2 {
                            if let Some(dir) = sample_dir.as_ref() {
                                n_samples += 1;
                                sampled = spawn_sample(dir, n_samples);
                            }
                        }
                        let _ = (&sample_dir, &mut n_samples);
                        cur = Some((beat_at, phase, feed_frames(), overslept, sampled));
                    }
                }
                Some(st) => {
                    if overslept {
                        st.3 = true;
                    }
                    if beat_at > st.0 {
                        n_stalls += 1;
                        let (t0, phase, f0, susp, sampled) = *st;
                        // 恢復後第一個打卡點＝這次停頓的結束
                        eprintln!(
                            "STALL|{{\"t\":{:.3},\"ms\":{:.1},\"phase\":\"{}\",\"resumedAt\":\"{}\",\"feedFrames\":{},\"procSuspended\":{},\"sampled\":{},\"n\":{}}}",
                            t0 as f64 / 1e6,
                            (beat_at - t0) as f64 / 1000.0,
                            phase_name(phase),
                            phase_name(PHASE.load(Ordering::Relaxed)),
                            feed_frames().saturating_sub(f0),
                            susp,
                            sampled,
                            n_stalls
                        );
                        cur = None;
                    }
                }
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn spawn_sample(dir: &str, n: u32) -> bool {
    let pid = std::process::id();
    let _ = std::fs::create_dir_all(dir);
    let file = format!("{dir}/stall-{pid}-{n}.txt");
    match std::process::Command::new("/usr/bin/sample")
        .args([pid.to_string(), "1".into(), "1".into(), "-mayDie".into(), "-file".into(), file])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}
