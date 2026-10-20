# 團隊學習路線：Rust + GPUI（全 AI 開發版）

> 對象：XQNext／XQ XT 翻寫團隊（多數是 C#／WPF 背景）。
> 前提：**程式主要由 AI 寫**。所以目標不是「每行都會自己寫」，而是**看得懂、判斷得出 AI 寫得對不對、會量測**。
> 所有練習都在這個 repo（`gpui-xq-demo`）裡做，範例都指向真的程式碼。

## 兩條路線

| | 架構負責人（深） | 功能開發者（讀＋審） |
|---|---|---|
| 人數 | 1–2 位 | 其他所有人 |
| 責任 | 共用底層（`crates/*`）、執行緒模型、元件庫、效能預算、審查關鍵 PR、決定 GPUI 升版 | 用 AI 開發功能面板、對齊 WPF 行為、寫行為基準、跑 bench、審同事的 PR |
| 要能做到 | 不靠 AI 也能讀懂並改 `bridge.rs`、`chart.rs` 的分層快取；看得懂 GPUI 原始碼 | 看得懂 AI 產出的 diff，抓出效能／執行緒錯誤；能把編譯錯誤和需求講清楚丟回給 AI |
| 每週時間 | 約 10–12 小時 | 約 6–8 小時（其餘時間直接做功能） |

每一週的格式：**主題 → 為什麼對 XQ 重要 → 練習（本 repo）→ 自我檢查 → 連結**。
功能開發者做標 🟢 的部分；架構負責人 🟢＋🔵 都要做。

開始前（第 0 天，兩條路線都要）：

```bash
git clone https://github.com/tina-moneydj/gpui-xq-demo && cd gpui-xq-demo
export PATH="$HOME/.cargo/bin:$PATH"     # rust-toolchain.toml 指定 stable（需 rustc ≥ 1.92）
cargo build --release
cargo test --release --workspace          # 全部要綠
cargo bench -p xq-tape                    # 看懂輸出：每項 ns/次 vs 預算
XQ_TAPE=1 ./target/release/gpui-xq-demo   # 有行情來源時（WryFeedHost 或 xq-tickgen）
```

沒有 WryFeedHost 時用內建產生器：`./target/release/xq-tickgen --rate 2000 --duration 600 --rows 0`（另開一個終端機）。

---

## 第 1 週：讀得懂 Rust（所有權、錯誤處理、型別）

### 1.1 🟢 所有權與借用、`clone()` 的成本

- **為什麼對 XQ 重要**：高頻行情下，每筆報價多一次 `String` clone，每秒 10 萬筆就是 10 萬次配置。AI 最常見的「讓它編譯過」手法就是到處 `.clone()` 或包 `Arc<Mutex<…>>`，人要抓得出來。
- **練習**：
  1. 讀 `crates/xq-tape/src/lib.rs` 的 `TapeCore::on_quote(&mut self, q: &Quote, …)`：為什麼參數是 `&Quote` 不是 `Quote`？如果改成 `Quote`，`src/bridge.rs` 的 `merge` 要多做什麼？
  2. 讀 `Trade` 為什麼是 `#[derive(Clone, Copy)]`，而 `Quote` 不是（提示：`String` 欄位）。
  3. 把 `TapeCore::set_symbol` 改成 `self.symbol = symbol.to_string();`，說明差在哪（每次換代號多一次配置；不嚴重，但要知道）。
- **自我檢查**：
  - `&T`、`&mut T`、`T` 三種傳法，各自代表呼叫端之後還能不能用這個值？
  - `Arc::clone` 和 `String::clone` 成本差在哪？（前者只加計數，後者複製內容）
  - 為什麼 `src/table.rs` 的 `Row` 用 `SharedString` 存代號？（clone 便宜，render 時要傳給元素）
