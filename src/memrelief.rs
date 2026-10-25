//! macOS：定期把 malloc「已 free、但還佔著實體記憶體」的大塊還給系統（2026-10-25）。
//!
//! **為什麼要有這個：** Tina 的 M4 Mac（24 GB）重圖表情境，vmmap 看到 GPUI 版有一塊
//! 「Malloc Large (empty)」201 MB（128 塊，每塊約 2.6 MB），egui 是 0；我們程式真正在用的 heap 只有約 18 MB。
//! - 原因：macOS 的 malloc（magazine zone）在 32 GB 以下的機器不開 medium 區，大於 small 上限的配置都走「large」＝
//!   每塊各自 mmap。free 之後不馬上還給系統，而是放進「death-row 快取」等下次重用，最多 128 塊（就是 vmmap 那 128 塊）。
//! - 重圖表每幀都有幾塊這種 2–3 MB 的暫存：1000 筆畫線三角化後的 Path 頂點陣列，GPUI 的 `paint_path` 要拿走一份
//!   （我們 clone 一次）、內部 `scale()` 再複製一次、`Scene` 又 clone 一次；Metal renderer 每幀再組一次頂點緩衝。
//!   這些都是 GPUI API 決定的，我們這層能省的只有自己那一次 clone，省不掉。大小每幀略有不同（平移、選取、換層），
//!   快取重用不完，就一路累積到 128 塊上限。
//! - Windows（HeapAlloc）和 Linux（glibc，大塊用 mmap、free 就 munmap）沒有這種快取，所以只有 Mac 會多 200 MB。
//!
//! **怎麼修（不改 GPUI）：** 背景執行緒每 `XQ_MALLOC_RELIEF_MS`（預設 500 ms）呼叫一次
//! `malloc_zone_pressure_relief(NULL, 0)`——這就是系統記憶體吃緊時 libmalloc 自己會做的事：清空 death-row 快取、
//! 把 tiny／small 的空頁 madvise 還回去。不在 UI 執行緒做（規則 1）；只影響 free 掉的記憶體，不碰在用的資料。
//! 代價：清空之後下一幀那幾塊要重新 mmap（約 10 MB 的缺頁），每 0.5 秒一次。
//! `XQ_MALLOC_RELIEF_MS=0` 關掉（A/B 對照用；Mac 壓測包的 gpui-norelief 欄就是這樣跑）。
//!
//! `malloc_zone_pressure_relief` 的回傳值不含 death-row 那部分（libmalloc 內部變數遮蔽），
//! 所以「還了多少」用呼叫前後的常駐記憶體（mach task_info resident_size）差估算，印在 CHARTPERF 的 `mallocReliefMb`。

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// 累計還回去的 KB（常駐記憶體差，估算）
    static RELEASED_KB: AtomicU64 = AtomicU64::new(0);
    static CALLS: AtomicU64 = AtomicU64::new(0);
    static INTERVAL_MS: AtomicU64 = AtomicU64::new(0);

    #[repr(C)]
    #[derive(Default)]
    struct TimeValue {
        seconds: i32,
        microseconds: i32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: TimeValue,
        system_time: TimeValue,
        policy: i32,
        suspend_count: i32,
    }
    const MACH_TASK_BASIC_INFO: u32 = 20;

    extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }

    fn resident_bytes() -> u64 {
        let mut info = MachTaskBasicInfo::default();
        let mut count = (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<i32>()) as u32;
        // SAFETY: info 是正確大小的 repr(C) 結構，count 以 natural_t（4 bytes）為單位
        let kr = unsafe { task_info(mach_task_self_, MACH_TASK_BASIC_INFO, &mut info as *mut _ as *mut i32, &mut count) };
        if kr == 0 { info.resident_size } else { 0 }
    }

    pub fn start() {
        let ms = std::env::var("XQ_MALLOC_RELIEF_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(500);
        if ms == 0 {
            eprintln!("[memrelief] 關閉（XQ_MALLOC_RELIEF_MS=0）");
            return;
        }
        let ms = ms.max(50);
        INTERVAL_MS.store(ms, Ordering::Relaxed);
        let r = std::thread::Builder::new().name("xq-malloc-relief".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_millis(ms));
            let before = resident_bytes();
            // SAFETY: NULL＝所有 zone、goal 0＝能還多少還多少；libmalloc 自己加鎖，任何執行緒都可呼叫
            unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
            let after = resident_bytes();
            RELEASED_KB.fetch_add(before.saturating_sub(after) / 1024, Ordering::Relaxed);
            CALLS.fetch_add(1, Ordering::Relaxed);
        });
        match r {
            Ok(_) => eprintln!("[memrelief] 每 {ms} ms 呼叫 malloc_zone_pressure_relief（XQ_MALLOC_RELIEF_MS=0 關閉）"),
            Err(e) => eprintln!("[memrelief] 背景執行緒開不起來：{e}"),
        }
    }

    /// CHARTPERF 用：`,"mallocReliefMs":500,"mallocReliefN":…,"mallocReliefMb":…`（關掉時 ms=0）
    pub fn kv() -> String {
        format!(
            ",\"mallocReliefMs\":{},\"mallocReliefN\":{},\"mallocReliefMb\":{:.1}",
            INTERVAL_MS.load(Ordering::Relaxed),
            CALLS.load(Ordering::Relaxed),
            RELEASED_KB.load(Ordering::Relaxed) as f64 / 1024.0
        )
    }
}

#[cfg(target_os = "macos")]
pub use imp::{kv, start};

/// 其他平台沒有 death-row 快取問題：什麼都不做
#[cfg(not(target_os = "macos"))]
pub fn start() {}
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn kv() -> String {
    String::new()
}
