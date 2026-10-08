//! 存檔：每個「代號|週期」各自的指標設定與畫線，寫到 ~/.local/share/gpui-xq-demo/state.json。
//! 新代號沿用上一次的指標設定（與 wry 版 lastCfg 相同）。壓測模式不讀也不寫。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::drawings::Drawing;
use crate::model::IndCfg;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub cfg: Option<IndCfg>,
    #[serde(default)]
    pub drawings: Vec<Drawing>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub v: u32,
    #[serde(default)]
    pub keys: HashMap<String, Entry>,
    #[serde(default)]
    pub last_cfg: Option<IndCfg>,
}

pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("XQ_GPUI_STATE") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("gpui-xq-demo").join("state.json"))
}

impl Store {
    pub fn load() -> Store {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 背景執行緒寫檔（先寫暫存檔再 rename，避免寫一半）
    pub fn save_async(&self) {
        let Some(p) = path() else { return };
        let Ok(json) = serde_json::to_string(self) else { return };
        std::thread::spawn(move || {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        });
    }
}
