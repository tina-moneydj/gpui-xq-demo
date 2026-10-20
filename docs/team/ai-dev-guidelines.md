# AI 開發規範（XQNext／XQ XT Rust + GPUI）

> 這份是**給人看的完整版**。給 AI 代理看的精簡版在 repo 根目錄 [`AGENTS.md`](../../AGENTS.md)（`CLAUDE.md`、`.cursor/rules/` 都指向它）。
> 兩份規則要一致；改規則時兩邊一起改。

## 1. 工作流程

每個功能（新面板、對齊一個 WPF 行為、修 bug）都走同一條線。**順序不能跳**：先有規格和測試，AI 才有明確目標；沒有驗收條件的需求，AI 會「寫出看起來能動」的東西。

```
① 規格＋驗收條件 ──► ② AI 先寫測試／行為基準 ──► ③ AI 寫實作 ──► ④ 機器驗證 ──► ⑤ 人工審查 ──► ⑥ 合併
   (人寫，AI 協助)       (人確認基準是對的)          (小步、可編譯)    clippy/test/bench     審查清單 §4
```

### ① 規格（spec）

用 `prompt-templates.md` §1 的格式，一個功能一份，放在 PR 描述或 `docs/specs/<模組>.md`。至少包含：

| 項目 | 內容 | 例（成交明細） |
|---|---|---|
| WPF 行為來源 | 截圖、錄影、WPF 原始碼檔名、行為基準錄製檔 | （示意）`XQNext.Wpf/Views/TickTapeView.xaml`、截圖 3 張 |
| 畫面 | 欄位、格式、顏色規則 | 時間 `HH:MM:SS.mmm`、價格兩位小數、量千分位、漲紅跌綠依昨收 |
| 操作 | 滑鼠、快捷鍵、右鍵選單、焦點 | `b` 只看大單、`[`/`]` 門檻、`Home` 回最新；點面板取得焦點 |
| 狀態保存 | 存什麼、存哪、何時 | 大單開關與門檻，`tick_tape.json`，變更後 450 ms |
| 效能預算 | 從 §5 表格挑對應的項目 | 每筆報價 ≤ 50 ns（背景）、可見列格式化 ≤ 1 µs／列、記憶體上限固定 |
| 驗收條件 | 可以自動判斷的句子 | 「行為基準 TT-BASIC 全過」「10 萬筆／秒下 `dropped == 0`」 |
| 已知差異 | 刻意／bug／海外版差異 | TT-D1 外內盤用價格升降近似 |

### ② 先寫測試與行為基準

- 純邏輯（推導、計算、格式化）一律放 `crates/xq-<模組>/`，**不依賴 GPUI**，才能在 CI 上不開視窗就測。
- 行為基準：把 WPF 版的「輸入 → 輸出」錄成 JSON（格式見 §6），放 `crates/xq-<模組>/tests/baseline/`。
- 人要確認基準是對的（對照 WPF 實機），**再**讓 AI 寫實作。基準錯了，後面全錯。

### ③ 實作

- 讓 AI 一次只改一個模組、一步一步可編譯。大改動拆成多個 commit。
- 新面板照 [`src/tick_tape.rs`](../../src/tick_tape.rs) 的五個【模式】結構寫（說明見 [example-module.md](example-module.md)）。
- 給 AI 的上下文至少要有：`AGENTS.md`、規格、範例模組、要改的檔案、相關的現有程式（例如 `bridge.rs`）。

