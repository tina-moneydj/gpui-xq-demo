//! macOS：讓 malloc 把 free 掉的大塊馬上還給系統（2026-10-25）。
//!
//! **為什麼要有這個：** Tina 的 M4 Mac 重圖表情境，vmmap 看到 GPUI 版有一塊「Malloc Large (empty)」201 MB（128 塊、
//! 每塊約 2.6 MB），egui 是 0；我們程式真正在用的 heap 只有約 18 MB。RSS 因此是 321 vs 131 MB。
//! - 原因：新版 macOS 的 malloc（xzone malloc，vmmap 裡有「Deferred Reclaim」區）對大於 2 MB 的「huge」配置，
//!   free 之後不馬上還，而是放進 huge 快取／延遲回收緩衝，等系統記憶體吃緊時才由核心收回。這些頁面還算在
//!   RSS 和「活動監視器」的記憶體裡。
//! - 重圖表每幀都有幾塊這種 2–3 MB 的暫存，大多在 GPUI 裡面（推測）：快取的子 view 每幀把上一幀的 Path 複製進新的
//!   `Scene`、`paint_path` 內部 `scale()` 再複製一次、Metal renderer 每幀把全部 path 頂點組成一個新的 Vec。
//!   我們自己的畫線快取只有約 1 萬個頂點，重用緩衝省不掉。egui 把整個畫面合成一份頂點緩衝重用，所以沒有這個現象。
//! - Windows（HeapAlloc）和 Linux（glibc：大塊 mmap、free 就 munmap）沒有這種快取，所以只有 Mac 會多 200 MB。
//! - 第一版（69b87bd）每 0.5 秒呼叫 `malloc_zone_pressure_relief`：CI 的 macOS 26 實測沒用——xzone malloc 的
//!   pressure_relief 是空的 fallback（libmalloc 原始碼 xzone_malloc.c），舊的 magazine malloc 才有作用。
//!
//! **怎麼修（不改 GPUI）：** libmalloc 的正式環境變數 `MallocLargeCache=0` 會關掉 huge 快取和延遲回收
//! （舊的 magazine malloc 則是關掉 large「death-row」快取）。它只在程式啟動時讀一次，所以 `main()` 一開始：
//! 沒設過就設好、用同一個執行檔 exec 自己一次（同一個 PID，多花幾 ms 啟動）。正式產品打包成 .app 時，改在
//! Info.plist 的 `LSEnvironment` 設同一個變數就不用 exec。
//! 代價：free 掉的大塊立刻還給系統，下一幀重新要（多一些缺頁），CPU 稍高一點；Mac 壓測包有對照欄可以看。
//! `XQ_MALLOC_LARGE_CACHE=1` 保留系統預設（不 exec，A/B 對照用；Mac 壓測包的 gpui-largecache 欄就是這樣跑）。

/// 在 `main()` 最前面呼叫（還沒開任何視窗、執行緒之前）。macOS 以外什麼都不做。
#[cfg(target_os = "macos")]
pub fn ensure_no_large_cache() {
    use std::os::unix::process::CommandExt;
    if std::env::var("XQ_MALLOC_LARGE_CACHE").ok().as_deref() == Some("1") {
        return;
    }
    if std::env::var_os("MallocLargeCache").is_some() {
        return; // 已經設過（exec 回來的這一次，或使用者自己設的）：照它的值跑
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // exec 成功就不會回來；失敗（極少見）就照系統預設繼續跑
    let err = std::process::Command::new(exe).args(args).env("MallocLargeCache", "0").exec();
    eprintln!("[memrelief] 設定 MallocLargeCache=0 後重新啟動失敗，照系統預設繼續：{err}");
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_no_large_cache() {}

/// CHARTPERF 用：`,"mallocLargeCache":0`（0＝已關掉快取；1＝系統預設）
#[cfg(target_os = "macos")]
pub fn kv() -> String {
    let off = std::env::var("MallocLargeCache").ok().as_deref() == Some("0");
    format!(",\"mallocLargeCache\":{}", if off { 0 } else { 1 })
}
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn kv() -> String {
    String::new()
}
