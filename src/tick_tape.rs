//! 成交明細面板（tick tape）—— 團隊的「照這個寫」範例模組。
//!
//! 開法：`XQ_TAPE=1 ./target/release/gpui-xq-demo`（右下多一欄；預設關閉，壓測版面不變）。
//! 純邏輯（推導成交、環狀緩衝、格式化）在 `crates/xq-tape`，有單元測試、行為基準、micro-bench；
//! 這個檔案只做 GPUI 的部分。一個面板通常就是這兩半：**純邏輯 crate ＋ 一個 view 檔**。
//!
//! 這個檔案依序示範（每段都有【模式】標記，新面板照抄結構）：
//! 1. 【模式 A：背景執行緒 → UI】`TapeFeed` 在行情執行緒上 O(1) 累積，UI 每幀最多取一次
//! 2. 【模式 B：Entity 狀態】`TickTape` 是一個 Entity，狀態改了才 `cx.notify()`
//! 3. 【模式 C：Render＋虛擬化】`uniform_list` 只建可見列，字串只為可見列格式化
//! 4. 【模式 D：Action＋快捷鍵＋焦點】`actions!`、`KeyBinding`、`key_context`、`track_focus`
//! 5. 【模式 E：存檔不在 UI 執行緒】讀檔／寫檔都丟到 `background_executor`，寫檔前防抖 450 ms
//!
//! 和報價表（`src/table.rs`）的差別：報價表是「同代號只留最新」（合併 OK）；成交明細是
//! 「每一筆都要」（不能合併），所以這裡用**固定容量的環狀緩衝**當背景佇列：
//! UI 跟不上時最舊的被擠掉並計數（`dropped`），記憶體永遠有上限，絕不會越積越多。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    actions, div, prelude::*, px, uniform_list, App, Context, FocusHandle, Focusable, KeyBinding, MouseButton,
    ScrollStrategy, SharedString, UniformListScrollHandle, Window,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use xq_feed::Quote;
use xq_tape::{format_row, Ring, TapeCore, Trade};

use crate::theme::*;

/// 畫面最多保留幾筆（≈ 5000 × 48 bytes ≈ 240 KB，固定不長大）
pub const TAPE_CAP: usize = 5000;
/// 背景佇列容量：UI 一幀（16 ms）內最多收幾筆；超過代表 UI 嚴重落後，擠掉最舊的並計數
pub const PENDING_CAP: usize = 4096;
/// 捲動位置離最上面超過這麼多像素，就算「正在看舊資料」，暫停跟隨
const FOLLOW_EPS: f32 = 1.0;

/// 新成交插在最上面時，該把捲動位置調到哪裡（純函式，好測）。
/// `offset_y`：GPUI 的捲動位移，往下捲是負值；0 代表停在最上面（跟隨最新）。
/// 停在最上面 → 不動（照常跟隨）；已經往下捲 → 再往下推 `added` 列，畫面內容不動。
/// 推過頭不用管：uniform_list 排版時會夾在最大捲動範圍內。
fn anchored_offset(offset_y: f32, added: usize, row_h: f32) -> f32 {
    if offset_y >= -FOLLOW_EPS || added == 0 {
        offset_y
    } else {
        offset_y - added as f32 * row_h
    }
}
fn row_h() -> f32 { xq_tokens::t().layout.tape_row }
const COLS: [(&str, f32); 4] = [("時間", 96.0), ("成交", 72.0), ("單量", 64.0), ("", 28.0)];

// ───────────────── 【模式 A】背景執行緒 → UI ─────────────────
//
// C# 對照：WPF 常見寫法是背景執行緒 `Dispatcher.BeginInvoke(() => list.Add(x))`，每筆一次 → 高頻時
// UI 佇列塞爆。這裡反過來：背景只把資料放進共享緩衝（鎖住的時間極短），UI 每幀「拉」一次整批。

/// 背景執行緒與 UI 共用的狀態。用 `parking_lot::Mutex`（比 std 的輕、不會 poison）。
pub struct TapeShared {
    core: TapeCore,
    pending: Ring<Trade>,
}

/// `Arc` = 跨執行緒共用的參考計數指標（C# 的物件參考本來就共用；Rust 要明講）。
/// `Clone` 只是把計數 +1，不複製內容。
#[derive(Clone)]
pub struct TapeFeed(Arc<Mutex<TapeShared>>);

impl TapeFeed {
    pub fn new(symbol: &str) -> Self {
        TapeFeed(Arc::new(Mutex::new(TapeShared { core: TapeCore::new(symbol), pending: Ring::with_capacity(PENDING_CAP) })))
    }

