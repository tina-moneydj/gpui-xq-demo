# Prompt 模板（複製貼上用）

> 用法：複製整段，把 `{{…}}` 換掉，並附上「必附上下文」列的檔案。
> 所有模板都假設 AI 讀得到 repo（Cursor／Claude Code／Codex 等會自動讀 `AGENTS.md`）；
> 如果是貼到聊天視窗，**先貼 `AGENTS.md` 全文**。
> 原則：**給驗收條件、給範例、要求先測試**。AI 拿到越具體的「什麼叫做完成」，越不會亂寫。

---

## 1. 從 WPF 截圖／規格做一個新面板

**必附上下文**：WPF 截圖（1–3 張）、WPF 對應的 XAML／ViewModel 檔（有的話）、`AGENTS.md`、`src/tick_tape.rs`、`crates/xq-tape/`（範例）、`docs/team/example-module.md`、`src/main.rs`（掛載點）、資料來源相關檔（通常是 `crates/xq-feed/src/lib.rs`、`src/bridge.rs`）。

```text
你要在 gpui-xq-demo（Rust + GPUI）新增「{{面板名稱，例：漲跌幅排行}}」面板，對齊 WPF 版 XQNext 的同名畫面。

【WPF 行為（正本）】
- 截圖：{{附件}}；WPF 原始碼：{{檔名或「無」}}
- 欄位與格式：{{欄位、小數位、千分位、顏色規則（漲紅跌綠依昨收）}}
- 操作：{{滑鼠、快捷鍵、右鍵選單、焦點規則}}
- 保存：{{要存的設定，或「不存」}}
- 已知要和 WPF 不同的地方：{{列出並標分類：刻意／海外版差異；沒有就寫「無」}}

【資料】來源：{{xq-feed 的哪個 Msg／欄位；若引擎還沒有，寫「先用假資料，介面要能換」}}
合併策略：{{可以同代號只留最新／每筆都要保留（固定容量緩衝＋丟棄計數）}}

【效能預算】（docs/team/ai-dev-guidelines.md §5）
- 背景每筆處理 ≤ {{50}} ns；可見列格式化 ≤ {{1}} µs／列；記憶體上限 {{N 筆 ≈ X KB}}，不隨時間成長
- 套用節奏：掛在 Terminal::apply_feed，每幀最多一次；不要自己開計時器

【做法要求】
1. 照 src/tick_tape.rs 的五個【模式】結構寫：純邏輯放 crates/xq-{{名稱}}（不依賴 gpui），view 放 src/{{名稱}}.rs。
2. 先寫：crates/xq-{{名稱}}/tests/baseline/{{名稱}}_basic.json（從上面的 WPF 行為整理至少 8 步，含邊界：空資料、換代號、異常值）＋單元測試。先給我看基準檔，我確認後再寫實作。
3. 再寫實作與 benches/{{名稱}}.rs（每個熱路徑函式一項，寫上預算）。
4. 快捷鍵用 actions! + KeyBinding + key_context，不要用 on_key_down。
5. 預設用環境變數 XQ_{{名稱大寫}}=1 才掛進主視窗，不影響現有壓測版面。
6. 註解用繁體中文、識別字英文；檔頭寫用途、開法、執行緒模型、效能重點。

【完成條件】
- cargo build --release、cargo clippy --release --workspace --all-targets（新檔 0 警告）、cargo test --release --workspace 全過
- cargo bench -p xq-{{名稱}} 全部 OK
- 回報：新增／修改的檔案、每段程式跑在哪個執行緒、記憶體上限怎麼保證、和 WPF 的差異清單
```

---

## 2. 把一個 WPF／C# 類別移植成 Rust

**必附上下文**：C# 原始檔（整個 class，含它用到的 model）、它的單元測試（有的話）、`AGENTS.md`、要放的目標模組現有程式、`docs/team/learning-roadmap.md` 的〈C# → Rust 對照表〉。

```text
把下面這個 C# 類別移植成 Rust，放在 {{crates/xq-xxx/src/xxx.rs}}。

【C# 原始碼】
{{貼上 .cs，或附檔名}}

【要求】
1. 行為完全相同（包含邊界：null、空集合、除以零、溢位、四捨五入方式）。先列出你從 C# 讀到的「所有行為規則」清單（含隱含的，例如 Math.Round 預設是銀行家捨入），讓我確認。
2. 對照：class → struct+impl；interface → trait；null → Option；例外 → Result（定義具體的錯誤 enum，不要用 String 當錯誤）；event → 回傳值或 callback，不要引入 GPUI。
3. 不要逐行直譯：C# 的 List<T> 頻繁 Add/Remove、LINQ 每次 ToList()、字串串接，改成 Rust 的高效寫法（借用、迭代器、預留容量），但不要為了效能改變行為。
4. 不用 unwrap/expect（測試除外）；不新增依賴。
5. 把 C# 的單元測試（若有）逐一移植成 #[test]；沒有的話，依第 1 點的規則清單各寫一個測試。
6. 另外給我一張表：C# 成員 → Rust 對應（名稱、型別、差異說明）。

【完成條件】cargo test -p {{crate}} 全過、cargo clippy -p {{crate}} --all-targets -- -D warnings 乾淨。
```

