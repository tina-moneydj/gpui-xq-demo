//! 畫線圖層與上下順序（z-order）：純邏輯，不碰繪圖框架。gpui／egui 兩版同一份檔案，規則與 wry `ui/chart.js` 相同。
//!
//! 規則：
//! - `layers` 陣列的位置就是圖層順序（0 = 最底層）。每層有 id、名稱、顯示、鎖定。
//! - 每條畫線記 `layer`（圖層 id，舊存檔沒有這欄 → 0）。同一圖層內，畫線在陣列裡越後面越上面。
//! - 繪製順序＝依（圖層位置, 陣列索引）排序；隱藏圖層的畫線不畫、也點不到；鎖定圖層的畫線會畫，但點不到。
//! - 「圖層」只是資料，不對應任何一張 GPU 貼圖：100 層的成本只跟畫線數有關，跟層數無關。
//! - 上移一層／下移一層／移到最上／移到最下，都在畫線自己的圖層內調整（圖層之間的順序用圖層面板調）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::drawings::Drawing;

/// 圖層數上限（契約：至少 100 層）
pub const MAX_LAYERS: usize = 200;
/// 隱藏圖層的畫線在 rank 裡的值
pub const HIDDEN: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

fn yes() -> bool {
    true
}

impl Layer {
    pub fn new(id: u32) -> Layer {
        Layer { id, name: format!("圖層 {}", id + 1), visible: true, locked: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOp {
    /// 上移一層（Ctrl+]）
    Forward,
    /// 下移一層（Ctrl+[）
    Backward,
    /// 移到最上（Ctrl+Shift+]）
    Front,
    /// 移到最下（Ctrl+Shift+[）
    Back,
}

impl ZOp {
    pub const ALL: [ZOp; 4] = [ZOp::Forward, ZOp::Backward, ZOp::Front, ZOp::Back];
    pub fn label(self) -> &'static str {
        match self {
            ZOp::Forward => "上移一層",
            ZOp::Backward => "下移一層",
            ZOp::Front => "移到最上",
            ZOp::Back => "移到最下",
        }
    }
    pub fn keys(self) -> &'static str {
        match self {
            ZOp::Forward => "Ctrl+]",
            ZOp::Backward => "Ctrl+[",
            ZOp::Front => "Ctrl+Shift+]",
            ZOp::Back => "Ctrl+Shift+[",
        }
    }
}

/// 補齊圖層：沒有圖層 → 建一個預設層；畫線指到不存在的圖層 → 補建（依 id 由小到大疊在最上面）。
pub fn normalize(layers: &mut Vec<Layer>, list: &[Drawing]) {
    if layers.is_empty() {
        layers.push(Layer::new(0));
    }
    let mut missing: Vec<u32> = list.iter().map(|d| d.layer).filter(|id| !layers.iter().any(|l| l.id == *id)).collect();
    missing.sort_unstable();
    missing.dedup();
    for id in missing {
        if layers.len() >= MAX_LAYERS {
            break;
        }
        layers.push(Layer::new(id));
    }
}

/// 繪製順序（快取：畫線內容／圖層／選取變了才重算，O(n + 層數)）
#[derive(Clone, Debug, Default)]
pub struct Order {
    /// 由下到上的畫線索引（不含隱藏圖層）
    pub order: Vec<u32>,
    /// rank[k] = 畫線 k 在 order 裡的位置；隱藏 = HIDDEN
    pub rank: Vec<u32>,
    /// 畫線 k 所在圖層是否鎖定（鎖定：畫得到、點不到）
    pub locked: Vec<bool>,
}

impl Order {
    pub fn build(layers: &[Layer], list: &[Drawing]) -> Order {
        let pos: HashMap<u32, usize> = layers.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
        // 計數排序：每個圖層一個桶，桶內維持陣列順序
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); layers.len().max(1)];
        let mut locked = vec![false; list.len()];
        for (k, d) in list.iter().enumerate() {
            // 不存在的圖層 id（理論上 normalize 過不會有）當成最底層
            let li = pos.get(&d.layer).copied().unwrap_or(0);
            let Some(l) = layers.get(li) else {
                buckets[0].push(k as u32);
                continue;
            };
            if !l.visible {
                continue;
            }
            locked[k] = l.locked;
            buckets[li].push(k as u32);
        }
        let mut order = Vec::with_capacity(list.len());
        for b in buckets {
            order.extend(b);
        }
        let mut rank = vec![HIDDEN; list.len()];
        for (r, &k) in order.iter().enumerate() {
            rank[k as usize] = r as u32;
        }
        Order { order, rank, locked }
    }

    #[inline]
    pub fn hidden(&self, k: usize) -> bool {
        self.rank.get(k).is_none_or(|&r| r == HIDDEN)
    }

    /// 可以點選（顯示中且沒鎖定）
    #[inline]
    pub fn pickable(&self, k: usize) -> bool {
        !self.hidden(k) && !self.locked.get(k).copied().unwrap_or(true)
    }

    /// 以選取的那條切成「下面／它自己／上面」三段（三明治）；沒選取或選取的被隱藏 → 全部在下面
    pub fn split(&self, selected: Option<usize>) -> (&[u32], Option<u32>, &[u32]) {
        match selected.and_then(|k| self.rank.get(k).copied()).filter(|&r| r != HIDDEN) {
            Some(r) => {
                let r = r as usize;
                (&self.order[..r], Some(self.order[r]), &self.order[r + 1..])
            }
            None => (&self.order[..], None, &[]),
        }
    }
}

