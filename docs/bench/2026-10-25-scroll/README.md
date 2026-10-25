# 5 萬列捲動卡頓：查原因與修正（2026-10-25 晚上，Linux 測試機）

## 結論先看

- **Windows 的卡頓主要不是畫報價表慢，是壓測「推捲動」的方式。** Windows 實測（[real-hw/2026-10-25-windows](../real-hw/2026-10-25-windows/README.md)）捲動 10 秒裡報價表只重畫 417 次（每秒約 42 次），每次只花 0.05 ms；畫面 51 FPS、卡頓 63 次、卡頓平均 32.8 ms（＝少一幀）。
  原因：GPUI 版用背景計時器「每 16 ms 推一次捲動位置」，Windows 計時器刻度是 15.6 ms（GPUI 原始碼 `platform_scheduler.rs` 也這樣註明），16 ms 常變成 31 ms，有些幀根本沒有新位置可以畫。egui 版是每一幀檢查一次，所以 60 FPS、0 次。
- **修正 1：捲動改成跟著畫面幀走**（每次 root render 開頭依經過時間算位置，再 `request_animation_frame` 要下一幀；跳價照 egui 版「距上次 ≥ 33 ms 才跳」）。舊做法保留：`XQ_SCROLL_DRIVER=timer`。
- **修正 2：字型找不到時不再每段字走錯誤路徑。** token 的 `Noto Sans CJK TC`／`Noto Sans Mono CJK TC` 不是每台都有（Windows 預設沒有）。找不到時 GPUI 每排一段字都會建錯誤物件、沿後援清單一個個找；有設 `RUST_BACKTRACE` 時每個錯誤還抓一次 backtrace——這台 perf 量到主執行緒約 7 成時間在 `_Unwind_Backtrace`，捲動只剩 24 FPS。現在啟動時查一次，找不到就直接用 GPUI 本來就會退到的那個字型（畫面一樣），stderr 印一行 `[fonts] …`。
- **GROUPPERF 新增欄位**：`frameMsP50/P95/P99/Max`（捲動中每幀間隔）、`frameSamples`、`scrollDriver`（frame／timer）、`scrollUpdatesPerSec`（報價表每秒拿到幾個新位置）。
- **Windows 還沒重跑**，這台沒有顯示卡、數字只看方向。真正要確認的是 Windows 上 `scrollUpdatesPerSec` 從約 42 變成約 60、卡頓變 0。

## 數字（這台：4 核、沒有顯示卡、Mesa lavapipe、Xvfb 1920×1200；交錯 3 輪，沒有設 RUST_BACKTRACE）

| 項目 | 修前（d44c086） | 只換字型（`XQ_SCROLL_DRIVER=timer`） | 全部（每幀推進） |
|---|---|---|---|
| 捲動 FPS（中位數；3 輪） | 57.4（57.4／56.1／57.8） | 55.5（53.3／55.5／56.6） | **58.6**（55.1／61.4／58.6） |
| 卡頓次數 >32 ms（中位數；3 輪） | 43（41／45／43） | 52（54／52／44） | **15**（38／5／15） |
| 幀間隔 p50／p95／p99（ms，中位數） | （舊版沒記） | 16.0／32.0／32.4 | 16.0／28.5／41.2 |

- 這台是軟體繪圖，一幀的時間本來就接近 60 Hz 的上限，所以「全部」還會有一些卡頓，而且輪跟輪差很多（5～38 次）。卡頓變少的方向在 3 輪裡都看得到（每輪都比修前少）。
- 「只換字型」那欄在這台沒有變好：沒設 `RUST_BACKTRACE` 時錯誤路徑本身不貴；Linux 計時器準，舊的推法在這台每秒也有約 56 次。
- **有設 `RUST_BACKTRACE=1` 時**（例如從開發用終端機跑）：修前 24.4 FPS、卡頓 179 次（`raw/before1.log`）→ 修後 56.5 FPS、24 次（`raw/a2-bt-frame.log`）。
- 第一幀時間沒變（修前 191／165 ms、修後 167／165 ms，各 2 次）。

## 怎麼測

- `run.sh`：`XQ_GROUP_STRESS=1`，跳價 3 秒（縮短）＋邊跳價邊捲動 10 秒，每秒 3000 筆，取 `GROUPPERF|{"phase":"done"…}`。原始輸出在 `raw/`（`r<輪>-before／timer／frame.log`）。
- Windows 重跑：用原本的壓測包即可，GROUPPERF 多出的欄位會在 log 裡；要對照舊做法加 `XQ_SCROLL_DRIVER=timer`。
