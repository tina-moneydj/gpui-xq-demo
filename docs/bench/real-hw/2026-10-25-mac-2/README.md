# Mac 第三次（新版壓測包：停頓看門狗、記憶體拆解、不限速對照）：MacBook Air M4，2026-10-25 早上由 Tina 執行

[← 總覽與診斷](../README.md)｜[上一次（10-24 晚上）](../2026-10-25-mac/README.md)

> 同一台 Mac16,12（Apple M4、24 GB、macOS 27.0.1、內建螢幕 1470×956 點 ×2、60 Hz、插電）。10-25 10:14–11:29（台北時間），每項 3 輪取中位數，0 失敗。
> 版本：GPUI `948a6c7`、egui `b9d07c1`（mac-bench 分支，CI 編譯）。多了一欄「**GPUI 未修補（gpui-nofix）**」＝恢復 GPUI 預設的背景視窗限速，用來比「不限速」的效果。
> 完整表格（壓測包自動產生，🏆＝該項較佳）：[raw/summary.md](raw/summary.md)；原始檔：[raw/](raw/)、記憶體分區表 [raw/vmmap/](raw/vmmap/)（已去掉電腦名稱、使用者名稱、電池序號）。

## 結論先看

1. **4 視窗結果和上一次一樣，仍是「勉強通過」。** GPUI p50 31.1 ms、p99 **35.6 ms**（上次 35.4）、最慢視窗 36.4 ms（上次 35.8）；egui wgpu 33.6／34.5 ms、egui OpenGL 34.1／36.9 ms。
2. **0.5 秒停頓確定不是 GPUI 卡住。**
   - 看門狗 3 輪只記到 3 次停頓，全部在主視窗情境、全部是「整個程式被暫停」（壓測包自己跑 vmmap），**「只有主執行緒卡住」0 次**。所以沒有 `stalls/` 資料夾（沒東西要抓堆疊）。
   - 高頻「延遲最大」從上次的 **288–350 ms 降到 62–66 ms**（量測修正生效）。egui 是 35–41 ms，GPUI 還是比較高：最慢那一筆每輪都落在第 30.0 秒（報價剛停的那一刻），大部分時間花在「套用 → 畫面」33–68 ms，也就是多等了 1–3 幀。p50／p99 不受影響。
3. **「不限速」修補量不出差別**：gpui vs gpui-nofix，4 視窗 p99 35.6 vs 35.4 ms、最慢視窗 36.4 vs 36.2 ms、每秒 1 萬筆 p99 35.4 vs 35.5 ms，都在雜訊內。原因和預期一樣：我們的報價更新走 `notify`，本來就不受限速影響。修補留著當預防。
4. **重圖表記憶體（GPUI 321 vs egui 131 MB）的原因找到了，而且和之前猜的不一樣：不是 Metal／GPU，是 malloc 釋放後沒還給系統。**
   - vmmap（重圖表結束時）：GPUI 有一塊 **「Malloc Large (empty)」201 MB**（128 塊、已經 free、但還佔著實體記憶體）；egui 兩版都沒有。
   - 我們程式實際在用的 heap 只有 **18 MB**（`heapUseMb`，各段都一樣）。
   - GPU 相關的分區（IOSurface＋IOAccelerator＋owned unmapped (graphics)）三版差不多：GPUI 約 121 MB、egui wgpu 約 132 MB、egui OpenGL 約 124 MB。
   - 推論：重圖表每幀有一批大的暫存配置（每塊約 2–3 MB，可能是畫線／路徑的頂點緩衝），用完就 free；macOS 的 malloc 把這些大塊留著備用，沒有還給系統。Windows 和 Linux 沒有這個現象（Windows 重圖表 GPUI 105–128 MB）。
   - 可以在我們這層修，不用改 GPUI：重用這些緩衝，或定期呼叫 `malloc_zone_pressure_relief` 把空塊還回去。改完要再請 Tina 在 Mac 上量一次。
5. **其他和上次差不多**：一般畫面 GPUI 最省記憶體（RSS 100.5 vs 116.8–121 MB）；高頻 CPU GPUI 25–34%，和 egui wgpu（23–36%）差不多，egui OpenGL 只有 6–21%。

## 和上一次（10-24 晚上）比