/// 在畫線自己的圖層內調整上下順序；回傳新的索引（沒動 → None）
pub fn apply_zop(list: &mut Vec<Drawing>, k: usize, op: ZOp) -> Option<usize> {
    let layer = list.get(k)?.layer;
    let same = |i: &usize| list[*i].layer == layer;
    let target = match op {
        ZOp::Forward => (k + 1..list.len()).find(same)?,
        ZOp::Backward => (0..k).rev().find(same)?,
        ZOp::Front => {
            // 已經是同層最上面 → 不動
            (k + 1..list.len()).find(same)?;
            list.len() - 1
        }
        ZOp::Back => {
            (0..k).rev().find(same)?;
            0
        }
    };
    let d = list.remove(k);
    list.insert(target, d);
    Some(target)
}

/// 圖層上移／下移一格（up = 往上疊）；回傳是否有動
pub fn move_layer(layers: &mut [Layer], id: u32, up: bool) -> bool {
    let Some(i) = layers.iter().position(|l| l.id == id) else { return false };
    let j = if up { i + 1 } else { i.wrapping_sub(1) };
    if j >= layers.len() {
        return false;
    }
    layers.swap(i, j);
    true
}

/// 新圖層（疊在最上面）；回傳新 id
pub fn add_layer(layers: &mut Vec<Layer>) -> Option<u32> {
    if layers.len() >= MAX_LAYERS {
        return None;
    }
    let id = layers.iter().map(|l| l.id + 1).max().unwrap_or(0);
    layers.push(Layer::new(id));
    Some(id)
}

/// 壓測：n 層（id 0..n，順序 = id），畫線 k 放在第 k % n 層
pub fn perf_layers(list: &mut [Drawing], n: usize) -> Vec<Layer> {
    let n = n.clamp(1, MAX_LAYERS);
    for (k, d) in list.iter_mut().enumerate() {
        d.layer = (k % n) as u32;
    }
    (0..n as u32).map(Layer::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{Pt, Tool};

    fn d(layer: u32, tag: &str) -> Drawing {
        Drawing { kind: Tool::Text, pts: vec![Pt { i: 0.0, p: 0.0 }], text: tag.into(), layer }
    }
    fn tags(list: &[Drawing], o: &Order) -> Vec<String> {
        o.order.iter().map(|&k| list[k as usize].text.clone()).collect()
    }

    #[test]
    fn order_by_layer_then_index() {
        let mut layers = vec![Layer::new(0), Layer::new(1)];
        let list = vec![d(1, "a"), d(0, "b"), d(1, "c"), d(0, "e")];
        let o = Order::build(&layers, &list);
        assert_eq!(tags(&list, &o), ["b", "e", "a", "c"]);
        // 圖層 1 移到下面
        assert!(move_layer(&mut layers, 1, false));
        let o = Order::build(&layers, &list);
        assert_eq!(tags(&list, &o), ["a", "c", "b", "e"]);
        // 隱藏圖層 0 → 不畫、rank = HIDDEN
        layers[1].visible = false;
        let o = Order::build(&layers, &list);
        assert_eq!(tags(&list, &o), ["a", "c"]);
        assert!(o.hidden(1));
        layers[0].locked = true;
        let o = Order::build(&layers, &list);
        assert!(!o.pickable(0) && !o.hidden(0));
    }

    #[test]
    fn zops_stay_in_layer() {
        let mut list = vec![d(0, "a"), d(1, "x"), d(0, "b"), d(0, "c"), d(1, "y")];
        assert_eq!(apply_zop(&mut list, 0, ZOp::Forward), Some(2)); // a 跳過 x（別層）到 b 後面
        let t: Vec<_> = list.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(t, ["x", "b", "a", "c", "y"]);
        assert_eq!(apply_zop(&mut list, 2, ZOp::Front), Some(4));
        assert_eq!(apply_zop(&mut list, 4, ZOp::Front), None);
        assert_eq!(apply_zop(&mut list, 4, ZOp::Back), Some(0));
        assert_eq!(apply_zop(&mut list, 0, ZOp::Back), None);
        assert_eq!(apply_zop(&mut list, 0, ZOp::Backward), None);
        let layers = vec![Layer::new(0), Layer::new(1)];
        let o = Order::build(&layers, &list);
        assert_eq!(tags(&list, &o), ["a", "b", "c", "x", "y"]);
    }

    #[test]
    fn sandwich_split() {
        let layers = vec![Layer::new(0), Layer::new(1)];
        let list = vec![d(1, "a"), d(0, "b"), d(1, "c")];
        let o = Order::build(&layers, &list);
        let (lo, s, hi) = o.split(Some(0));
        assert_eq!((lo, s, hi), (&[1u32][..], Some(0), &[2u32][..]));
        let (lo, s, hi) = o.split(None);
        assert_eq!((lo.len(), s, hi.len()), (3, None, 0));
    }

    #[test]
    fn json_backward_compatible() {
        // 舊存檔：畫線沒有 layer、沒有 layers
        let list: Vec<Drawing> = serde_json::from_str(r#"[{"type":"trend","pts":[{"i":1,"p":2},{"i":3,"p":4}]}]"#).unwrap();
        assert_eq!(list[0].layer, 0);
        let mut layers = Vec::new();
        normalize(&mut layers, &list);
        assert_eq!(layers.len(), 1);
        // layer 0 不寫出（舊版讀得回去）
        assert!(!serde_json::to_string(&list).unwrap().contains("layer"));
        let l: Layer = serde_json::from_str(r#"{"id":3}"#).unwrap();
        assert!(l.visible && !l.locked);
    }
}