    /// **在行情執行緒上**被 `bridge::merge` 呼叫，每個報價封包一次。
    /// 規則：只做 O(1)／筆的事、不配置記憶體、不碰任何 UI 物件、不做 I/O、不 log。
    pub fn on_quotes(&self, quotes: &[Quote]) {
        let now = crate::plat::unix_us() / 1000;
        let mut s = self.0.lock(); // 鎖一次處理整包，不要每筆鎖一次
        let TapeShared { core, pending } = &mut *s; // 解構成兩個可變借用（不同欄位可同時借）
        for q in quotes {
            if let Some(t) = core.on_quote(q, now) {
                pending.push(t);
            }
        }
    }

    fn set_symbol(&self, symbol: &str) {
        let mut s = self.0.lock();
        s.core.set_symbol(symbol);
        s.pending.clear();
    }
}

// ───────────────── 【模式 D】Action 與快捷鍵 ─────────────────
//
// C# 對照：WPF 的 `RoutedCommand` + `InputBindings`。GPUI 的 action 是一個型別，
// 快捷鍵綁在「key context」上：只有焦點在這個面板裡時，按 `b` 才會觸發這裡的 ToggleBigOnly，
// 不會跟走勢圖或報價表的同一個鍵打架。要對齊 WPF 版快捷鍵，就在 `bind_keys` 改這張表。

actions!(tick_tape, [ToggleBigOnly, JumpToLatest, RaiseBigQty, LowerBigQty]);

pub const KEY_CONTEXT: &str = "TickTape";

/// 在 `main()` 開視窗前呼叫一次。
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("b", ToggleBigOnly, Some(KEY_CONTEXT)),
        KeyBinding::new("home", JumpToLatest, Some(KEY_CONTEXT)),
        KeyBinding::new("]", RaiseBigQty, Some(KEY_CONTEXT)),
        KeyBinding::new("[", LowerBigQty, Some(KEY_CONTEXT)),
    ]);
}

// ───────────────── 【模式 E】設定與存檔 ─────────────────

/// 要存檔的設定。`#[serde(default)]`：舊檔少欄位也讀得進來（版本相容）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TapeSettings {
    /// 只看大單
    pub big_only: bool,
    /// 大單門檻（張）
    pub big_qty: i64,
}

impl Default for TapeSettings {
    fn default() -> Self {
        TapeSettings { big_only: false, big_qty: 50 }
    }
}

impl TapeSettings {
    /// 門檻階梯：1 → 5 → 10 → 20 → 50 → 100 → 200 → 500 → 1000（純函式，好測）
    pub fn step_big_qty(&self, up: bool) -> i64 {
        const STEPS: [i64; 9] = [1, 5, 10, 20, 50, 100, 200, 500, 1000];
        let i = STEPS.iter().position(|&s| s >= self.big_qty).unwrap_or(STEPS.len() - 1);
        let j = if up { (i + 1).min(STEPS.len() - 1) } else { i.saturating_sub(1) };
        STEPS[j]
    }
}

fn settings_path() -> Option<PathBuf> {
    if let Some(p) = crate::plat::var_os("XQ_TAPE_STATE") {
        return Some(PathBuf::from(p));
    }
    let base = crate::data_dir()?;
    Some(base.join("gpui-xq-demo").join("tick_tape.json"))
}