### ④ 機器驗證（PR 前必跑，CI 也跑）

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
cargo clippy --release --workspace --all-targets      # 新檔案 0 警告；共用 crate 用 -D warnings
cargo test --release --workspace
cargo bench -p xq-<模組>                               # 有熱路徑就要有 bench，附前後數字
```

改到行情路徑、報價表、走勢圖時，另外跑對應壓測（`scripts/bench-main-window.sh`、`scripts/bench-ticks.sh`），把數字貼進 PR。

### ⑤ 人工審查

用 §4 的清單。審查者**不一定要會寫 Rust**，但要能回答清單上每一題。看不懂的程式碼，用 `prompt-templates.md` §8 請 AI 解釋給 C# 開發者聽，再判斷。

### ⑥ 合併

- 至少一位審查者核可；動到 `crates/xq-feed`、`src/bridge.rs`、執行緒模型、效能預算的 PR，**必須**有架構負責人核可。
- 行為基準檔有改的 PR，標題加 `[baseline]`，描述寫清楚差異分類（§6.3）。

---

## 2. 專案規則（AI 必須遵守）

### 2.1 執行緒

1. **UI 執行緒不做 I/O**：讀寫檔、網路、等鎖很久的事，一律 `cx.background_executor().spawn(…)` 或背景執行緒。（例：`TickTape::new` 讀設定、`save_settings`、`store.rs` `save_async`）
2. **行情回呼（`xq_feed::spawn_client` 的 callback、`bridge::merge`）只做 O(1)／筆的合併**：不配置大物件、不 log、不 I/O、不碰 UI。
3. **UI 每幀最多套用一次行情**：背景寫進共享緩衝，只在「空 → 非空」時喚醒 UI；UI `take()`／`swap` 整批後睡 16 ms（`main.rs` `feed_task`）。新面板掛在 `Terminal::apply_feed` 同一個節奏，**不要自己開計時器輪詢**。
4. 鎖裡只做最小的事（`std::mem::swap`、`std::mem::take`），處理放到鎖外。
5. `Rc<RefCell<…>>` 只能在 UI 執行緒；跨執行緒用 `Arc<Mutex<…>>`，熱路徑上能不用鎖就不用（原子變數、交換緩衝）。

### 2.2 資料量與記憶體

6. **合併還是保留每筆**要明講：報價（同代號只留最新）可以合併；成交明細、委託回報、成交回報**不能合併**，用固定容量緩衝＋丟棄計數，計數要顯示在效能面板／log。
7. **所有會隨時間成長的容器都要有上限**（`Ring::with_capacity`、LRU）。沒有上限的 `Vec::push` 在行情路徑上是 bug。
8. **不在每幀／每筆配置**：熱路徑用 `Copy` 小 struct、重用緩衝（`clear()` 保留容量）、雙緩衝交換；字串只為可見列格式化。

### 2.3 計算與繪製

9. **增量計算**：tick 只更新最後一根、新 K 棒才 append；指標保存每根的遞迴狀態（`indicators.rs`，有「增量＝整段重算」測試 `incremental_equals_full`）。整段重算只在換代號／週期／參數時。
10. **大清單虛擬化**（`uniform_list`），只格式化可見列。
11. **只在真的有變時 `cx.notify()`**；畫面拆成獨立 Entity 並 `.cached()`，讓 tick 只重畫有變的那塊。
12. 自繪圖形分層快取（走勢圖三層：主圖／畫線／即時），滑鼠移動只重畫最上層。
13. 上限是效能契約：主圖疊加 ≤ 20、副圖 ≤ 10（`MAX_OVERLAYS`／`MAX_SUBS`），超過要先改預算再改程式。

### 2.4 錯誤處理與 log

14. **正式路徑不用 `unwrap()`／`expect()`／索引越界 `[]`**：用 `?`、`let … else`、`get()`、`unwrap_or_default()`。允許的例外：測試、`main()` 啟動失敗、有註解說明不可能失敗的地方。
15. 壞檔／壞封包／斷線要**降級**而不是當掉：讀不到設定用預設值，封包解不開就丟掉該包（`xq_feed::decode` 回 `None`），斷線自動重連並在狀態列顯示。
16. Log 用 `eprintln!("[模組] 訊息: {e}")`（目前沒有 log crate；之後統一換 `tracing` 時一次改）。**不在每筆 tick 上 log**；量測輸出只在環境變數開啟時（`XQ_METRICS`、`XQ_TICK_BENCH`）。

### 2.5 狀態保存

17. 存檔：防抖（約 450 ms，世代號 `save_gen` 判斷是否最後一次）→ 背景寫檔 → **先寫 `.tmp` 再 `rename`**。
18. 設定型別加 `#[serde(default)]`，舊檔少欄位要讀得進來；有測試（`settings_tolerate_missing_fields_and_bad_files`）。
19. 壓測／基準模式（`XQ_BENCH`、`XQ_TICK_BENCH`、`XQ_CHART_STRESS`、`XQ_GROUP_STRESS`）**不讀不寫**使用者狀態。

### 2.6 結構與命名

