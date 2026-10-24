//! GPUI 自己的 log（`log` crate）原本沒有人接，所以 Metal／wgpu renderer 的訊息（例如「increased instance buffer size」
//! 繪圖指令緩衝區變大、「failed to retrieve next drawable」）全部看不到。這裡接一個最小的 logger：
//! 只印 target 以 gpui 開頭的訊息，格式 `GPUILOG|秒數|等級|target|訊息`（stderr）。
//! XQ_GPUI_LOG=off 關掉；=debug 連 debug 一起印（Linux 的 wgpu renderer 用 debug 記緩衝區變大）；預設 info。

struct L(log::LevelFilter);

impl log::Log for L {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.0 && m.target().starts_with("gpui")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("GPUILOG|{:.3}|{}|{}|{}", crate::stall::since_start_s(), r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

pub fn init() {
    let lv = match std::env::var("XQ_GPUI_LOG").ok().as_deref() {
        Some("off") | Some("0") => return,
        Some("debug") => log::LevelFilter::Debug,
        Some("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Info,
    };
    let lg: &'static L = Box::leak(Box::new(L(lv)));
    if log::set_logger(lg).is_ok() {
        log::set_max_level(lv);
    }
}
