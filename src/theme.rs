//! 顏色／字型：全部來自共用設計 token（crates/xq-tokens，W3C DTCG JSON），不再寫死。台股慣例：漲紅、跌綠。
//! 函式名稱保持不變，呼叫端不用改；換主題／熱更新後下一次呼叫就拿到新值。

use gpui::{rgb, Rgba};

pub use xq_tokens::t as tk;

/// 0xRRGGBB ＋ alpha（0..1，token 裡的 alpha）→ gpui Rgba；alpha 先量化成 8 bit，跟原本 rgba(0xRRGGBBAA) 寫法逐位元相同
pub fn rgba_tok(c: xq_tokens::Rgba) -> Rgba {
    gpui::rgba((c.rgb << 8) | (c.a.clamp(0.0, 1.0) * 255.0).round() as u32)
}

#[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
pub fn ui_font() -> &'static str { font_sub(tk().font.ui, 0) }
#[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios", windows)))]
pub fn mono_font() -> &'static str { font_sub(tk().font.mono, 1) }

/// 字型替代（2026-10-25）：token 的 Noto Sans CJK 不是每台都有（Windows 預設沒有）。找不到時 GPUI 每排一段字都會
/// 重新建錯誤物件、再沿後援清單（.ZedMono → .ZedSans → Helvetica → Segoe UI…）逐一找，報價表一幀約 200 段字；
/// 有設 RUST_BACKTRACE 時每個錯誤還抓一次 backtrace（Linux 實測主執行緒約 7 成時間在這，5 萬列捲動掉到 25 FPS）。
/// 啟動時查一次，找不到就直接用 GPUI 本來就會退到的那個字型（畫面不變，只是不再每次走錯誤路徑）。
/// 內容：[(token 名稱, 實際用的名稱); ui, mono]。token 熱更新換了名稱就不套用（回傳新名稱）。
#[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
static FONT_SUB: std::sync::OnceLock<[(&'static str, &'static str); 2]> = std::sync::OnceLock::new();

#[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
fn font_sub(name: &'static str, k: usize) -> &'static str {
    match FONT_SUB.get().and_then(|s| s.get(k)) {
        Some(&(orig, sub)) if orig == name => sub,
        _ => name,
    }
}

/// 蘋果系統與 Windows 用內建字型，不需要替代
#[cfg(any(target_os = "macos", target_os = "ios", windows))]
pub fn resolve_fonts(_cx: &gpui::App) {}

/// 啟動時（開視窗前）呼叫一次。每個名稱只查一次字型表，不列舉系統字型。
#[cfg(not(any(target_os = "macos", target_os = "ios", windows)))]
pub fn resolve_fonts(cx: &gpui::App) {
    let ts = cx.text_system();
    let resolved = |name: &'static str| -> &'static str {
        let fam = ts.get_font_for_id(ts.resolve_font(&gpui::font(name))).map(|f| f.family);
        match fam {
            Some(f) if f.as_ref() != name => {
                eprintln!("[fonts] 這台沒有「{name}」，改用 GPUI 後援字型「{f}」");
                // 啟動時最多兩個名稱，留到程式結束
                String::from(f.as_ref()).leak()
            }
            _ => name,
        }
    };
    let t = tk();
    let _ = FONT_SUB.set([(t.font.ui, resolved(t.font.ui)), (t.font.mono, resolved(t.font.mono))]);
}
/// Windows：系統內建的微軟正黑體；等寬用 Consolas（中文字由 DirectWrite 字型後援補上）。token 的 Noto Sans CJK 在 Windows 上沒有內建。
#[cfg(windows)]
pub fn ui_font() -> &'static str { "Microsoft JhengHei UI" }
#[cfg(windows)]
pub fn mono_font() -> &'static str { "Consolas" }
/// macOS／iOS：系統內建的蘋方-繁（PingFang TC）；等寬用 Menlo（中文字由 CoreText 字型後援補上）。
/// token 的 Noto Sans CJK 在蘋果系統上沒有內建。字型由 CoreText 依名稱按需開啟（檔案映射、系統共用），不整檔讀進記憶體。
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn ui_font() -> &'static str { "PingFang TC" }
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn mono_font() -> &'static str { "Menlo" }
/// Android 系統字型沒有 Mono CJK：等寬字也用介面字型（Noto Sans CJK TC，egui 手機版同樣做法）
#[cfg(target_os = "android")]
pub fn mono_font() -> &'static str { ui_font() }

pub fn bg() -> Rgba { rgb(tk().ui.bg) }
pub fn panel() -> Rgba { rgb(tk().ui.panel) }
pub fn header() -> Rgba { rgb(tk().ui.header) }
pub fn border() -> Rgba { rgb(tk().ui.border) }
pub fn text() -> Rgba { rgb(tk().ui.text) }
pub fn dim() -> Rgba { rgb(tk().ui.text_dim) }
pub fn up() -> Rgba { rgb(tk().market.up) }
pub fn down() -> Rgba { rgb(tk().market.down) }
pub fn flat() -> Rgba { rgb(tk().market.flat) }
pub fn accent() -> Rgba { rgb(tk().ui.accent) }
pub fn selected() -> Rgba { rgb(tk().ui.selected) }

pub fn dir_color(change: f64) -> Rgba {
    if change > 0.0 {
        up()
    } else if change < 0.0 {
        down()
    } else {
        flat()
    }
}

pub fn fmt_int(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    if v < 0 {
        out.push('-');
    }
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn fmt_change(change: f64) -> String {
    if change > 0.0 {
        format!("▲{:.2}", change)
    } else if change < 0.0 {
        format!("▼{:.2}", -change)
    } else {
        "0.00".into()
    }
}

pub fn pct(price: f64, change: f64) -> f64 {
    let prev = price - change;
    if prev.abs() < 1e-9 { 0.0 } else { change / prev * 100.0 }
}
