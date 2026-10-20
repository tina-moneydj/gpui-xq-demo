# AGENTS.md — 給 AI 代理的專案規則

XQ 風格高頻看盤終端，Rust + GPUI 原生繪圖。XQNext（原 WPF）翻寫中，之後 XQ XT 共用底層。完整說明：`docs/team/ai-dev-guidelines.md`。
註解、文件、UI 文字用**繁體中文**；程式碼與識別字用英文。

## 建置與驗證（每次改完都要跑）

```bash
export PATH="$HOME/.cargo/bin:$PATH"     # stable，rustc ≥ 1.92（rust-toolchain.toml）
cargo build --release
cargo clippy --release --workspace --all-targets   # 新程式碼 0 警告；crates/* 用 -- -D warnings
cargo test --release --workspace
cargo bench -p xq-tape                              # 有熱路徑的 crate 都要有 bench
```

GUI 驗證（Linux 無 GPU 的 box）：`DISPLAY=:2 XQ_METRICS=1 ./target/release/gpui-xq-demo`；行情用 WryFeedHost 或 `./target/release/xq-tickgen --rate 2000 --rows 0`。WryFeedHost 只服務一個 client。結束程式用確切的 PID `kill <pid>`，不要 `pkill -f`。

## 結構

- `crates/xq-feed` 行情協議（零依賴）｜`crates/xq-tape` 成交明細純邏輯（**範例**）
- `src/bridge.rs` 背景合併層｜`src/main.rs` 根 view、`apply_feed`（每幀最多一次）
- `src/table.rs` 報價表（uniform_list）｜`src/chart.rs` + `model.rs`/`indicators.rs`/`drawings.rs`/`series.rs`/`gfx.rs` 走勢圖（三層 cached）
- `src/tick_tape.rs` 成交明細面板（**範例，新面板照它的五個【模式】寫**；說明 `docs/team/example-module.md`）
- `src/store.rs` 存檔｜`scripts/` 壓測腳本｜`docs/bench/` 壓測原始數據

## 必守規則

1. **UI 執行緒不做 I/O、不阻塞**。檔案／網路用 `cx.background_executor().spawn`。GPUI 的 `cx.spawn` 本身在 UI 執行緒。
2. **行情回呼／`bridge::merge` 只做 O(1)／筆**：不配置大物件、不 log、不 I/O、不碰 UI。
3. **UI 每幀最多套用一次行情**：背景寫共享緩衝，UI `take()`／`swap`。新面板掛在 `Terminal::apply_feed`，不自己開計時器輪詢。
4. 鎖裡只做 `swap`／`take` 等 O(1) 操作；熱路徑每包鎖一次，不要每筆鎖一次。
5. 報價可以同代號合併；成交、委託／成交回報**不能合併**→ 固定容量緩衝＋丟棄計數。
6. **所有會成長的容器要有上限**（`Ring::with_capacity`、LRU）。
7. **不在每幀／每筆配置**：`Copy` 小 struct、重用緩衝、字串只為可見列格式化。不要為了過編譯 `.clone()` String/Vec 或包 `Arc<Mutex<…>>`。
8. **增量計算**：tick 只改最後一根；指標保存遞迴狀態；整段重算只在換代號／週期／參數。
9. 大清單用 `uniform_list`；只在真的有變時 `cx.notify()`；子 view 用 `.cached()`。
10. 快捷鍵用 `actions!` + `KeyBinding`（綁 key context）+ `on_action`，不要 `on_key_down` 比字串。
11. **正式路徑不用 `unwrap()`／`expect()`／越界索引**；壞檔、壞封包、斷線要降級不當掉。
12. 存檔：防抖 → 背景寫 `.tmp` → `rename`；`#[serde(default)]`；`XQ_BENCH`／`XQ_TICK_BENCH`／`XQ_CHART_STRESS`／`XQ_GROUP_STRESS` 時不讀不寫。
13. 純邏輯放 `crates/`（不依賴 gpui，可測）；畫面放 `src/`。模組間只用公開函式與 `EventEmitter`。
14. 不新增依賴（要加需說明並經架構負責人同意）。不改 `crates/xq-feed` 協議格式（要與 wry 版／WryFeedHost 相容）。
15. 上限契約：主圖疊加 ≤ 20、副圖 ≤ 10。

## 做功能的順序

規格（含 WPF 行為與效能預算）→ **先寫**行為基準 JSON（`crates/<x>/tests/baseline/`）與單元測試 → 實作 → build/clippy/test/bench → 回報。
跟 WPF 不同的地方必須分類：**刻意／bug／海外版差異**，寫進基準檔 `known_diffs`。沒分類的差異當 bug。

## 效能預算（本機無 GPU 的回歸門檻；詳見 guidelines §5）

第一幀 ≤ 400 ms｜第一筆報價 ≤ 500 ms｜主視窗 RSS ≤ 220 MB｜重圖表 RSS ≤ 260 MB｜`apply_feed` ≤ 1 ms｜走勢圖每幀 CPU 組圖 ≤ 4 ms｜
10 萬筆／秒：掉筆 0、RSS 30 秒成長 ≤ 5 MB、延遲 p99 ≤ 125 ms｜每筆報價背景處理 ≤ 50 ns｜可見列格式化 ≤ 1 µs。

## 不要做

直接推 `main`（團隊模式）、`git push --force`、`--no-verify`、提交 `target/` 或使用者狀態檔、在壓測模式寫使用者狀態、改既有壓測的版面（新面板預設用環境變數關閉）。