20. 純邏輯在 `crates/`（XQNext 和 XQ XT 共用），畫面在 `src/`。新 crate 不依賴 `gpui`。
21. 模組之間只透過公開函式和事件（`EventEmitter`）溝通，不直接改別的 Entity 的欄位。
22. 程式碼與識別字用英文；註解、文件、UI 文字用繁體中文。新面板的檔頭註解要寫：用途、開法、執行緒模型、效能重點。
23. 不新增外部依賴，除非 PR 說明理由並經架構負責人同意（目前全部依賴：`gpui-pre`、`gpui-pre-platform`、`futures`、`parking_lot`、`serde`、`serde_json`）。

---

## 3. 模組負責人與介面合約

| 模組 | 路徑 | 負責人 | 合約（對外的東西） |
|---|---|---|---|
| 行情協議 | `crates/xq-feed` | 架構負責人 | `Subscribe`、`Msg`、`Event`、`spawn_client`；封包格式與 wry 版／WryFeedHost 一致 |
| 行情合併層 | `src/bridge.rs` | 架構負責人 | `FeedBridge::start/take/subscribe`、`Inbox`；合併規則 |
| 成交明細（範例） | `crates/xq-tape`、`src/tick_tape.rs` | 待指派 | `TapeCore::on_quote`、`Ring`、`format_row`；`TapeFeed::on_quotes`、`TickTape::apply/set_symbol` |
| 報價表 | `src/table.rs` | 待指派 | `QuoteTable::apply`、`TableEvent::Select` |
| 走勢圖 | `src/chart.rs`、`model.rs`、`indicators.rs`、`drawings.rs`、`series.rs`、`gfx.rs` | 待指派（指標可再拆一人） | `ChartView::load/apply_quote/set_intraday/apply_minute`、`ChartEvent` |
| 存檔 | `src/store.rs` | 待指派 | `Store::load/save_async`；`state.json` 格式（與 wry 畫線 JSON 相容） |
| 主視窗與版面 | `src/main.rs`、`theme.rs` | 架構負責人 | 各面板的掛載點、`apply_feed` 節奏 |

規則：

- 合約（上表右欄）要改，PR 要 @ 該模組負責人＋架構負責人。
- 一個模組一個負責人，其他人可以改，但負責人要審。人多以後用 GitHub `CODEOWNERS` 自動指派。
- 新模組先在這張表加一列（PR 一起送）。

---

## 4. 審查清單（人工審 AI 的 diff）

### 4.1 效能（最常見的 AI 毛病）

- [ ] **不必要的 `clone()`**：有沒有為了過編譯而 clone `String`／`Vec`／整個 struct？能不能改成借用 `&T`？（`Arc::clone` 便宜可以；`String::clone` 在熱路徑不行）
- [ ] **熱路徑上的 `Arc<Mutex<…>>`**：每筆報價都鎖一次？鎖裡做了什麼？能不能一包鎖一次、或改用交換緩衝？
- [ ] **全量重算**：tick 進來是不是重算了整段指標／整張表／整個畫線層？（應該只改最後一根、只改有變的列）
- [ ] **UI 執行緒阻塞**：`std::fs`、`TcpStream`、`thread::sleep`、長時間等鎖，是不是出現在 `render`、action handler、`cx.spawn`（前景）裡？
- [ ] **每幀配置**：`render`／`paint` 裡有沒有建大 `Vec`、`HashMap`、`format!` 一大堆不可見的東西？
- [ ] **無上限成長**：新的 `Vec`／`HashMap`／`VecDeque` 會不會隨時間一直長？上限在哪？
- [ ] **多餘的 notify**：沒變也 `cx.notify()`？或 notify 了父 view 導致整個視窗重建？
- [ ] **自己開計時器輪詢行情**（應該掛在 `apply_feed` 的節奏）。
- [ ] 有 bench 或壓測數字，且沒超過 §5 預算（或 PR 說明為什麼要調預算）。

### 4.2 正確性與穩定

- [ ] 正式路徑沒有 `unwrap()`／`expect()`／`[i]` 越界風險（§2.4 #14）。
- [ ] 壞資料、斷線、空資料、換代號、換日（總量歸零）都有處理，最好有測試。
- [ ] 行為基準有涵蓋這次改的行為；改了基準的話，差異已分類（§6.3）。
- [ ] 背景執行緒裡沒有碰 Entity／Window（編譯器多半會擋，但要確認沒有用奇怪的方法繞過）。
- [ ] 存檔：防抖、背景寫、`.tmp`+rename、壓測模式不寫。

