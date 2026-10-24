//! # xq-tokens：XQ 共用設計 token
//!
//! 設計師改 `tokens/*.tokens.json`（W3C DTCG 2025.10，Figma Variables 原生匯出／Tokens Studio 都吐這個格式），
//! egui 版與 GPUI 版都從這裡讀顏色、字級、間距、圓角、線寬、版面尺寸，不再各自寫死。
//!
//! - **編譯時**：`build.rs` 驗證 JSON（缺 token、別名錯、型別錯 → 編譯失敗並列出 token 名）；內容用 `include_str!` 內嵌進執行檔。
//! - **執行時**：第一次呼叫 [`t()`] 時解析一次（< 1ms），之後每次讀 token = 一次原子讀取＋指標取值，沒有鎖。
//! - **換主題**：[`set_mode`]／[`toggle_mode`]（深色↔淺色），[`generation`] 會 +1，App 看到就清快取重畫。
//! - **熱更新**：`XQ_TOKENS_DIR=路徑` 從磁碟讀 token（不用重編）；debug 版或 `XQ_TOKENS_WATCH=1` 時每 300ms 檢查檔案，
//!   改了就重載（解析失敗會印錯誤並保留舊值）。
//! - `XQ_THEME=light` 指定起始主題。

mod dtcg;
mod theme;

use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

pub use dtcg::{Rgba, Tok};
pub use theme::*;

const EMBEDDED: [(&str, &str); 4] = [
    ("xq.resolver.json", include_str!("../tokens/xq.resolver.json")),
    ("base.tokens.json", include_str!("../tokens/base.tokens.json")),
    ("color.dark.tokens.json", include_str!("../tokens/color.dark.tokens.json")),
    ("color.light.tokens.json", include_str!("../tokens/color.light.tokens.json")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }
}

static CUR: AtomicPtr<Theme> = AtomicPtr::new(std::ptr::null_mut());
static GEN: AtomicU64 = AtomicU64::new(0);
static STATE: Mutex<Option<State>> = Mutex::new(None);
static LISTENERS: Mutex<Vec<Box<dyn Fn() + Send + Sync>>> = Mutex::new(Vec::new());

struct State {
    mode: Mode,
    dark: &'static Theme,
    light: &'static Theme,
}

/// token 從哪裡讀：XQ_TOKENS_DIR → 那個資料夾；否則內嵌
fn disk_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| std::env::var_os("XQ_TOKENS_DIR").map(PathBuf::from)).clone()
}

fn read_file(name: &str) -> Result<String, String> {
    if let Some(d) = disk_dir() {
        return std::fs::read_to_string(d.join(name)).map_err(|e| format!("{}: {e}", d.join(name).display()));
    }
    EMBEDDED.iter().find(|(n, _)| *n == name).map(|(_, s)| s.to_string()).ok_or_else(|| format!("內嵌檔沒有 {name}"))
}

/// 解析一個主題（不影響目前狀態）
pub fn load(mode: Mode) -> Result<Theme, Vec<String>> {
    let m = dtcg::resolve_theme(&read_file, mode.name())?;
    Theme::from_tokens(mode.name(), &m)
}

fn load_both() -> Result<(&'static Theme, &'static Theme), Vec<String>> {
    let d = load(Mode::Dark)?;
    let l = load(Mode::Light)?;
    // 舊的 Theme 故意不釋放（別的執行緒可能還拿著 &'static）；只有換主題／熱更新才會多一份，量很小
    Ok((Box::leak(Box::new(d)), Box::leak(Box::new(l))))
}

fn publish(s: &State) {
    let p = match s.mode {
        Mode::Dark => s.dark,
        Mode::Light => s.light,
    };
    CUR.store(p as *const Theme as *mut Theme, Ordering::Release);
    GEN.fetch_add(1, Ordering::AcqRel);
    for f in LISTENERS.lock().unwrap().iter() {
        f();
    }
}

#[cold]
fn init() -> &'static Theme {
    let mut g = STATE.lock().unwrap();
    if g.is_none() {
        let (dark, light) = load_both().unwrap_or_else(|e| panic!("設計 token 載入失敗：\n  {}", e.join("\n  ")));
        let mode = if std::env::var("XQ_THEME").as_deref() == Ok("light") { Mode::Light } else { Mode::Dark };
        let s = State { mode, dark, light };
        let p = match mode {
            Mode::Dark => dark,
            Mode::Light => light,
        };
        CUR.store(p as *const Theme as *mut Theme, Ordering::Release);
        *g = Some(s);
    }
    unsafe { &*CUR.load(Ordering::Acquire) }
}

/// 目前主題。熱路徑：一次原子讀取
#[inline]
pub fn t() -> &'static Theme {
    let p = CUR.load(Ordering::Acquire);
    if p.is_null() {
        return init();
    }
    // SAFETY: 指向 Box::leak 出來、永不釋放的 Theme
    unsafe { &*p }
}

