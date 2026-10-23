//! Android 小工具（egui／GPUI 手機版同一份）：
//! - 從系統屬性 `debug.xq.e0`…`debug.xq.e9`（每個 `KEY=VAL`）讀環境變數：桌面版的 XQ_* 開關原樣沿用
//!   （`adb shell setprop debug.xq.e0 XQ_CHART_STRESS=1`；屬性值上限 91 字元，所以拆成多個）。
//! - 把 stderr 導到 logcat（tag `xq`）：桌面版壓測印的 `BENCH …`／`CHARTPERF|…`／`TICKBENCH|…` 不用改。

use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader};
use std::os::raw::{c_char, c_int};

extern "C" {
    fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
}
#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

pub fn prop(name: &str) -> Option<String> {
    let n = CString::new(name).ok()?;
    let mut buf = [0 as c_char; 96];
    let len = unsafe { __system_property_get(n.as_ptr(), buf.as_mut_ptr()) };
    if len <= 0 {
        return None;
    }
    Some(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

/// 讀 debug.xq.e0..e9 設成環境變數；回傳設了哪些（印 log 用）
pub fn env_from_props() -> Vec<String> {
    let mut set = Vec::new();
    for k in 0..10 {
        if let Some(v) = prop(&format!("debug.xq.e{k}")) {
            if let Some((a, b)) = v.split_once('=') {
                if !a.is_empty() {
                    std::env::set_var(a.trim(), b.trim());
                    set.push(v.clone());
                }
            }
        }
    }
    set
}

pub fn log(s: &str) {
    let tag = CString::new("xq").unwrap();
    if let Ok(t) = CString::new(s) {
        unsafe { __android_log_write(4, tag.as_ptr(), t.as_ptr()) };
    }
}

/// stderr（fd 2）→ pipe → 背景執行緒逐行寫 logcat
pub fn stderr_to_logcat() {
    let mut fds = [0 as c_int; 2];
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], 2);
        libc::close(fds[1]);
    }
    let read_fd = fds[0];
    std::thread::Builder::new()
        .name("stderr-logcat".into())
        .spawn(move || {
            use std::os::fd::FromRawFd;
            let f = unsafe { std::fs::File::from_raw_fd(read_fd) };
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                log(&line);
            }
        })
        .ok();
}

/// 共同啟動：logcat、環境變數、HOME（存檔位置）
pub fn init(data_dir: Option<std::path::PathBuf>) {
    stderr_to_logcat();
    if let Some(d) = data_dir {
        std::env::set_var("HOME", &d);
        std::env::set_var("XDG_DATA_HOME", &d);
    }
    let set = env_from_props();
    eprintln!("[xq] android start, env from props: {set:?}");
}

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
