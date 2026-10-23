//! 顏色／字型：全部來自共用設計 token（crates/xq-tokens，W3C DTCG JSON），不再寫死。台股慣例：漲紅、跌綠。
//! 函式名稱保持不變，呼叫端不用改；換主題／熱更新後下一次呼叫就拿到新值。

use gpui::{rgb, Rgba};

pub use xq_tokens::t as tk;

/// 0xRRGGBB ＋ alpha（0..1，token 裡的 alpha）→ gpui Rgba；alpha 先量化成 8 bit，跟原本 rgba(0xRRGGBBAA) 寫法逐位元相同
pub fn rgba_tok(c: xq_tokens::Rgba) -> Rgba {
    gpui::rgba((c.rgb << 8) | (c.a.clamp(0.0, 1.0) * 255.0).round() as u32)
}

pub fn ui_font() -> &'static str { tk().font.ui }
pub fn mono_font() -> &'static str { tk().font.mono }

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
