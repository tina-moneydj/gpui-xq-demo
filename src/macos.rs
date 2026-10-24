//! macOS：告訴系統這是延遲敏感的即時程式（2026-10-25）。
//! `NSProcessInfo beginActivityWithOptions:`＝NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical：
//! 關掉 App Nap 與計時器合併（timer coalescing），但不阻止系統閒置睡眠。看盤／下單程式本來就該這樣宣告；
//! 這次也是 0.3–0.5 秒停頓的候選原因之一。XQ_MAC_ACTIVITY=0 關掉（A/B 對照用）。
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

pub fn begin_latency_critical_activity() {
    if std::env::var("XQ_MAC_ACTIVITY").ok().as_deref() == Some("0") {
        return;
    }
    // NSActivityUserInitiated = 0x00FFFFFF | IdleSystemSleepDisabled(1<<20)；去掉禁止睡眠那一位
    const USER_INITIATED_ALLOWING_IDLE_SLEEP: u64 = 0x00FF_FFFF & !(1u64 << 20);
    const LATENCY_CRITICAL: u64 = 0xFF_0000_0000;
    unsafe {
        let info: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
        if info.is_null() {
            return;
        }
        let reason: *mut Object =
            msg_send![class!(NSString), stringWithUTF8String: b"XQ realtime quotes\0".as_ptr() as *const std::os::raw::c_char];
        let token: *mut Object =
            msg_send![info, beginActivityWithOptions: USER_INITIATED_ALLOWING_IDLE_SLEEP | LATENCY_CRITICAL reason: reason];
        if !token.is_null() {
            // 整個程式期間都有效：retain 一次、永不 release
            let _: *mut Object = msg_send![token, retain];
            eprintln!("[macos] beginActivity: latency-critical（App Nap／計時器合併關閉）");
        }
    }
}
