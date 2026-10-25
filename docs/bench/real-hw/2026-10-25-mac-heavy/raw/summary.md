# XQ 壓測結果（macOS）：GPUI vs egui

> 這份是**在這台 mac 上實際量到的數字**（3 輪取中位數）。🏆＝該項最好，或和最好的差距在 5% 以內（可以多個 🏆）；全部 app 都在 5% 內才算平手（＝）。勝負項數：每個 🏆 各算一項。

## 結論

- 共比較 30 項：egui（glow／OpenGL） 勝 10 項、GPUI（Metal） 勝 8 項、egui（wgpu／Metal） 勝 3 項、GPUI 未修記憶體（對照） 勝 1 項、平手 12 項。
- **重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）**：egui（glow／OpenGL） 較好（10/30 項）
- 註：螢幕最高更新率 60 Hz；FPS 到這個數字就是頂了（兩邊都到頂＝平手）。

## 這台機器

- 機型：mac16,12
- CPU：Apple M4
- 邏輯核心：10
- 記憶體 GB：24.0
- macOS：27.0.1 (26A434)
- 架構：arm64
- Rosetta 轉譯中：False
- 低耗電模式：False
- 電源：["Now drawing from 'AC Power'", ' -InternalBattery-0 (id=…)\t100%; charged; 0:00 remaining present: true']
- GPU：Apple M4 10 核心，Metal spdisplays_metal4
- 螢幕 1：內建Retina顯示器，1470x956 點 ×2 倍＝2940x1912 像素，最高 60 Hz（主螢幕）
- 編譯：{"gpui_rev": "70c249c", "gpui_branch": "mac-bench", "shaders": "precompiled", "targets": "universal", "xcode": "Xcode 26.6", "built": "2026-10-25T05:58:06Z", "ci": true, "egui_rev": "b9d07c1", "egui_branch": "mac-bench", "egui_targets": "arm64"}

## 重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）

| 項目 | GPUI（Metal） | GPUI 未修記憶體（對照） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|---|
| 啟動到第一個重圖表幀 | 602.1 ms ＝ | 599.3 ms ＝ | 575.6 ms ＝ | 591.2 ms ＝ |
| ticks：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.8 fps ＝ |
| ticks：每幀圖表繪製 | 2.3 ms ＝ | 2.1 ms ＝ | 1.6 ms ＝ | 1.3 ms ＝ |
| ticks：CPU | 32 % | 31.1 % | 35.6 % | 25.3 % 🏆 |
| ticks：RSS | 115.2 MB 🏆 | 321.2 MB | 131.5 MB | 134 MB |
| crosshair：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.9 fps ＝ |
| crosshair：每幀圖表繪製 | 2.4 ms | 2 ms 🏆 | 1.6 ms 🏆 | 1.3 ms 🏆 |
| crosshair：CPU | 35.6 % | 29.7 % | 33.8 % | 24.6 % 🏆 |
| crosshair：RSS | 117.2 MB 🏆 | 321.5 MB | 133.7 MB | 136.2 MB |
| pan：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.8 fps ＝ |
| pan：每幀圖表繪製 | 1.5 ms ＝ | 1.4 ms ＝ | 1.1 ms ＝ | 0.9 ms ＝ |
| pan：CPU | 32.6 % | 27.4 % | 28.5 % | 20.4 % 🏆 |
| pan：RSS | 120.8 MB 🏆 | 329 MB | 134.4 MB | 137.6 MB |
| drag：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.6 fps ＝ |
| drag：每幀圖表繪製 | 1.5 ms ＝ | 1.3 ms ＝ | 0.8 ms ＝ | 0.7 ms ＝ |
| drag：CPU | 30.7 % | 26.9 % | 25.2 % | 17.7 % 🏆 |
| drag：RSS | 120.8 MB 🏆 | 325.7 MB | 134.2 MB | 138.3 MB |
| select：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.7 fps ＝ |
| select：每幀圖表繪製 | 2.2 ms | 1.9 ms | 1.1 ms 🏆 | 0.8 ms 🏆 |
| select：CPU | 31.5 % | 28.1 % | 26.4 % | 17.4 % 🏆 |
| select：RSS | 121.2 MB 🏆 | 326.9 MB | 134.8 MB | 140.9 MB |
| reorder：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62 fps ＝ |
| reorder：每幀圖表繪製 | 2.1 ms | 1.9 ms | 1.1 ms 🏆 | 0.8 ms 🏆 |
| reorder：CPU | 30.9 % | 26.4 % | 26.2 % | 16.5 % 🏆 |
| reorder：RSS | 121.4 MB 🏆 | 326.9 MB | 137.1 MB | 141 MB |
| zshot：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 61.5 fps ＝ |
| zshot：每幀圖表繪製 | 0.2 ms ＝ | 0.2 ms ＝ | 0.1 ms ＝ | 0.1 ms ＝ |
| zshot：CPU | 16.5 % | 15.8 % | 12.7 % | 9.8 % 🏆 |
| zshot：RSS | 121.4 MB 🏆 | 327 MB | 133.8 MB | 141.1 MB |
| 實體記憶體（結束時） | 158.5 MB 🏆 | 370.4 MB | 189.5 MB | 203.3 MB |

## 停頓診斷（GPUI；2026-10-25 新增）

- GPUI 版有一個背景看門狗：主執行緒超過 120 ms 沒動就記一筆 STALL。**整個程序被暫停**＝看門狗自己也停了（通常是壓測包自己的 vmmap 量記憶體，在量測區間外、不影響表格數字）；**只有主執行緒卡住**＝背景收行情照常、畫面停住（這才是要找的問題）。
- 只有主執行緒卡住時，會自動用 macOS 內建的 sample 記下卡住當下的呼叫堆疊（stalls 資料夾），請一起傳回。

| 情境 | app | 停頓次數（3 輪合計） | 壓測包 vmmap 造成 | 只有主執行緒卡住 | 最長 | 卡住前的階段 |
|---|---|---|---|---|---|---|
| 重圖表 | GPUI（Metal） | 0 | 0 | 0 | — | — |
| 重圖表 | GPUI 未修記憶體（對照） | 0 | 0 | 0 | — | — |

## 量法

- 情境同 Linux／Windows 版（scripts/bench-*.sh、bench/windows）。主視窗／5 萬列／重圖表的行情來自 xq-demofeed（WryFeedHost --demo-ticks 的 Rust 移植，合成資料）；高頻與多視窗來自 xq-tickgen。
- RSS＝ps 的常駐記憶體；實體記憶體＝vmmap 的 Physical footprint（≈ 活動監視器「記憶體」欄，只在量測區間外取）。CPU：100%＝一顆核心。
- GPU＝ioreg 的整機 GPU 使用率（含其他程式），只當參考。FPS 是程式自己回報的每秒畫面數。
- 高頻情境沒有做「點報價列切換」的反應時間（mac 上要自動點擊得開「輔助使用」權限，這版先不做）。
- 視窗位置由程式自己依 XQ_WIN_RECTS 開在指定螢幕，不需要任何權限；多視窗 ×4：螢幕夠就一個螢幕一個，不夠就平均分配後格狀鋪滿。
