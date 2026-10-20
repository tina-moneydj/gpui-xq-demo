//! # xq-tape — 成交明細（tick tape）的「純邏輯」層
//!
//! 【範例模組／照這個寫】這個 crate 是團隊的參考範本之一，示範「邏輯和畫面分開」：
//!
//! - 這裡**不依賴 GPUI**，只吃 `xq_feed::Quote`，所以可以在 CI 上不開視窗就跑單元測試、
//!   行為基準測試（`tests/baseline.rs`）和 micro-bench（`benches/tape.rs`）。
//! - 畫面那一半在 `src/tick_tape.rs`（GPUI view），它只負責「把這裡算好的東西畫出來」。
//! - XQNext 與之後的 XQ XT 共用底層：像這種純邏輯 crate 就是「共用底層」，介面可以不同。
//!
//! ## 資料從哪來
//! 行情協議目前沒有逐筆成交封包，只有報價（成交價＋**累計**總量）。所以成交明細用
//! 「總量增加了多少」推導每一筆成交量；方向用「成交價比上一筆高／低」近似外盤／內盤
//! （沒有五檔買賣價，無法真正判斷；等引擎有逐筆成交封包再換掉 [`TapeCore::on_quote`] 就好，
//! UI 不用改——這就是把邏輯切乾淨的好處）。
//!
//! ## 效能重點（為什麼這樣寫）
//! - [`Trade`] 是 `Copy` 的小 struct（沒有 `String`）：每筆成交不配置記憶體。
//! - [`Ring`] 是固定容量環狀緩衝：一開始配置好，之後 push 只覆寫，**記憶體有上限**、不會隨時間長大。
//! - 格式化（[`format_row`]）只在畫面上「看得到的那幾列」才做，不預先替每筆建字串。

use xq_feed::Quote;

/// 成交方向（用成交價升降近似外盤／內盤）。
///
/// C# 對照：這就像 `enum Side { Up, Down, Flat }`，但 Rust 的 enum 可以 `match` 窮舉，
/// 少寫一個分支編譯器就會報錯。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 價格比上一筆高（近似外盤成交）
    Up,
    /// 價格比上一筆低（近似內盤成交）
    Down,
    /// 和上一筆同價，或是當日第一筆
    Flat,
}

/// 一筆成交。刻意設計成 `Copy`：只有數字，沒有 `String`／`Vec`，
/// 複製成本＝幾十 bytes 的 memcpy，可以放心在熱路徑上傳值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trade {
    /// 單調遞增序號（同一個 TapeCore 內），UI 用來判斷「哪些是新的」
    pub seq: u64,
    /// 收到的時間（Unix 毫秒）。協議沒有成交時間，先用收到時間。
    pub ts_ms: u64,
    pub price: f64,
    /// 漲跌（相對昨收），決定顏色：漲紅跌綠
    pub change: f64,
    /// 這筆成交量（總量差）
    pub qty: i64,
    pub side: Side,
}

/// 由「報價流」推導「成交流」的狀態機。只追蹤一個代號（目前顯示的那一檔）。
///
/// 規則（也寫在 `tests/baseline/tick_tape_basic.json`，那是行為規格的正本）：
/// 1. 換代號或第一次看到這檔：只記基準（總量、價格），**不產生**成交（不知道前一筆量）。
/// 2. 總量增加：產生一筆成交，量＝增加量；方向＝和上一筆**成交**價比。
/// 3. 總量不變（只有價格變）：不產生成交，也不更新比較基準。
/// 4. 總量變少（換日或資料更正）：重設基準、`resets += 1`，不產生成交。
#[derive(Debug, Default)]
pub struct TapeCore {
    symbol: String,
    last_volume: Option<i64>,
    last_price: f64,
    next_seq: u64,
    /// 規則 4 發生的次數（給效能／資料品質面板看）
    pub resets: u64,
}

