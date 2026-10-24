//! iOS 小工具（GPUI／egui 手機版同一份）：
//! - 環境變數：Xcode 的 Scheme → Run → Environment Variables，或模擬器 `SIMCTL_CHILD_XQ_…=… xcrun simctl launch …`；
//!   桌面版的 XQ_* 開關原樣沿用。
//! - 行情：手機上沒有電腦的行情服務，所以預設在 app 裡開 `xq-demofeed`（127.0.0.1:47631，合成行情）。
//!   要連 Mac 上的真行情服務：設 `XQ_FEED_ADDR=192.168.x.x:47631`（第一次會跳「區域網路」權限）。
//! - log：stderr 直接出現在 Xcode 下方的主控台。

/// `log` 巨集（gpui／gpui-mobile 內部的警告、錯誤）印到 stderr＝Xcode 主控台
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{} {}] {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}
static LOGGER: StderrLog = StderrLog;

/// 啟動時印出 CoreText 找得到的字型（PingFang TC／Menlo／Helvetica）與檔案位置，字型問題好查
fn font_report() {
    for name in ["PingFangTC-Regular", "Menlo-Regular", "Helvetica"] {
        match core_text::font::new_from_name(name, 16.0) {
            Ok(f) => eprintln!("[fonts] {name} → family={:?} path={:?}", f.family_name(), f.copy_descriptor().font_path()),
            Err(()) => eprintln!("[fonts] {name} → CoreText 找不到"),
        }
    }
}

/// 共同啟動：環境變數預設值、app 內建行情
pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    font_report();
    if std::env::var_os("XQ_FEED_ADDR").is_none() && std::env::var("XQ_DEMO_FEED").map(|v| v != "0").unwrap_or(true) {
        let addr = format!("127.0.0.1:{}", xq_feed::DEFAULT_PORT);
        match xq_demofeed::spawn(&addr, std::time::Duration::from_millis(25)) {
            Ok(()) => eprintln!("[xq] iOS：app 內建合成行情 {addr}（不是真實行情）"),
            Err(e) => eprintln!("[xq] iOS：內建行情開不起來（{e}），改連既有的 {addr}"),
        }
    }
    let set: Vec<String> = std::env::vars().filter(|(k, _)| k.starts_with("XQ_")).map(|(k, v)| format!("{k}={v}")).collect();
    eprintln!("[xq] ios start, env: {set:?}");
}

/// 實體記憶體（mach task_info 的 resident_size，MB）；跟 Xcode 記憶體儀表的 footprint 不完全一樣
#[allow(deprecated)]
pub fn rss_mb() -> f64 {
    unsafe {
        let mut info: libc::mach_task_basic_info = std::mem::zeroed();
        let mut count = (std::mem::size_of::<libc::mach_task_basic_info>() / std::mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
        let r = libc::task_info(libc::mach_task_self(), libc::MACH_TASK_BASIC_INFO, &mut info as *mut _ as libc::task_info_t, &mut count);
        if r == 0 { info.resident_size as f64 / 1048576.0 } else { 0.0 }
    }
}