---

## 3. 寫行為基準測試（對 WPF 錄下來的行為）

**必附上下文**：WPF 實機的觀察紀錄（截圖、操作步驟、輸入資料與顯示結果）、`crates/xq-tape/tests/baseline.rs` 與 `tests/baseline/tick_tape_basic.json`（範例格式）、`docs/team/ai-dev-guidelines.md` §6。

```text
幫 {{模組}} 建立行為基準，格式照 crates/xq-tape/tests/baseline/ 的範例。

【WPF 觀察紀錄】
{{逐步列：輸入（或操作）→ WPF 畫面顯示什麼；附截圖}}

【要求】
1. 產生 crates/xq-{{模組}}/tests/baseline/{{案例名}}.json：
   - id、title、source（填：{{WPF 版本號 + 截圖路徑}}）
   - steps：每步有 note（繁中描述）、輸入欄位、expect（預期畫面文字；不產生輸出時為 null）
   - known_diffs：已知差異，category 只能是「刻意」「bug」「海外版差異」之一
2. 補齊我沒列到但應該要測的邊界：{{例：第一筆、換代號、資料更正、零、負數、極大值、跨日}}。補的步驟在 note 前面加「（補）」，expect 先依你推測填，並列出來讓我去 WPF 實機確認。
3. 如果 tests/baseline.rs 還不存在，照 xq-tape 的寫法建立，讀取目錄下所有 JSON、逐步比對、失敗時印「案例 id、第幾步、note、預期、實際」。
4. 這一步**不要改實作**。跑 cargo test -p xq-{{模組}} --test baseline，回報哪些步驟目前不過（這就是待修清單）。
```

---

## 4. 效能審查一個 diff

**必附上下文**：`git diff main...HEAD`（或 PR 連結）、`AGENTS.md`、`docs/team/ai-dev-guidelines.md` §4 審查清單與 §5 預算、被改到的檔案全文（不只 diff）。

```text
你是這個 repo 的效能審查者。審查下面的 diff，目標是 XQ 高頻看盤（每秒 10 萬筆報價、開盤爆量）下不卡、不漲記憶體。

【diff】
{{貼上 git diff，或 PR 連結}}

【請逐項檢查並回報，每一項都要指到檔案:行號】
1. 執行緒：哪些新程式碼跑在 UI 執行緒、哪些在行情背景執行緒、哪些在 background_executor？有沒有 UI 執行緒 I/O、阻塞、長時間持鎖？
2. 熱路徑：每筆報價／每幀會跑到的程式碼裡，有沒有 clone String/Vec、配置新容器、format!、HashMap 重建、log？
3. Arc<Mutex<…>>：在熱路徑上嗎？鎖裡做什麼？能不能一包鎖一次或改交換緩衝？
4. 全量重算：有沒有「tick 進來就整段重算／整表重建／整層重畫」？
5. 成長：新的容器有沒有上限？10 萬筆／秒跑 1 小時會變多大？
6. notify：有沒有沒變也 notify、或 notify 太上層造成大範圍重畫？
7. unwrap/expect/索引越界在正式路徑上？
8. 跟 §5 預算比，哪一項可能超標？建議跑哪個 bench／壓測驗證（給指令）。

【輸出格式】
- 依嚴重度排序：🔴 必改（會卡頓／當機／記憶體無上限）、🟡 建議、🟢 沒問題的部分（簡短）
- 每個 🔴／🟡 附修改建議的程式碼片段
- 不要改檔案，只回報。
```

---

## 5. 修編譯錯誤

**必附上下文**：**完整**的 `cargo build` 錯誤輸出（不要只貼第一行）、出錯的檔案全文、你原本想做什麼。

```text
cargo build 失敗。請先解釋原因，再修。

【我原本想做的事】{{一句話}}

【完整錯誤輸出】
{{貼上全部，包含 error[E....]、箭頭標示與 help:}}

【相關檔案】{{檔名；或貼全文}}

【要求】
1. 先用 C# 開發者聽得懂的話解釋：這個錯誤在保護什麼（例如：同時有可變借用和不可變借用 = C# 裡一邊 foreach 一邊改 List 的那種 bug）。
2. 給出修法，並說明為什麼這個修法是對的。
3. **禁止**用以下方式「讓它編譯過」，除非你說明理由且我同意：
   - 加 .clone() 複製 String/Vec/大 struct
   - 包成 Arc<Mutex<…>> 或 Rc<RefCell<…>>
   - 加 'static、unsafe、unwrap()
   - 改變函式的公開簽名（會影響其他模組）
4. 修完跑 cargo build --release 和 cargo test -p {{crate}}，貼結果。
```

