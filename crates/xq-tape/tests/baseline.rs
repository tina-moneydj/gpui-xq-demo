//! 行為基準測試（behaviour baseline）範例。
//!
//! 【照這個寫】每個要對齊 WPF 版的功能，都把「輸入序列 → 預期畫面文字」錄成 JSON 放在
//! `tests/baseline/*.json`，這支測試逐步重播、逐格比對。好處：
//! - 「跟舊版不一樣」從口頭爭論變成一行 diff：哪一步、哪一格、預期什麼、實際什麼。
//! - 刻意的差異寫在 JSON 的 `known_diffs`（分類：刻意／bug／海外版差異），QA 不用每次重吵。
//! - 要改行為時：先改 JSON（PR 標題標 `[baseline]`，說明分類），再改程式讓測試過。
//!
//! 跑：`cargo test -p xq-tape --test baseline`

use std::path::Path;

use serde_json::Value;
use xq_feed::Quote;
use xq_tape::{format_row, TapeCore};

fn run_case(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("讀不到 {}: {e}", path.display()));
    let case: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON: {e}", path.display()));
    let offset = case["utc_offset_sec"].as_i64().unwrap_or(28_800);
    let mut core = TapeCore::new(case["symbol"].as_str().unwrap_or_default());
    let mut failures = Vec::new();

    let steps = case["steps"].as_array().map(Vec::as_slice).unwrap_or_default();
    for (i, s) in steps.iter().enumerate() {
        let q = Quote {
            symbol: s["symbol"].as_str().unwrap_or_default().to_string(),
            name: String::new(),
            price: s["price"].as_f64().unwrap_or_default(),
            change: s["change"].as_f64().unwrap_or_default(),
            volume: s["volume"].as_i64().unwrap_or_default(),
        };
        let got = core.on_quote(&q, s["ts_ms"].as_u64().unwrap_or_default()).map(|t| format_row(&t, offset));
        let got_json = match &got {
            None => Value::Null,
            Some(r) => serde_json::json!({
                "time": r.time, "price": r.price, "qty": r.qty, "side": r.side, "color": r.color
            }),
        };
        if got_json != s["expect"] {
            failures.push(format!(
                "{} 第 {i} 步（{}）\n  預期: {}\n  實際: {}",
                case["id"].as_str().unwrap_or("?"),
                s["note"].as_str().unwrap_or(""),
                s["expect"],
                got_json
            ));
        }
    }
    failures
}

#[test]
fn tick_tape_matches_recorded_baselines() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/baseline");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("baseline 目錄")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "{} 底下沒有基準檔", dir.display());

    let failures: Vec<String> = files.iter().flat_map(|p| run_case(p)).collect();
    assert!(failures.is_empty(), "行為基準不符（{} 處）：\n{}", failures.len(), failures.join("\n"));
}
