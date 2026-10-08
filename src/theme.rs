//! 顏色／字型常數。台股慣例：漲紅、跌綠。

use gpui::{rgb, Rgba};

pub const UI_FONT: &str = "Noto Sans CJK TC";
pub const MONO_FONT: &str = "Noto Sans Mono CJK TC";

pub fn bg() -> Rgba { rgb(0x0b0e14) }
pub fn panel() -> Rgba { rgb(0x10141c) }
pub fn header() -> Rgba { rgb(0x171c26) }
pub fn border() -> Rgba { rgb(0x2a3140) }
pub fn text() -> Rgba { rgb(0xd8dee9) }
pub fn dim() -> Rgba { rgb(0x7f8a9e) }
pub fn up() -> Rgba { rgb(0xff4d4f) }
pub fn down() -> Rgba { rgb(0x22c55e) }
pub fn flat() -> Rgba { rgb(0xe5e7eb) }
pub fn accent() -> Rgba { rgb(0x3b82f6) }
pub fn selected() -> Rgba { rgb(0x1e3a5f) }
pub fn grid() -> Rgba { rgb(0x1c2230) }
pub fn cross() -> Rgba { rgb(0x9aa4b2) }
pub fn ma_colors() -> [Rgba; 3] { [rgb(0xf5c542), rgb(0xd17dff), rgb(0x4fc3f7)] }

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