---

## 6. 加一個 XS 相關功能

**必附上下文**：XS 語法／函式的官方定義或 XQ 說明文件、WPF 版（或 XQ 現行版）的實際輸出（固定資料上的數值或截圖）、`AGENTS.md`、`src/indicators.rs`（增量計算範例）、`src/series.rs`（K 線資料）、相容性要求。

```text
實作 XS 的 {{函式／功能名稱，例：Highest(price, length)、Lowest、Average、交叉判斷 CrossOver}}，要和 XQ 現行版**數值完全相同**（XS 用戶寫了上百支腳本，升版行為不同是主要抱怨）。

【XQ 官方定義】{{貼上說明文件段落，含參數、回傳、邊界行為}}
【XQ 實際輸出】{{固定資料（附 CSV）上 XQ 算出的值；至少 50 根，含資料不足 length 根的開頭}}
【已知爭議】{{例：升版後 highest/lowest 是否包含當根、資料不足時回傳什麼}}

【要求】
1. 純邏輯放 crates/xq-xs/（不依賴 gpui；沒有這個 crate 就建立）。
2. 先把【XQ 實際輸出】做成行為基準 tests/baseline/{{函式}}.json（容許誤差 1e-9），以及資料不足、NaN、length=0／1、跨日等邊界測試。先給我看。
3. 實作要支援**增量更新**：tick 改最後一根時 O(1) 或 O(log n)，新 K 棒 append；並有「增量結果＝整段重算」的測試（參考 src/indicators.rs 的 incremental_equals_full）。
4. 任何跟 XQ 現行版不同的地方，寫進 known_diffs 並標「刻意」或「bug」，不可以默默不同。
5. 加 bench：1200 根整段計算、單根增量更新，各寫預算（整段 ≤ {{1}} ms、增量 ≤ {{1}} µs）。

【完成條件】基準全過、clippy 乾淨、bench OK；回報一張「XQ 行為 vs 我們的行為」對照表。
```

---

## 7. 寫 bench

**必附上下文**：要量的函式所在檔案、`crates/xq-tape/benches/tape.rs`（範例）、`docs/team/ai-dev-guidelines.md` §5 預算、這個函式被呼叫的頻率（每筆報價？每幀？每次使用者操作？）。

```text
幫 {{crate}} 的 {{函式名稱}} 寫 micro-bench，放 crates/{{crate}}/benches/{{名稱}}.rs，格式照 crates/xq-tape/benches/tape.rs（harness = false、不加外部依賴、std::hint::black_box、先暖身、印「項目 / ns/次 / 預算 / OK|OVER」、XQ_BENCH_STRICT=1 時超預算 exit 1）。

【呼叫頻率】{{例：每筆報價一次，高頻壓測時 10 萬次／秒，在背景執行緒}}
【預算推導】請依頻率推導預算並寫在檔頭註解：
  例：10 萬次／秒 × 50 ns = 5 ms／秒 = 0.5% 一顆核心 → 預算 50 ns
【輸入資料】{{要模擬的真實分佈，例：90% 非目前代號、10% 目前代號；或 1200 根日 K}}

【要求】
1. 量測不含準備資料的成本（資料在迴圈外建好）。
2. 每個 case 的迭代次數讓總時間落在 0.2–2 秒。
3. 如果有「整段 vs 增量」兩種路徑，兩個都量。
4. Cargo.toml 加 [[bench]] name = "{{名稱}}" harness = false。
5. 跑 cargo bench -p {{crate}}，貼結果；若有 OVER，分析原因但先不要改實作。
```

---

## 8. 把不熟的 Rust 程式碼解釋給 C# 開發者

**必附上下文**：要解釋的檔案或片段（附檔名與行號）、`docs/team/learning-roadmap.md` 的〈C# → Rust 對照表〉。

```text
我是 C#／WPF 開發者，剛開始讀 Rust + GPUI。請解釋 {{檔名:行號範圍}} 這段程式碼。

【請依序回答】
1. 一句話：這段在做什麼、在整個資料流（行情背景執行緒 → bridge 合併 → UI 每幀套用 → render）裡的哪一站。
2. 跑在哪個執行緒？為什麼一定要在那裡？
3. 逐段解釋，每個 Rust 特有的寫法都給「C# 大概等於…」的對照：
   例：&mut self、借用、move closure、Option/?、let…else、match、trait、Arc/Mutex、Rc/RefCell、cx.notify()、Entity、cx.spawn、actions!。
4. 這段為了效能做了什麼特別的選擇（例如借用而不 clone、Copy struct、固定容量緩衝、只格式化可見列）？如果用 C# 習慣直接寫，會有什麼問題？
5. 如果我要改 {{想改的地方}}，要注意什麼、會影響哪些檔案？
6. 給我 2 個自我檢查題（附答案，答案摺疊在 <details> 裡）。

不要改任何檔案。用繁體中文回答。
```