- **連結**：[Rust Book 第 4 章 所有權](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)（[繁中版](https://rust-lang.tw/book-tw/ch04-00-understanding-ownership.html)）、[Rustlings](https://github.com/rust-lang/rustlings)（做 `move_semantics`、`primitive_types`、`structs`、`enums`）、[Microsoft：Rust for C#/.NET 開發者](https://microsoft.github.io/rust-for-dotnet-devs/latest/)

### 1.2 🟢 `Result`、`?`、不亂用 `unwrap()`

- **為什麼對 XQ 重要**：C# 的例外會往上丟，Rust 的 `unwrap()` 失敗就是**整個程式 panic 關掉**。盤中看盤軟體因為一個壞掉的設定檔就當掉，是最傷信任的事（見黏著度研究：開盤可靠度是第一流失原因）。
- **練習**：
  1. 讀 `src/tick_tape.rs` 的 `load_settings`／`save_settings`：讀檔失敗回預設值、寫檔失敗只記 log；找出 `?` 在哪裡把錯誤往上傳。
  2. 讀 `crates/xq-feed/src/lib.rs` 的 `Cursor`：每個讀取都回 `Option`，`decode` 用 `?` 串起來 —— 封包壞掉回 `None`，**不會 panic**。
  3. `rg -n "unwrap\(\)|expect\(" src crates`，逐一判斷：這個能不能在正式路徑上失敗？（測試裡可以用；`main()` 開視窗失敗用 `expect` 可以接受）
- **自我檢查**：`Option` 和 `Result` 差在哪？`?` 在回傳 `Option` 的函式和回傳 `Result` 的函式裡各做什麼？什麼情況 `expect("…")` 是可以接受的？
- **連結**：[Rust Book 9.2 Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)（[繁中](https://rust-lang.tw/book-tw/ch09-02-recoverable-errors-with-result.html)）、Rustlings `error_handling`、`options`

### 1.3 🟢 struct／enum／match、trait 與泛型（基本看得懂）

- **為什麼對 XQ 重要**：指標（`src/indicators.rs` 的 `OvSpec`／`SubKind`）、畫線工具（`src/drawings.rs` 的 `Tool`）、行情訊息（`xq_feed::Msg`）都是 enum + match。新增一種指標時，編譯器會告訴你哪些 `match` 漏了 —— 這是比 C# `switch` 更安全的地方。
- **練習**：
  1. 在 `xq_tape::Side` 加一個 `Auction`（試撮），跑 `cargo build`，看編譯器列出哪些 `match` 要補，再改回來。
  2. 讀 `Ring<T: Copy>`：泛型參數 `T` 加了 `Copy` 限制，為什麼 `newest()` 才能回傳 `Option<T>` 而不是 `Option<&T>`？
  3. 讀 `impl Render for TickTape`、`impl Focusable for TickTape`：trait ≈ C# interface。
- **自我檢查**：`impl Trait for Type` 對應 C# 的什麼？`T: Copy` 這種限制對應 C# 的什麼？（`where T : struct` 之類的泛型條件）
- **連結**：[Rust Book 第 10 章 泛型與 trait](https://doc.rust-lang.org/book/ch10-00-generics.html)（[繁中](https://rust-lang.tw/book-tw/ch10-00-generics.html)）、[第 18 章 Rust 的 OOP](https://doc.rust-lang.org/book/ch18-00-oop.html)、[Rust by Example](https://doc.rust-lang.org/rust-by-example/)

### 1.4 🟢 cargo／clippy／test／bench，和「用 AI 讀編譯錯誤」

- **為什麼對 XQ 重要**：全 AI 開發時，**編譯器和測試就是第一道審查**。會跑、會看輸出、會把錯誤原樣丟回 AI，是每天的基本功。
- **練習**：
  1. `cargo clippy --release -p xq-tape --all-targets -- -D warnings` 要是乾淨的；故意在 `fmt_int` 寫回 `% 3 == 0`，看 clippy 建議什麼。
  2. `cargo test -p xq-tape --test baseline`：把 `tests/baseline/tick_tape_basic.json` 某個 `"qty": "1,200"` 改成 `"1200"`，讀失敗訊息（哪一步、預期 vs 實際），再改回來。
  3. 故意寫一個借用錯誤（例如在 `TickTape::apply` 的 closure 裡呼叫 `self.rows()`），把**完整錯誤訊息**和那段程式碼貼給 AI，請它解釋「為什麼」再修（模板：`prompt-templates.md` §5）。
- **自我檢查**：`error[E0502]` 是什麼意思？`cargo test` 和 `cargo test --release` 差在哪？bench 為什麼要 `black_box`？
- **連結**：[Cargo Book](https://doc.rust-lang.org/cargo/)、[Clippy](https://doc.rust-lang.org/clippy/)、[錯誤代碼索引](https://doc.rust-lang.org/error_codes/error-index.html)、[Rust Book 第 11 章 測試](https://doc.rust-lang.org/book/ch11-00-testing.html)、[`std::hint::black_box`](https://doc.rust-lang.org/std/hint/fn.black_box.html)

### 🔵 架構負責人加修（第 1 週）

- 生命週期標註（讀懂 `FrameReader::next_frame` 回傳 `&[u8]` 借用內部緩衝，為什麼這樣能「每包不配置」）。
- `Rc<RefCell<T>>`（內部可變性）：`src/chart.rs` 的 `type M = Rc<RefCell<Model>>` 讓三個繪圖層共用一份 Model。什麼時候會 `BorrowMutError` panic？為什麼這裡可以接受（全在 UI 執行緒、不跨 await 持有）？
- 連結：[Rust Book 15.5 內部可變性](https://doc.rust-lang.org/book/ch15-05-interior-mutability.html)、[Comprehensive Rust（繁中）](https://google.github.io/comprehensive-rust/zh-TW/)

---

## 第 2 週：並行模型與 UI 執行緒邊界（最關鍵的一週）

### 2.1 🟢 執行緒、channel、`Arc<Mutex<T>>`

- **為什麼對 XQ 重要**：「行情和 I/O 不進 UI 執行緒」是整個架構的地基。wry 版在每秒 10 萬筆時延遲 p99 164 ms、記憶體漲到 1.1 GB，根因就是「每包都推給 UI」；改成每幀最多推一次後 p99 降到 72 ms（見 README 高頻壓測）。GPUI 版從第一版就是背景合併、UI 每 16 ms 拉一次。
- **練習**：
  1. 照順序讀：`crates/xq-feed/src/lib.rs` `spawn_client`（背景執行緒讀 TCP）→ `src/bridge.rs` `merge`（背景合併進 `Inbox`）→ `wake`（只有空變非空才喚醒）→ `src/main.rs` `feed_task`（UI 被喚醒 → `apply_feed` → 睡 16 ms）。畫出一張「哪段在哪個執行緒」的圖。
  2. 讀 `src/tick_tape.rs` 的 `TapeFeed::on_quotes`：鎖一次處理整包；`TickTape::apply` 在鎖裡只做 `std::mem::swap`。說明為什麼不在鎖裡做 `drain_oldest_first`。
  3. 讀 `tick_tape::tests::feed_buffers_only_current_symbol_and_caps_memory`：UI 不來取時，記憶體上限在哪？
- **自我檢查**：
  - 報價表可以「同代號只留最新」，成交明細為什麼不行？（每筆都是資料）
  - 為什麼 `bridge.rs` 用 `AtomicBool` 判斷要不要喚醒，而不是每包都送訊號？
  - 如果有人在 `merge` 裡寫 `eprintln!` 或寫檔，會發生什麼？
- **連結**：[Rust Book 第 16 章 無懼並行](https://doc.rust-lang.org/book/ch16-00-concurrency.html)（[繁中](https://rust-lang.tw/book-tw/ch16-00-concurrency.html)）、[`Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html)、[`parking_lot`](https://docs.rs/parking_lot/latest/parking_lot/)、Rustlings `threads`、`smart_pointers`

### 2.2 🟢 async、`cx.spawn`、`background_executor`

- **為什麼對 XQ 重要**：GPUI 的 async task 預設跑在**前景（UI）執行緒**；只有 `background_executor().spawn` 才在背景。分不清楚，就會在 UI 執行緒上讀檔、算全段指標，盤中卡頓。
- **練習**：
  1. 讀 `TickTape::new` 的讀檔：`cx.spawn(async move |this, cx| { … cx.background_executor().spawn(async move { load_settings(path) }).await; this.update(…) })`。哪一行在背景、哪一行回到 UI？
  2. 讀 `TickTape::schedule_save` 和 `src/chart.rs` `schedule_save`：防抖 450 ms 的寫法（`save_gen` 世代號）。
  3. 讀 `src/main.rs` 的 `stats_task`：每秒取樣一次 FPS／RSS，**不綁在 tick 上**。
- **自我檢查**：`this.update(cx, …)` 為什麼回傳 `Result`？`.detach()` 和把 `Task` 存進 `_tasks` 欄位差在哪？（後者 view 關掉時 task 會一起取消）
- **連結**：[Rust Book 第 17 章 async](https://doc.rust-lang.org/book/ch17-00-async-await.html)、[Async Book](https://rust-lang.github.io/async-book/)、GPUI 原始碼 [`executor.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/executor.rs)

### 🔵 架構負責人加修（第 2 週）

- `Send`／`Sync`：為什麼 `Rc<RefCell<Model>>` 不能丟進背景執行緒，而 `Arc<Mutex<TapeShared>>` 可以？編譯器怎麼擋？
- 鎖競爭：用 `perf`／計時量 `TapeFeed` 鎖的持有時間；10 萬筆／秒下 UI `apply` 等鎖多久。
- Memory ordering：`bridge.rs` 的 `swap(true, AcqRel)`／`store(false, Release)` 為什麼這樣配。
- 背壓策略設計：合併（報價）、固定環狀緩衝＋計數（成交明細）、阻塞（不可用在 UI）三種，哪種資料用哪種；寫成架構決策紀錄（ADR）。

---

## 第 3 週：GPUI 核心

> 本 repo 鎖定 `gpui-pre = 0.3.7`（Zed 的 GPUI 發佈快照）。**先讀本機那一版的原始碼**：
> `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.7/`（含 `docs/`、`examples/`），
> Zed 的 `main` 分支可能已經改了 API。API 文件：[docs.rs/gpui-pre/0.3.7](https://docs.rs/gpui-pre/0.3.7/gpui/)、官網 [gpui.rs](https://www.gpui.rs/)。

### 3.1 🟢 `App`／`Window`／`Context<T>`、`Entity` 與 `cx.notify()`

- **為什麼對 XQ 重要**：這就是 GPUI 版的 MVVM。沒有 binding，改完狀態要自己 `cx.notify()`；**沒 notify 就不重畫**，這正是 GPUI 省 CPU 的原因，也是新手最常見的 bug（「資料改了畫面沒變」）。
- **練習**：
  1. 讀 `src/main.rs` `Terminal::new`：`cx.new(|cx| ChartView::new(…))` 建 Entity；`cx.subscribe(&table, …)` 收 `TableEvent::Select`（≈ C# event）。
  2. 讀 `QuoteTable::apply`：只有真的有列改變才 `cx.notify()`。
  3. 在 `TickTape::apply` 把 `cx.notify()` 註解掉，跑起來看成交明細還會不會動（不會，直到別的原因觸發重畫）。
- **自我檢查**：`Entity<T>`、`WeakEntity<T>`、`Context<T>` 各是什麼？`cx.emit` + `EventEmitter` 對應 C# 的什麼？
- **連結**：本機 `gpui-pre-0.3.7/docs/contexts.md`（[Zed main 版](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/contexts.md)）、[`app/context.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/app/context.rs)、[README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)

### 3.2 🟢 `Render` 與元素樹

- **為什麼對 XQ 重要**：`render()` 每次 notify 都會重跑，它是**熱路徑**。WPF 的 XAML 是宣告一次；GPUI 的 `div().flex()…` 是每幀重建一棵輕量的樹（所以不能在裡面做重活）。
- **練習**：對照 `TickTape::render` 和 WPF 的 `<DockPanel>`／`<Grid>`：標頭、欄名、清單、提示列各對應什麼。改欄寬（`COLS` 常數）看效果。
- **自我檢查**：為什麼 `render` 裡的 `format!` 只出現在 `render_rows`（可見列），而不是在 `apply` 時預先建好全部字串？
- **連結**：本機 `gpui-pre-0.3.7/examples/hello_world.rs`、`data_table.rs`；[Zed：「像做電玩一樣做 UI」](https://zed.dev/blog/videogame)

### 3.3 🟢 `uniform_list` 虛擬化、🔵 快取圖層

- **為什麼對 XQ 重要**：5 萬列報價表、5000 筆成交明細都只畫可見的十幾列；GPUI 加 5 萬列只多 ≈ 4.5 MB。走勢圖則是三個 `.cached()` 圖層，滑鼠移動只重畫十字線那層（重圖表壓測：GPUI 每幀 CPU 組圖 3 ms、RSS 220 MB）。
- **練習**：
  1. 🟢 讀 `src/table.rs` `render_rows` 和 `src/tick_tape.rs` `render_rows`：`range` 是誰給的？一次多大？
  2. 🟢 `XQ_NO_VIEW_CACHE=1` 跑一次，比較狀態列的 FPS 與「走勢圖重畫」次數。
  3. 🔵 讀 `src/chart.rs` 開頭的三層（`PlotLayer`／`DrawLayer`／`LiveLayer`）與 `refresh(f)` 的旗標；說明 tick 進來時哪幾層會重畫、平移時畫線層為什麼不重建（`model.rs` `draw_layer_stale`）。
- **自我檢查**：`uniform_list` 為什麼要求每列等高？不等高要用什麼？（`list`，成本較高）
- **連結**：[`uniform_list.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/uniform_list.rs)、[範例 uniform_list.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/uniform_list.rs)、🔵 [`view.rs`（cached view）](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/view.rs)、[`canvas.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/canvas.rs)

### 3.4 🟢 Action、快捷鍵、焦點

- **為什麼對 XQ 重要**：**「不動使用者習慣」的核心就是快捷鍵和焦點行為**。XQ 老用戶靠鍵盤操作，WPF 版的每個快捷鍵都要在這裡對齊，而且要寫進行為基準。
- **練習**：
  1. 讀 `src/tick_tape.rs`：`actions!` → `bind_keys`（綁在 `KEY_CONTEXT`）→ render 裡 `key_context` + `track_focus` + `on_action`。按鈕的 `on_click` 也呼叫同一個 handler。
  2. 加一個 action `ClearTape`（`ctrl-l` 清空），焦點不在成交明細時按 `ctrl-l` 應該沒反應。
  3. 對照 `src/chart.rs` `on_key`（用 `on_key_down` 自己判斷按鍵）—— 舊寫法，新面板一律用 action，才能集中管理、之後做「自訂快捷鍵」。
- **自我檢查**：同一個鍵綁在兩個 context，焦點在巢狀的兩層裡時誰先收到？（讀 key_dispatch.md）
- **連結**：本機 `gpui-pre-0.3.7/docs/key_dispatch.md`（[Zed main](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/key_dispatch.md)）、範例 [`focus_visible.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/focus_visible.rs)、[`input.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/input.rs)

### 3.5 🔵 去哪裡讀 Zed 原始碼

GPUI 文件少，**Zed 本身就是最大的範例庫**：

| 想學 | 讀這裡 |
|---|---|
| 大清單＋虛擬化＋鍵盤導覽 | [`crates/project_panel`](https://github.com/zed-industries/zed/tree/main/crates/project_panel)、[`crates/outline_panel`](https://github.com/zed-industries/zed/tree/main/crates/outline_panel) |
| 分割窗格、分頁、拖放 | [`crates/workspace/src/pane.rs`](https://github.com/zed-industries/zed/blob/main/crates/workspace/src/pane.rs) |
| 元件庫寫法（按鈕、選單、tooltip） | [`crates/ui`](https://github.com/zed-industries/zed/tree/main/crates/ui) |
| 視窗、幀排程、`on_next_frame` | [`crates/gpui/src/window.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/window.rs) |
| 測試 GPUI view | [`examples/testing.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/testing.rs)（需要 `test-support` feature） |

🔵 另外要負責：GPUI 升版評估（鎖版本、在 `src/` 外包一層自己的元件，升版只改那層）、Windows／macOS 實機與「無 GPU」（遠端桌面／VDI）環境的效能驗證、輸入法與字型。

---

## 第 4 週：照範例做一個面板（畢業專題）

- **題目（任選）**：漲跌幅排行、自選股分類切換列、五檔報價（引擎有五檔封包前先用假資料）、分價量表。
- **要求**（就是 `ai-dev-guidelines.md` 的完整流程）：
  1. 寫規格：WPF 版截圖＋操作說明＋快捷鍵＋格式＋效能預算（模板：`prompt-templates.md` §1）。
  2. 純邏輯放 `crates/xq-<名稱>/`，先寫行為基準 JSON 與單元測試，再寫實作。
  3. GPUI view 放 `src/<名稱>.rs`，照 `src/tick_tape.rs` 的五個【模式】。
  4. `cargo clippy … -D warnings`（新程式碼）、`cargo test --workspace`、`cargo bench -p xq-<名稱>` 全過。
  5. 發 PR，附截圖、bench 數字、行為基準差異分類；另一位同事用 `ai-dev-guidelines.md` 的審查清單審。
- **自我檢查（畢業標準）**：
  - 能說出自己面板的每段程式碼跑在哪個執行緒。
  - 每秒 10 萬筆報價時，你的面板記憶體上限是多少？怎麼證明？
  - 你的面板跟 WPF 版有哪些不同？各屬於「刻意／bug／海外版差異」哪一類？

---

## C# → Rust／GPUI 對照表

| C# / WPF | Rust / GPUI | 本 repo 例子 | 注意 |
|---|---|---|---|
| `class`（參考型別） | `struct` + `impl` | `TickTape`、`QuoteTable` | 預設是值，所有權只有一個；要共用用 `Arc`／`Rc`，要在 GPUI 共用用 `Entity<T>` |
| `struct`（值型別） | `#[derive(Clone, Copy)] struct` | `xq_tape::Trade` | 只有數字的小 struct 用 `Copy`，熱路徑傳值零配置 |
| `interface` | `trait` | `Render`、`Focusable` | 可以為別人的型別實作；沒有繼承 |
| 泛型 `where T : struct` | `T: Copy` 等 trait bound | `Ring<T: Copy>` | 編譯期展開（單態化），沒有 boxing |
| `enum` + `switch` | `enum` + `match` | `Side`、`Msg`、`SubKind` | enum 可帶資料；`match` 必須窮舉 |
| `null` / `Nullable<T>` | `Option<T>` | `Ring::newest` | 沒有 null；`?`／`let … else` 提早返回 |
| `throw` / `try-catch` | `Result<T, E>` + `?` | `save_settings` | 不要 `unwrap()` 在正式路徑；`panic` = 程式結束 |
| `IDisposable` / `using` | `Drop`（自動） | `Task` 存在 `_tasks`，view 被丟掉時自動取消 | 不用手動 Dispose |
| `async Task` / `await` | `async` + `.await`；GPUI `cx.spawn` | `TickTape::new` 讀檔 | GPUI 的 `cx.spawn` 跑在 UI 執行緒；背景要 `background_executor().spawn` |
| `Task.Run(() => …)` | `cx.background_executor().spawn(async move { … })` 或 `std::thread::spawn` | `save_settings`、`store.rs` `save_async` | 背景裡不能碰 Entity／UI |
| `Dispatcher.BeginInvoke` | 背景寫共享緩衝 + UI 每幀 `take()`；或 `this.update(cx, …)` | `bridge.rs` `Inbox`、`TapeFeed` | **不要每筆 invoke 一次**，一幀最多一次 |
| `lock (obj)` | `Mutex<T>`（`parking_lot`） | `TapeShared` | 鎖保護的是**資料**，不是程式碼；鎖裡不做 I/O |
| `Interlocked` / `volatile` | `AtomicU64`／`AtomicBool` | `bridge.rs` `Counters`、`pending` | |
| `ConcurrentQueue<T>` | `std::sync::mpsc`、`futures::channel::mpsc` | `xq-feed` 訂閱指令、喚醒訊號 | |
| `event` / `EventHandler` | `EventEmitter<E>` + `cx.emit` + `cx.subscribe` | `TableEvent::Select` | 訂閱回傳 `Subscription`，要存起來，丟掉就取消訂閱 |
| `INotifyPropertyChanged` | 改欄位後 `cx.notify()` | `QuoteTable::apply` | 沒有自動通知；只在真的有變時 notify |
| WPF Binding / `ObservableCollection` | 沒有 binding；`render()` 直接讀欄位 | `TickTape::render` | 「狀態 → 畫面」是單向函式，比 binding 好除錯 |
| XAML 版面 | `div().flex().flex_col()…`（類 Tailwind／flexbox） | 所有 `render` | 每次 notify 重建，所以要輕 |
| `VirtualizingStackPanel` | `uniform_list` | `table.rs`、`tick_tape.rs` | 列高要固定 |
| `ICommand` / `RoutedCommand` | `actions!` + `on_action` | `ToggleBigOnly` | |
| `InputBindings` / `KeyBinding` | `cx.bind_keys([KeyBinding::new("b", …, Some("TickTape"))])` | `tick_tape::bind_keys` | 綁在 key context，焦點在該區才生效 |
| `Focusable` / `FocusManager` | `FocusHandle` + `track_focus` + `window.focus(…)` | `TickTape` 點擊取得焦點 | |
| `UserControl` | 一個 `Entity<T>` view，用 `.cached()` 掛進父 view | `main.rs` `view(…)` | cached：自己沒 notify 就不重畫 |
| `OnRender` / `DrawingContext` | `canvas(…)` + `window.paint_*` | `chart.rs` 三個圖層、`gfx.rs` | |
| NuGet / `.csproj` | crates.io / `Cargo.toml` | workspace 三個 crate | 版本鎖 `=0.3.7` |
| xUnit / NUnit | `#[test]`、`tests/*.rs` | `xq-tape/tests/baseline.rs` | 純邏輯 crate 不開視窗就能測 |
| BenchmarkDotNet | `benches/*.rs`（`harness = false`）或 criterion | `xq-tape/benches/tape.rs` | 寫上預算 |
| Roslyn analyzers | `clippy` | — | `-D warnings` 擋在 CI |