impl TapeCore {
    pub fn new(symbol: &str) -> Self {
        TapeCore { symbol: symbol.to_string(), ..Default::default() }
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// 換代號：清掉基準。序號不歸零，UI 才不會把舊序號誤認成新的。
    pub fn set_symbol(&mut self, symbol: &str) {
        if self.symbol != symbol {
            // 用 clear + push_str 重用既有的 String 容量，不另外配置
            self.symbol.clear();
            self.symbol.push_str(symbol);
            self.last_volume = None;
            self.last_price = 0.0;
        }
    }

    /// 餵一筆報價；是目前代號且總量增加才回傳 `Some(Trade)`。
    ///
    /// 這個函式在**背景行情執行緒**上、每筆報價都會跑，所以必須 O(1)、不配置記憶體。
    /// 參數用 `&Quote`（借用）而不是 `Quote`（取得所有權）：呼叫端之後還要把 Quote 放進 Inbox，
    /// 我們只是看一眼，不需要擁有它，也不需要 clone。
    #[inline]
    pub fn on_quote(&mut self, q: &Quote, ts_ms: u64) -> Option<Trade> {
        if q.symbol != self.symbol {
            return None;
        }
        // `let ... else`：沒有基準就記下基準後提早 return（規則 1）
        let Some(prev_vol) = self.last_volume else {
            self.last_volume = Some(q.volume);
            self.last_price = q.price;
            return None;
        };
        match q.volume.cmp(&prev_vol) {
            std::cmp::Ordering::Equal => None, // 規則 3
            std::cmp::Ordering::Less => {
                // 規則 4
                self.resets += 1;
                self.last_volume = Some(q.volume);
                self.last_price = q.price;
                None
            }
            std::cmp::Ordering::Greater => {
                // 規則 2
                let side = if q.price > self.last_price {
                    Side::Up
                } else if q.price < self.last_price {
                    Side::Down
                } else {
                    Side::Flat
                };
                let t = Trade { seq: self.next_seq, ts_ms, price: q.price, change: q.change, qty: q.volume - prev_vol, side };
                self.next_seq += 1;
                self.last_volume = Some(q.volume);
                self.last_price = q.price;
                Some(t)
            }
        }
    }
}

/// 固定容量環狀緩衝。滿了就覆寫最舊的，`evicted` 記被擠掉幾筆。
///
/// C# 對照：像一個自己寫的 `CircularBuffer<T>`。Rust 標準庫有 `VecDeque`，
/// 但這裡要「容量固定、永遠不 realloc、可以 O(1) 用『第幾新』取值」，自己寫十幾行最清楚。
#[derive(Debug, Clone)]
pub struct Ring<T: Copy> {
    buf: Vec<T>,
    cap: usize,
    /// 下一個要寫的位置
    head: usize,
    len: usize,
    pub evicted: u64,
}

impl<T: Copy> Ring<T> {
    /// 容量至少 1。一開始就 `with_capacity`，之後不會再配置。
    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1);
        Ring { buf: Vec::with_capacity(cap), cap, head: 0, len: 0, evicted: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn push(&mut self, v: T) {
        if self.buf.len() < self.cap {
            // 還沒填滿：Vec 已預留容量，push 不會 realloc
            self.buf.push(v);
        } else {
            self.buf[self.head] = v;
        }
        self.head = (self.head + 1) % self.cap;
        if self.len == self.cap {
            self.evicted += 1;
        } else {
            self.len += 1;
        }
    }

    /// 第 i 新的元素（0 = 最新）。超出範圍回 `None`，不 panic。
    pub fn newest(&self, i: usize) -> Option<T> {
        if i >= self.len {
            return None;
        }
        let ix = (self.head + self.cap - 1 - i) % self.cap;
        self.buf.get(ix).copied()
    }

    /// 由舊到新逐一交給 `f`，然後清空（保留已配置的容量，下次不用再配置）。
    pub fn drain_oldest_first(&mut self, mut f: impl FnMut(T)) {
        for i in (0..self.len).rev() {
            if let Some(v) = self.newest(i) {
                f(v);
            }
        }
        self.clear();
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.head = 0;
        self.len = 0;
    }
}

/// 一列成交明細的顯示文字。只為「看得到的列」建立（見 `src/tick_tape.rs` 的 render_rows）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowText {
    /// `HH:MM:SS.mmm`
    pub time: String,
    /// 成交價，兩位小數
    pub price: String,
    /// 成交量，千分位
    pub qty: String,
    /// 方向：外／內／－
    pub side: &'static str,
    /// 顏色類別：up／down／flat（漲紅跌綠，依漲跌而不是依方向）
    pub color: &'static str,
}

/// 依 `utc_offset_sec`（台北 = 28800）格式化一筆成交。
///
/// 時區當參數傳進來，而不是在這裡讀系統時區：
/// (1) 函式是純的，測試結果不受執行機器影響；(2) 海外版（XQNext）可以傳當地時區。
pub fn format_row(t: &Trade, utc_offset_sec: i64) -> RowText {
    RowText {
        time: fmt_time(t.ts_ms, utc_offset_sec),
        price: format!("{:.2}", t.price),
        qty: fmt_int(t.qty),
        side: match t.side {
            Side::Up => "外",
            Side::Down => "內",
            Side::Flat => "－",
        },
        color: if t.change > 0.0 {
            "up"
        } else if t.change < 0.0 {
            "down"
        } else {
            "flat"
        },
    }
}