### 4.3 可維護性

- [ ] 純邏輯有沒有放進 `crates/`、能不能不開視窗就測？
- [ ] 快捷鍵是 action + key context，不是 `on_key_down` 自己比字串。
- [ ] 檔頭註解有寫執行緒模型與效能重點；註解是繁中、識別字是英文。
- [ ] 沒有新依賴（或已經核可）。
- [ ] 沒有不相關的格式化／重構混在同一個 PR。

---

## 5. 效能預算

數字來自本 repo 的實測（README〈效能比較〉，測試機：8 vCPU、**無 GPU**、Mesa lavapipe 軟體繪圖、Xvfb，release build，3 次中位數）。
**「門檻」是回歸警戒線**：PR 讓數字超過門檻就要說明或修正。有 GPU 的目標機（Windows）數字出來後，再另訂一欄。

| 項目 | 目前實測（GPUI） | 門檻（不可超過） | 怎麼量 |
|---|---|---|---|
| 啟動 → 第一幀 | 285 ms | 400 ms | `scripts/bench-main-window.sh 3 all` |
| 啟動 → 第一筆報價上畫面 | 398 ms | 500 ms | 同上 |
| 主視窗 RSS（60 秒） | 182 MB | 220 MB | 同上 |
| 重圖表 RSS（20 疊加＋8 副圖＋1000 畫線） | 219–223 MB | 260 MB | `GPUI_TOP_FRAC=0.811 scripts/bench-main-window.sh 3 heavy` |
| CPU（扣掉軟體繪圖），一般／重圖表 | 22% ／ 37% | 35% ／ 50% | 同上（llvmpipe 執行緒另計） |
| 每次 UI 套用行情（`apply_feed`） | 17–24 µs | 1 ms | `XQ_METRICS=1` 的 `apply_us_avg` |
| 走勢圖每幀 CPU 組圖（重圖表） | 2.2–3.1 ms | 4 ms | `XQ_CHART_STRESS=1` 的 `CHARTPERF` |
| 報價表可見格更新 | 0.09 ms／次 | 0.5 ms | `XQ_GROUP_STRESS=1` 的 `GROUPPERF` |
| 加 5 萬列報價表的記憶體增量 | ≈ 4.5 MB | 10 MB | 按「壓力 5 萬」前後看 RSS |
| 高頻 10 萬筆／秒：掉筆 | 0 | 0 | `scripts/bench-ticks.sh 3` |
| 高頻 10 萬筆／秒：積壓（後 5 秒 vs 前 5 秒延遲 p50） | 不增加 | 增加 ≤ 1 幀 | 同上 |
| 高頻 10 萬筆／秒：RSS 30 秒內成長 | 0（固定 198 MB） | 5 MB | 同上 |
| 高頻 10 萬筆／秒：延遲 p99（本機無 GPU） | 108 ms | 125 ms | 同上 |
| 高頻下點選切換代號 | 51–56 ms | 70 ms | 同上（xdotool 點選） |
| 熱路徑函式：每筆報價的背景處理 | `TapeCore::on_quote` 1.4 ns | 50 ns | `cargo bench -p xq-tape` |
| 熱路徑函式：可見列格式化 | `format_row` ≈ 250 ns | 1 µs | 同上 |
| 單一幀的總預算（目標 60 FPS） | — | 16 ms（CPU 端工作 ≤ 4 ms） | — |

對照：wry 版（WebView）同情境 RSS 674 MB、重圖表 685–804 MB；高頻 10 萬筆節流前 p99 164 ms／記憶體漲到 1,163 MB —— 這就是「每筆推給 UI」的代價，也是規則 §2.1 #3 的由來。

---

## 6. 行為對齊流程（對 WPF 版）＝行為基準庫

XQNext QA 現在卡在「跟舊版行為不同」，因為行為沒有被寫成可比對的規格。做法：**把 WPF 版的行為錄下來，變成自動測試**。

### 6.1 要錄什麼