| 項目 | 上次 GPUI | 這次 GPUI | 這次 GPUI 未修補 | 這次 egui wgpu | 這次 egui OpenGL |
|---|---:|---:|---:|---:|---:|
| 4 視窗延遲 p50 | 31.1 ms | 31.1 ms | 30.8 ms | 25.4 ms | **17.1 ms** 🏆 |
| 4 視窗延遲 p99 | 35.4 ms | 35.6 ms | 35.4 ms | **33.6 ms** 🏆 | **34.1 ms** 🏆 |
| 4 視窗最慢視窗 p99 | 35.8 ms | 36.4 ms | **36.2 ms** 🏆 | **34.5 ms** 🏆 | 36.9 ms |
| 4 視窗 CPU | 32.7 % | 32.5 % | 32.3 % | 43.1 % | **26.9 %** 🏆 |
| 每秒 1 萬筆 延遲最大 | 288–350 ms（量測假象） | 65 ms | 66.7 ms | **37.1 ms** 🏆 | **37.1 ms** 🏆 |
| 每秒 1 萬筆 CPU | 27.3 % | 27.1 % | 27.4 % | 25 % | **7.1 %** 🏆 |
| 重圖表 RSS（跳價） | 321.2 MB | 321.3 MB | — | **131.1 MB** 🏆 | **134.1 MB** 🏆 |
| 開啟到第一筆報價 | 152.5 ms | **188.5 ms** 🏆 | — | **187.5 ms** 🏆 | 207.9 ms |

- 開啟時間 152.5 → 188.5 ms 是機器狀態的雜訊（第 1 輪冷啟動 771 ms，後兩輪正常）。
- 總比分（壓測包計分）：egui OpenGL 34、egui wgpu 30、GPUI 25、GPUI 未修補 4、平手 54（共 123 項）。

## 重圖表記憶體：vmmap 拆解（MB，重圖表跑完時）

| 分區 | GPUI | egui wgpu | egui OpenGL | 說明 |
|---|---:|---:|---:|---|
| Malloc Large (empty) | **201.0** | 0 | 0 | 已 free、沒還給系統的大塊 |
| Malloc Small | 29.2 | 26.2 | 26.2 | 一般 heap |
| IOSurface | 40.3 | 26.9 | 65.9 | 視窗畫面緩衝（GPU） |
| IOAccelerator (graphics) | 13.6 | 4.8 | 7.5 | GPU 資源 |
| owned unmapped (graphics) | 66.8 | 100.1 | 50.7 | GPU 驅動配置 |
| 合計 dirty（≈ 實體記憶體） | 372.6 | 189.5 | 197.8 | |

- 程式自己回報：GPUI heap 在用 18.1 MB、向系統要了 64–68 MB（`heapUseMb`／`heapAllocMb`）。
- 注意：這版 Mac 壓測包的 vmmap 檔名少了情境名稱（都叫 `*-x-秒數.txt`），每輪會覆蓋上一輪，只留最後一輪。依秒數和檔案時間對應：`-11s`／`-61s`＝主視窗、`-42s`＝5 萬列、`-67s`＝重圖表、`-41s`＝多視窗。上表用 `-67s`。壓測包下一版會修。

## 下一步

1. 重圖表：在 Mac 上把那 201 MB 還回去，請 Tina 只跑 `heavy` 情境確認。**10-25 下午已修，等 Mac 重跑**：背景執行緒每 0.5 秒呼叫 `malloc_zone_pressure_relief`（`src/memrelief.rs`）。
   - 原因：這台 24 GB 的 Mac 沒開 malloc 的 medium 區，大於 small 上限的配置都各自 mmap；free 後進「death-row 快取」，最多 128 塊（正好是 vmmap 的 128 塊）。
   - 這些大塊是重圖表每幀的畫線 Path 頂點陣列。GPUI 的 `paint_path` 要拿走一份、內部 `scale()` 和 `Scene` 各再複製一次，Metal renderer 也會再組一次，所以無法只靠重用緩衝解決。
   - 壓測包新增對照欄 `gpui-norelief`（`XQ_MALLOC_RELIEF_MS=0`，只跑重圖表）；CHARTPERF 多了 `mallocReliefN`／`mallocReliefMb`（呼叫次數、估計還回去的量）。
2. GPUI 多視窗第 2–4 個視窗第一筆報價晚約 3 秒（Mac、Windows 都有），要查。
3. 壓測包修 vmmap 檔名。