/// Unix 毫秒 → `HH:MM:SS.mmm`（加上時區偏移）。
pub fn fmt_time(ts_ms: u64, utc_offset_sec: i64) -> String {
    let local_ms = ts_ms as i64 + utc_offset_sec * 1000;
    let day_ms = local_ms.rem_euclid(86_400_000);
    let (h, m, s, ms) = (day_ms / 3_600_000, day_ms / 60_000 % 60, day_ms / 1000 % 60, day_ms % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// 千分位整數（與 `src/theme.rs::fmt_int` 同規則；純邏輯 crate 不依賴 app，所以各自一份並用測試鎖住）。
pub fn fmt_int(v: i64) -> String {
    let s = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    if v < 0 {
        out.push('-');
    }
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(sym: &str, price: f64, volume: i64) -> Quote {
        Quote { symbol: sym.into(), name: String::new(), price, change: price - 1000.0, volume }
    }

    #[test]
    fn first_quote_only_sets_baseline() {
        let mut c = TapeCore::new("2330");
        assert_eq!(c.on_quote(&q("2330", 1000.0, 100), 0), None);
    }

    #[test]
    fn volume_delta_becomes_trade_with_tick_side() {
        let mut c = TapeCore::new("2330");
        c.on_quote(&q("2330", 1000.0, 100), 0);
        let t = c.on_quote(&q("2330", 1005.0, 130), 1).expect("trade");
        assert_eq!((t.qty, t.side, t.seq), (30, Side::Up, 0));
        let t = c.on_quote(&q("2330", 1000.0, 131), 2).expect("trade");
        assert_eq!((t.qty, t.side, t.seq), (1, Side::Down, 1));
        let t = c.on_quote(&q("2330", 1000.0, 140), 3).expect("trade");
        assert_eq!(t.side, Side::Flat);
    }

    #[test]
    fn other_symbols_and_price_only_changes_are_ignored() {
        let mut c = TapeCore::new("2330");
        c.on_quote(&q("2330", 1000.0, 100), 0);
        assert_eq!(c.on_quote(&q("2317", 200.0, 999), 1), None);
        assert_eq!(c.on_quote(&q("2330", 1010.0, 100), 2), None);
        // 規則 3：價格變但量不變，不更新比較基準 → 下一筆仍跟 1000 比
        let t = c.on_quote(&q("2330", 1005.0, 101), 3).expect("trade");
        assert_eq!(t.side, Side::Up);
    }

    #[test]
    fn volume_drop_resets_baseline() {
        let mut c = TapeCore::new("2330");
        c.on_quote(&q("2330", 1000.0, 500), 0);
        assert_eq!(c.on_quote(&q("2330", 1000.0, 10), 1), None);
        assert_eq!(c.resets, 1);
        assert_eq!(c.on_quote(&q("2330", 1000.0, 12), 2).map(|t| t.qty), Some(2));
    }

    #[test]
    fn switching_symbol_clears_baseline_but_keeps_seq() {
        let mut c = TapeCore::new("2330");
        c.on_quote(&q("2330", 1000.0, 1), 0);
        c.on_quote(&q("2330", 1000.0, 2), 0);
        c.set_symbol("2317");
        assert_eq!(c.on_quote(&q("2317", 100.0, 50), 0), None);
        assert_eq!(c.on_quote(&q("2317", 100.0, 51), 0).map(|t| t.seq), Some(1));
    }

    #[test]
    fn ring_overwrites_oldest_and_counts_evictions() {
        let mut r = Ring::with_capacity(3);
        for i in 0..5 {
            r.push(i);
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.evicted, 2);
        assert_eq!((r.newest(0), r.newest(2), r.newest(3)), (Some(4), Some(2), None));
        let mut seen = Vec::new();
        r.drain_oldest_first(|v| seen.push(v));
        assert_eq!(seen, vec![2, 3, 4]);
        assert!(r.is_empty());
    }

    #[test]
    fn ring_never_reallocates_after_creation() {
        let mut r = Ring::with_capacity(1000);
        let cap_before = r.buf.capacity();
        for i in 0..10_000u32 {
            r.push(i);
        }
        assert_eq!(r.buf.capacity(), cap_before);
    }

    #[test]
    fn time_format_uses_given_offset() {
        // 2026-10-20 01:00:01.250 UTC = 台北 09:00:01.250
        let ts = 1_792_458_001_250;
        assert_eq!(fmt_time(ts, 8 * 3600), "09:00:01.250");
        assert_eq!(fmt_time(ts, 0), "01:00:01.250");
    }

    #[test]
    fn int_format_matches_app_theme() {
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(1234567), "1,234,567");
        assert_eq!(fmt_int(-1000), "-1,000");
    }
}