| 類別 | 內容 | 格式 | 怎麼錄（建議） |
|---|---|---|---|
| 快捷鍵 | 每個畫面：按鍵 → 動作 → 結果狀態 | `shortcuts.json`：`{context, keys, action, expect}` | WPF 原始碼的 `InputBindings`／`KeyBinding` 掃一遍產生清單（可請 AI 掃 XAML） |
| 右鍵選單 | 每個畫面的選單項目、順序、勾選狀態、停用條件 | `menus.json` | 用 UI Automation（如 FlaUI）在 WPF 實機上展開選單錄下來 |
| 顯示格式 | 價格小數位、千分位、漲跌符號、百分比、時間格式、顏色規則 | 輸入值 → 預期字串（如 `tick_tape_basic.json`） | 在 WPF 版的 formatter 加一個 debug 匯出，或手抄 20–50 個代表值 |
| 指標數值 | 固定一份 K 線資料，WPF 算出的每根 MA/EMA/BB/SAR/KD/MACD/RSI/%R/DMI/ATR/OBV | CSV：`t, 指標, 參數, 值`；容許誤差 1e-6 | WPF 版跑同一份資料匯出 CSV |
| XS 結果 | 固定資料上的 XS 腳本輸出（選股結果、訊號點） | JSON | 之後 XS 引擎接進來時用 |
| 互動狀態 | 焦點移動順序、拖曳分隔線的範圍、視窗記憶的版面 | 文字描述＋截圖＋可測的部分寫 JSON | QA 錄影＋手寫 |

### 6.2 放哪、怎麼跑

- 邏輯類基準跟著模組：`crates/xq-<模組>/tests/baseline/*.json`，由 `tests/baseline.rs` 自動讀取全部 JSON 逐步比對（範例：`crates/xq-tape/tests/baseline.rs`）。
- 失敗訊息要能直接看懂：「哪個案例、第幾步、預期、實際」。
- 每個 JSON 都要有 `source`（WPF 版本、截圖或錄製檔路徑），沒有來源的基準不收。
- 畫面類（截圖比對）之後再做；先把邏輯和格式全部鎖住，CP 值最高。

### 6.3 差異分類

每一個「跟 WPF 不一樣」的地方都要歸到一類，寫進 JSON 的 `known_diffs` 或 issue：

| 分類 | 意思 | 處理 |
|---|---|---|
| **刻意** | 新版故意改的（例如更好的預設、技術限制） | 要有公告；能保留舊行為的話提供設定開關；產品負責人核可 |
| **bug** | 不小心改壞的 | 修程式，讓基準過；基準**不改** |
| **海外版差異** | XQNext 海外版本來就不同（時區、市場、語系、幣別） | 寫成設定或參數（如 `utc_offset_sec`），台灣版與海外版各一份預期值 |

規則：**沒有分類的差異一律當 bug。** 改基準檔的 PR 要標 `[baseline]` 並寫分類與核可人。這樣 QA 不用每件事重新爭論，只要看清單。

---

## 7. 分支與 PR 慣例

> 目前這個 repo 只有一個人在推 `main`。團隊加入後改用以下規則（`main` 設保護，必須 PR＋CI 綠＋審查）。

- **分支**：`feat/<模組>-<簡述>`、`fix/<模組>-<簡述>`、`perf/<模組>-<簡述>`、`baseline/<模組>-<簡述>`、`docs/<簡述>`。例：`feat/tape-big-lot-filter`。
- **Commit 訊息**：`<類型>(<模組>): <繁中說明>`，類型用 `feat`／`fix`／`perf`／`test`／`docs`／`refactor`／`bench`。例：`perf(bridge): 喚醒訊號改成空→非空才送`。
- **PR 大小**：一個 PR 一件事，diff 盡量 < 500 行（不含基準 JSON）。AI 產生的大改動拆成多個 PR。
- **PR 描述模板**：

```markdown
## 做什麼
（一兩句；連到規格）

## WPF 對齊
- 來源：（截圖／錄製檔／WPF 檔名）
- 行為基準：新增／修改了哪些案例
- 差異：（無／列出並分類：刻意／bug／海外版差異）

## 驗證
- [ ] cargo build --release
- [ ] cargo clippy（新程式 0 警告）
- [ ] cargo test --workspace
- [ ] bench／壓測（前 → 後）：
- [ ] 截圖：

## AI 使用
- 用了哪個 prompt 模板、人工改了哪些地方

## 審查重點
（希望審查者特別看哪裡）
```

- **審查**：至少一人；§3 的合約或 §5 的預算有動 → 架構負責人。審查者用 §4 清單。
- **禁止**：直接推 `main`、`--force` 推共用分支、跳過 CI（`--no-verify`）、把壓測模式的狀態檔提交進 repo。