/// 讀檔失敗（沒檔案、壞檔）一律回預設值，不 panic、不擋 UI。
fn load_settings(path: Option<PathBuf>) -> TapeSettings {
    path.and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 先寫暫存檔再 rename：寫到一半當機也不會留下壞檔。錯誤只記 log，不往上丟到 UI。
fn save_settings(path: PathBuf, json: String) {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = result {
        eprintln!("[tick_tape] 設定存檔失敗 {}: {e}", path.display());
    }
}

// ───────────────── 【模式 B】Entity 狀態 ─────────────────
//
// C# 對照：這就是 ViewModel ＋ View 合在一起。沒有 INotifyPropertyChanged／binding：
// 改完欄位呼叫 `cx.notify()`，GPUI 就會在下一幀重新呼叫 `render`。沒呼叫就不會重畫（省 CPU）。

pub struct TickTape {
    feed: TapeFeed,
    /// 全部成交（最新 TAPE_CAP 筆）
    all: Ring<Trade>,
    /// 大單（同上，另存一份讓「只看大單」切換是 O(1)）
    big: Ring<Trade>,
    /// 雙緩衝：和 `TapeShared::pending` 交換，背景與 UI 都不用每幀重新配置
    spare: Ring<Trade>,
    symbol: SharedString,
    settings: TapeSettings,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// 時區偏移（秒）。台灣版 +8；海外版可由設定傳入（見 baseline 的 TT-D2）
    utc_offset_sec: i64,
    persist: bool,
    save_gen: u64,
    /// 統計：UI 收到幾筆、背景因 UI 落後擠掉幾筆（應該永遠是 0）
    pub applied: u64,
    pub dropped: u64,
}

impl TickTape {
    pub fn new(feed: TapeFeed, symbol: &str, cx: &mut Context<Self>) -> Self {
        // 壓測模式不讀也不寫設定（跟 store.rs 同規則），結果才可重現
        let persist = !["XQ_BENCH", "XQ_TICK_BENCH", "XQ_CHART_STRESS", "XQ_GROUP_STRESS"]
            .iter()
            .any(|k| crate::plat::var_os(k).is_some_and(|v| v != "0"));
        if persist {
            // 【模式 E】讀檔也不在 UI 執行緒：先用預設值畫出來，讀完再更新
            let path = settings_path();
            cx.spawn(async move |this, cx| {
                let loaded = cx.background_executor().spawn(async move { load_settings(path) }).await;
                // `this` 是 WeakEntity：面板若已關閉，update 回 Err，安靜結束即可
                let _ = this.update(cx, |t, cx| t.set_settings(loaded, false, cx));
            })
            .detach();
        }
        TickTape {
            feed,
            all: Ring::with_capacity(TAPE_CAP),
            big: Ring::with_capacity(TAPE_CAP),
            spare: Ring::with_capacity(PENDING_CAP),
            symbol: SharedString::from(symbol.to_string()),
            settings: TapeSettings::default(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            utc_offset_sec: 8 * 3600,
            persist,
            save_gen: 0,
            applied: 0,
            dropped: 0,
        }
    }

    /// 由 `Terminal::apply_feed` 每次喚醒呼叫（≤ 每幀一次，和報價表同一個節奏，不自己另開計時器）。
    /// 回傳這次新增幾筆；0 筆就不 notify、不重畫。
    pub fn apply(&mut self, cx: &mut Context<Self>) -> usize {
        {
            let mut s = self.feed.0.lock();
            if s.pending.is_empty() {
                return 0;
            }
            // 鎖裡只做一次指標交換（O(1)），真正的處理在鎖外
            std::mem::swap(&mut s.pending, &mut self.spare);
        }
        let n = self.spare.len();
        self.dropped += std::mem::take(&mut self.spare.evicted);
        let big_qty = self.settings.big_qty;
        // closure 只借用 self.all / self.big 兩個欄位，和 self.spare 不衝突（Rust 2021 分欄位捕獲）
        let (all, big) = (&mut self.all, &mut self.big);
        let mut big_added = 0usize;
        self.spare.drain_oldest_first(|t| {
            all.push(t);
            if t.qty >= big_qty {
                big.push(t);
                big_added += 1;
            }
        });
        // 捲動時暫停跟隨：使用者往下捲去看舊成交時，新成交插在最上面（index 0），
        // 把捲動位置往下推同樣的列數，畫面上的那幾筆就不會被推走。停在最上面時照常跟隨最新。
        let added = if self.settings.big_only { big_added } else { n };
        {
            let state = self.scroll.0.borrow();
            let off = state.base_handle.offset();
            let new_y = anchored_offset(f32::from(off.y), added, row_h());
            if new_y != f32::from(off.y) {
                state.base_handle.set_offset(gpui::point(off.x, px(new_y)));
            }
        }
        self.applied += n as u64;
        cx.notify();
        n
    }

    /// 使用者是否離開最上面（正在看舊成交）。footer 顯示「已暫停跟隨」用。
    fn following_paused(&self) -> bool {
        f32::from(self.scroll.0.borrow().base_handle.offset().y) < -FOLLOW_EPS
    }

    pub fn set_symbol(&mut self, symbol: &str, cx: &mut Context<Self>) {
        if self.symbol.as_ref() == symbol {
            return;
        }
        self.feed.set_symbol(symbol);
        self.symbol = SharedString::from(symbol.to_string());
        self.all.clear();
        self.big.clear();
        cx.notify();
    }

    fn rows(&self) -> &Ring<Trade> {
        if self.settings.big_only { &self.big } else { &self.all }
    }

    fn set_settings(&mut self, s: TapeSettings, save: bool, cx: &mut Context<Self>) {
        if s == self.settings {
            return;
        }
        let rebuild = s.big_qty != self.settings.big_qty;
        self.settings = s;
        if rebuild {
            // 門檻改了才重建大單清單：O(TAPE_CAP)，只在使用者操作時發生，不在 tick 路徑上
            self.big.clear();
            for i in (0..self.all.len()).rev() {
                if let Some(t) = self.all.newest(i).filter(|t| t.qty >= self.settings.big_qty) {
                    self.big.push(t);
                }
            }
        }
        if save {
            self.schedule_save(cx);
        }
        cx.notify();
    }

    /// 【模式 E】防抖存檔：連按快捷鍵只存最後一次。序列化在 UI 執行緒（小物件，µs 級），
    /// 寫檔丟到背景。大物件（如上千條畫線）連序列化都應該移到背景。
    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if !self.persist {
            return;
        }
        self.save_gen += 1;
        let gen = self.save_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(450)).await;
            let Ok(Some(job)) = this.update(cx, |t, _| {
                // 450 ms 內又改過 → 這次不用存
                (t.save_gen == gen).then(|| (settings_path(), serde_json::to_string(&t.settings)))
            }) else {
                return;
            };
            if let (Some(path), Ok(json)) = job {
                cx.background_executor().spawn(async move { save_settings(path, json) }).await;
            }
        })
        .detach();
    }

    // ── Action handlers：簽名固定是 (&mut self, &Action, &mut Window, &mut Context<Self>) ──

    fn toggle_big_only(&mut self, _: &ToggleBigOnly, _: &mut Window, cx: &mut Context<Self>) {
        let s = TapeSettings { big_only: !self.settings.big_only, ..self.settings.clone() };
        self.set_settings(s, true, cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn jump_to_latest(&mut self, _: &JumpToLatest, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn raise_big_qty(&mut self, _: &RaiseBigQty, _: &mut Window, cx: &mut Context<Self>) {
        let s = TapeSettings { big_qty: self.settings.step_big_qty(true), ..self.settings.clone() };
        self.set_settings(s, true, cx);
    }

    fn lower_big_qty(&mut self, _: &LowerBigQty, _: &mut Window, cx: &mut Context<Self>) {
        let s = TapeSettings { big_qty: self.settings.step_big_qty(false), ..self.settings.clone() };
        self.set_settings(s, true, cx);
    }

    // ───────────────── 【模式 C】虛擬化：只建可見列 ─────────────────

    /// uniform_list 只會用「看得到的範圍」呼叫這裡（約 10–30 列），5000 筆也只格式化這幾列。
    /// 這裡每幀會跑：不要在這裡做排序、過濾、查表以外的重活。
    fn render_rows(&mut self, range: std::ops::Range<usize>, _cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let rows = self.rows();
        let mut out = Vec::with_capacity(range.len());
        for ix in range {
            let Some(t) = rows.newest(ix) else { continue };
            let r = format_row(&t, self.utc_offset_sec);
            let color = match r.color {
                "up" => up(),
                "down" => down(),
                _ => flat(),
            };
            let big = t.qty >= self.settings.big_qty;
            out.push(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(px(row_h()))
                    .w_full()
                    .font_family(mono_font())
                    .when(big, |d| d.bg(crate::theme::rgba_tok(tk().market.big_trade_bg)))
                    .child(cell(COLS[0].1, false).text_color(dim()).child(r.time))
                    .child(cell(COLS[1].1, true).text_color(color).child(r.price))
                    .child(cell(COLS[2].1, true).text_color(if big { gpui::rgb(tk().market.volume) } else { text() }).child(r.qty))
                    .child(cell(COLS[3].1, true).text_color(color).child(r.side))
                    .into_any_element(),
            );
        }
        out
    }
}

fn cell(w: f32, right: bool) -> gpui::Div {
    let d = div().w(px(w)).px_1().overflow_hidden().whitespace_nowrap();
    if right { d.flex().justify_end() } else { d }
}

/// 讓別人可以 `window.focus(&tape.focus_handle(cx))`（例如從選單「跳到成交明細」）。
impl Focusable for TickTape {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TickTape {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let count = self.rows().len();
        let s = &self.settings;
        let filter_label = format!("大單 ≥ {}", fmt_int(s.big_qty));
        div()
            .id("tick-tape")
            // 【模式 D】key_context + track_focus：焦點在這塊時，bind_keys 那張表才生效
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_big_only))
            .on_action(cx.listener(Self::jump_to_latest))
            .on_action(cx.listener(Self::raise_big_qty))
            .on_action(cx.listener(Self::lower_big_qty))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                window.focus(&this.focus, cx);
                cx.notify(); // 外框顏色要變
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(panel())
            .border_1()
            .border_color(if focused { accent() } else { border() })
            .text_size(px(tk().font.md))
            .text_color(text())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(tk().layout.table_header))
                    .px_2()
                    .bg(header())
                    .border_b_1()
                    .border_color(border())
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child("成交明細")
                            .child(div().text_color(dim()).child(self.symbol.clone()))
                            .child(div().text_color(dim()).child(format!("{} 筆", fmt_int(count as i64)))),
                    )
                    .child(
                        div()
                            .id("tape-big")
                            .px_2()
                            .rounded(px(tk().radius.sm))
                            .cursor_pointer()
                            .border_1()
                            .border_color(if s.big_only { accent() } else { border() })
                            .text_color(if s.big_only { accent() } else { dim() })
                            .hover(|st| st.bg(selected()))
                            // 滑鼠和鍵盤走同一個 action handler，行為只寫一份
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_big_only(&ToggleBigOnly, window, cx)))
                            .child(filter_label),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(tk().layout.tape_col_header))
                    .items_center()
                    .text_color(dim())
                    .border_b_1()
                    .border_color(border())
                    .children(COLS.iter().enumerate().map(|(i, (label, w))| cell(*w, i >= 1).child(*label))),
            )
            .child(
                uniform_list("tape-rows", count, cx.processor(|this, range, _window, cx| this.render_rows(range, cx)))
                    .track_scroll(&self.scroll)
                    .flex_1(),
            )
            .child(
                div()
                    .h(px(tk().layout.tape_footer))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_size(px(tk().font.xs))
                    .text_color(dim())
                    .child(if self.following_paused() {
                        "已暫停跟隨最新（正在看舊成交）· Home 回最新"
                    } else if focused {
                        "b 只看大單 · [ ] 門檻 · Home 回最新"
                    } else {
                        "點一下面板啟用快捷鍵"
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 測試用固定列高（預設 token layout.tape_row 也是 20）
    const ROW_H: f32 = 20.0;

    #[test]
    fn scroll_anchor_follows_at_top_and_holds_when_scrolled() {
        // 停在最上面：照常跟隨，新成交直接出現在頂端
        assert_eq!(anchored_offset(0.0, 5, ROW_H), 0.0);
        assert_eq!(anchored_offset(-0.5, 5, ROW_H), -0.5);
        // 往下捲了 3 列：進來 5 筆 → 再往下推 5 列，看到的還是同一批舊成交
        assert_eq!(anchored_offset(-60.0, 5, ROW_H), -160.0);
        // 沒有新成交（例如只看大單時進來的都是小單）：不動
        assert_eq!(anchored_offset(-60.0, 0, ROW_H), -60.0);
    }

    #[test]
    fn big_qty_steps_clamp_at_both_ends() {
        let s = |q| TapeSettings { big_only: false, big_qty: q };
        assert_eq!(s(50).step_big_qty(true), 100);
        assert_eq!(s(50).step_big_qty(false), 20);
        assert_eq!(s(1).step_big_qty(false), 1);
        assert_eq!(s(1000).step_big_qty(true), 1000);
        // 不在階梯上的值（舊檔手改）→ 往上對齊到下一階
        assert_eq!(s(30).step_big_qty(true), 100);
    }

    #[test]
    fn settings_tolerate_missing_fields_and_bad_files() {
        let s: TapeSettings = serde_json::from_str(r#"{"big_only":true}"#).expect("舊格式也要讀得進來");
        assert_eq!(s, TapeSettings { big_only: true, big_qty: 50 });
        let dir = std::env::temp_dir().join(format!("xq-tape-test-{}", std::process::id()));
        let p = dir.join("tick_tape.json");
        save_settings(p.clone(), "{ 壞掉的 json".into());
        assert_eq!(load_settings(Some(p.clone())), TapeSettings::default());
        save_settings(p.clone(), serde_json::to_string(&TapeSettings { big_only: true, big_qty: 200 }).unwrap_or_default());
        assert_eq!(load_settings(Some(p)), TapeSettings { big_only: true, big_qty: 200 });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn feed_buffers_only_current_symbol_and_caps_memory() {
        let feed = TapeFeed::new("2330");
        let q = |sym: &str, v| Quote { symbol: sym.into(), name: String::new(), price: 1000.0, change: 0.0, volume: v };
        // 第一筆是基準；之後每筆量 +1 → 一筆成交
        let quotes: Vec<Quote> = (0..(PENDING_CAP as i64 + 11)).map(|v| q("2330", v)).collect();
        feed.on_quotes(&quotes);
        feed.on_quotes(&[q("2317", 5)]);
        let s = feed.0.lock();
        assert_eq!(s.pending.len(), PENDING_CAP);
        assert_eq!(s.pending.evicted, 10, "UI 沒來取：超過容量的最舊幾筆被擠掉並計數");
    }
}