/// 每次換主題／熱更新 +1；App 記住上次看到的值，變了就清快取重畫
#[inline]
pub fn generation() -> u64 {
    GEN.load(Ordering::Acquire)
}

pub fn mode() -> Mode {
    t();
    STATE.lock().unwrap().as_ref().map(|s| s.mode).unwrap_or(Mode::Dark)
}

pub fn set_mode(mode: Mode) {
    t();
    let mut g = STATE.lock().unwrap();
    let s = g.as_mut().unwrap();
    if s.mode != mode {
        s.mode = mode;
        publish(s);
    }
}

pub fn toggle_mode() -> Mode {
    let m = if mode() == Mode::Dark { Mode::Light } else { Mode::Dark };
    set_mode(m);
    m
}

/// 重讀 token（XQ_TOKENS_DIR 或內嵌）。失敗時保留舊值並回傳錯誤
pub fn reload() -> Result<(), Vec<String>> {
    t();
    let (dark, light) = load_both()?;
    let mut g = STATE.lock().unwrap();
    let s = g.as_mut().unwrap();
    s.dark = dark;
    s.light = light;
    publish(s);
    Ok(())
}

/// 換主題／熱更新時要呼叫的函式（例如 egui 的 ctx.request_repaint）。可能從背景執行緒呼叫
pub fn on_change(f: impl Fn() + Send + Sync + 'static) {
    LISTENERS.lock().unwrap().push(Box::new(f));
}

/// 熱更新：有 XQ_TOKENS_DIR 且（debug 版或 XQ_TOKENS_WATCH=1）時，開一條背景執行緒每 300ms 看檔案修改時間。
/// 回傳是否有開。release 版預設不開，不影響效能量測。
pub fn watch() -> bool {
    let want = cfg!(debug_assertions) || std::env::var("XQ_TOKENS_WATCH").as_deref() == Ok("1");
    let Some(dir) = disk_dir() else { return false };
    if !want || std::env::var("XQ_TOKENS_WATCH").as_deref() == Ok("0") {
        return false;
    }
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return true;
    }
    t();
    let stamp = move || -> Vec<Option<SystemTime>> {
        EMBEDDED.iter().map(|(n, _)| std::fs::metadata(dir.join(n)).and_then(|m| m.modified()).ok()).collect()
    };
    std::thread::Builder::new()
        .name("xq-tokens-watch".into())
        .spawn(move || {
            let mut last = stamp();
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let now = stamp();
                if now != last {
                    last = now;
                    match reload() {
                        Ok(()) => eprintln!("[xq-tokens] 已重新載入設計 token"),
                        Err(e) => eprintln!("[xq-tokens] token 有錯，保留舊值：\n  {}", e.join("\n  ")),
                    }
                }
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_themes_load() {
        let d = load(Mode::Dark).unwrap();
        let l = load(Mode::Light).unwrap();
        // 深色主題 = 改成 token 之前程式裡寫死的值（畫面不變的保證之一）
        assert_eq!(d.market.up, 0xff4d4f);
        assert_eq!(d.chart.up, 0xe5484d);
        assert_eq!(d.chart.flat, 0x3b82f6); // alias → color.ui.accent
        assert_eq!(d.chart.sub_dmi, vec![0xe5484d, 0x30a46c, 0xf5c542]);
        assert_eq!((d.market.up_flash.rgb, (d.market.up_flash.a * 255.0).round() as u32), (0xff4d4f, 0x22));
        assert_eq!(d.font.ui, "Noto Sans CJK TC");
        assert_eq!(d.layout.table_row, 22.0);
        assert_ne!(l.ui.bg, d.ui.bg);
    }

    #[test]
    fn dtcg_variants() {
        let doc: serde_json::Value = serde_json::from_str(
            r##"{"c":{"$type":"color","a":{"$value":"#ff000080"},"b":{"$value":{"colorSpace":"srgb","components":[0,0.5,1]}},
                 "r":{"$ref":"#/c/a/$value"},"al":{"$value":"{c.b}"}},
                 "d":{"$type":"dimension","x":{"$value":{"value":1,"unit":"rem"}}}}"##,
        )
        .unwrap();
        let mut f = dtcg::Flat::default();
        f.merge_doc(&doc).unwrap();
        let m = f.resolve().unwrap();
        assert_eq!(m["c.a"], Tok::Color(Rgba { rgb: 0xff0000, a: 128.0 / 255.0 }));
        assert_eq!(m["c.b"], Tok::Color(Rgba { rgb: 0x0080ff, a: 1.0 }));
        assert_eq!(m["c.r"], m["c.a"]);
        assert_eq!(m["c.al"], m["c.b"]);
        assert_eq!(m["d.x"], Tok::Px(16.0));
    }
}
