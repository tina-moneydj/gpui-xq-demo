# XQ 壓測結果（macOS）：GPUI vs egui

> 這份是**在這台 Mac 上實際量到的數字**（3 輪取中位數）。🏆＝該項最好，或和最好的差距在 5% 以內（可以多個 🏆）；全部 app 都在 5% 內才算平手（＝）。勝負項數：每個 🏆 各算一項。

## 結論

- 共比較 123 項：egui（glow／OpenGL） 勝 35 項、egui（wgpu／Metal） 勝 30 項、GPUI（Metal） 勝 24 項、GPUI quad 畫線（XQ_DRAW_QUADS=1） 勝 0 項、平手 56 項。
- **主視窗（1200×720 點、xq-demofeed 約 40Hz）**：GPUI（Metal） 較好（6/8 項）
- **5 萬列報價表（每秒 3000 筆跳價＋捲動）**：GPUI（Metal） 較好（3/10 項）
- **重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）**：egui（glow／OpenGL） 較好（17/30 項）
- **高頻進價 s1k**：GPUI（Metal） 較好（2/11 項）
- **高頻進價 s10k**：egui（glow／OpenGL） 較好（3/11 項）
- **高頻進價 s50k**：egui（wgpu／Metal） 較好（3/11 項）
- **高頻進價 s100k**：egui（wgpu／Metal） 較好（3/11 項）
- **高頻進價 burst**：GPUI（Metal） 較好（2/11 項）
- **多視窗 ×1（每秒 1 萬筆）**：GPUI（Metal） 較好（2/10 項）
- **多視窗 ×4（每秒 1 萬筆）**：egui（glow／OpenGL） 較好（3/10 項）
- 註：螢幕最高更新率 60 Hz；FPS 到這個數字就是頂了（兩邊都到頂＝平手）。

## 這台機器

- 機型：Mac16,12
- CPU：Apple M4
- 邏輯核心：10
- 記憶體 GB：24.0
- macOS：27.0.1 (26A434)
- 架構：arm64
- Rosetta 轉譯中：False
- 低耗電模式：False
- 電源：["Now drawing from 'AC Power'", ' -InternalBattery-0 (id=35520611)\t100%; charged; 0:00 remaining present: true']
- GPU：Apple M4 10 核心，Metal spdisplays_metal4
- 螢幕 1：內建Retina顯示器，1470x956 點 ×2 倍＝2940x1912 像素，最高 60 Hz（主螢幕）
- 編譯：{"gpui_rev": "11cc009", "gpui_branch": "mac-bench", "shaders": "precompiled", "targets": "universal", "xcode": "Xcode 26.6", "built": "2026-10-24T14:46:52Z", "ci": true, "egui_rev": "0d98298", "egui_branch": "mac-bench", "egui_targets": "arm64"}

## 主視窗（1200×720 點、xq-demofeed 約 40Hz）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 啟動到第一幀 | 152.5 ms 🏆 | 147.9 ms 🏆 | 177.4 ms |
| 啟動到第一筆報價 | 152.5 ms 🏆 | 186.3 ms | 218.7 ms |
| RSS 第 10 秒 | 100.5 MB 🏆 | 116.7 MB | 120.7 MB |
| RSS 第 60 秒 | 100.6 MB 🏆 | 116.8 MB | 120.9 MB |
| 實體記憶體（活動監視器）第 60 秒 | 147.5 MB 🏆 | 168.4 MB | 174.3 MB |
| CPU（第 30–60 秒） | 12.7 % | 12.8 % | 11.3 % 🏆 |
| FPS（第 30–60 秒）（越高越好） | 31.7 fps ＝ | 31.4 fps ＝ | 31.4 fps ＝ |
| 整機 GPU 使用率 | 7.6 % 🏆 | 6.9 % 🏆 | 10.4 % |

## 5 萬列報價表（每秒 3000 筆跳價＋捲動）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| CPU（跳價） | 16.9 % 🏆 | 20.7 % | 16.1 % 🏆 |
| CPU（跳價＋捲動） | 21 % | 22.6 % | 11.7 % 🏆 |
| FPS（跳價）（越高越好） | 45.9 fps | 54.1 fps 🏆 | 53.5 fps 🏆 |
| FPS（捲動）（越高越好） | 59.9 fps ＝ | 60.1 fps ＝ | 85.4 fps ＝ |
| 每次重畫表格耗時（跳價） | 0.1 ms ＝ | 0.4 ms ＝ | 0.4 ms ＝ |
| 每次重畫表格耗時（捲動） | 0.1 ms ＝ | 0.6 ms ＝ | 0.2 ms ＝ |
| 捲動卡頓次數 | 1 次 ＝ | 0 次 ＝ | 0 次 ＝ |
| RSS（跳價末） | 105.1 MB 🏆 | 122.6 MB | 126.3 MB |
| 實體記憶體（結束時） | 151.9 MB 🏆 | 174.5 MB | 185.8 MB |
| 整機 GPU（跳價） | 13.9 % | 11.6 % 🏆 | 17.8 % |

## 重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）

| 項目 | GPUI（Metal） | GPUI quad 畫線（XQ_DRAW_QUADS=1） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|---|
| 啟動到第一個重圖表幀 | 571.7 ms ＝ | 573.1 ms ＝ | 554.8 ms ＝ | 565.1 ms ＝ |
| ticks：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.3 fps ＝ |
| ticks：每幀圖表繪製 | 2.2 ms ＝ | 2.4 ms ＝ | 1.9 ms ＝ | 1.6 ms ＝ |
| ticks：CPU | 32.8 % | 38.9 % | 37.6 % | 29.6 % 🏆 |
| ticks：RSS | 321.2 MB | 328 MB | 130.7 MB 🏆 | 131.9 MB 🏆 |
| crosshair：FPS（越高越好） | 59.9 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.4 fps ＝ |
| crosshair：每幀圖表繪製 | 2 ms ＝ | 2.3 ms ＝ | 1.8 ms ＝ | 1.5 ms ＝ |
| crosshair：CPU | 28 % | 37.2 % | 34.5 % | 25.9 % 🏆 |
| crosshair：RSS | 321.4 MB | 328.4 MB | 133.5 MB 🏆 | 135 MB 🏆 |
| pan：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.5 fps ＝ |
| pan：每幀圖表繪製 | 1.4 ms ＝ | 1.3 ms ＝ | 1.4 ms ＝ | 1.1 ms ＝ |
| pan：CPU | 26.9 % | 35.6 % | 30.3 % | 22.9 % 🏆 |
| pan：RSS | 328.3 MB | 335.8 MB | 134 MB 🏆 | 136.2 MB 🏆 |
| drag：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62.7 fps ＝ |
| drag：每幀圖表繪製 | 1.3 ms ＝ | 1.2 ms ＝ | 1.1 ms ＝ | 0.8 ms ＝ |
| drag：CPU | 26.4 % | 35.1 % | 26.9 % | 20 % 🏆 |
| drag：RSS | 324.9 MB | 332.1 MB | 134.2 MB 🏆 | 136.9 MB 🏆 |
| select：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62 fps ＝ |
| select：每幀圖表繪製 | 1.9 ms 🏆 | 2.2 ms | 1.1 ms 🏆 | 0.9 ms 🏆 |
| select：CPU | 26.4 % | 35.5 % | 25.9 % | 18.6 % 🏆 |
| select：RSS | 325.5 MB | 332.6 MB | 134.3 MB 🏆 | 140.2 MB 🏆 |
| reorder：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62 fps ＝ |
| reorder：每幀圖表繪製 | 1.9 ms 🏆 | 2.2 ms | 1.1 ms 🏆 | 0.9 ms 🏆 |
| reorder：CPU | 26.3 % | 35.6 % | 25.9 % | 18.8 % 🏆 |
| reorder：RSS | 325.5 MB | 332.6 MB | 134.6 MB 🏆 | 140.2 MB 🏆 |
| zshot：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ | 62 fps ＝ |
| zshot：每幀圖表繪製 | 0.2 ms ＝ | 0.2 ms ＝ | 0.2 ms ＝ | 0.1 ms ＝ |
| zshot：CPU | 17 % | 17.7 % | 15.8 % | 10.1 % 🏆 |
| zshot：RSS | 325.5 MB | 332.6 MB | 134.2 MB 🏆 | 140.3 MB 🏆 |
| 實體記憶體（結束時） | 367.4 MB | 383.2 MB | 189.8 MB 🏆 | 198.2 MB 🏆 |

## 高頻進價 s1k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 31 ms ＝ | 30.8 ms ＝ | 31.1 ms ＝ |
| 延遲 p99 | 34.9 ms ＝ | 33.5 ms ＝ | 34.4 ms ＝ |
| 延遲最大 | 349.7 ms | 35 ms 🏆 | 41.9 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms ＝ | -0.2 ms ＝ | -0.2 ms ＝ |
| CPU | 25.7 % | 23.8 % | 8 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 59 fps ＝ |
| RSS（結束） | 124.3 MB 🏆 | 141.4 MB | 142.4 MB |
| 實體記憶體（結束後） | 171 MB 🏆 | 182.5 MB | 195.7 MB |
| 整機 GPU | 19.4 % | 12.2 % 🏆 | 22 % |

## 高頻進價 s10k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 31.7 ms ＝ | 30.5 ms ＝ | 31.3 ms ＝ |
| 延遲 p99 | 35.4 ms | 33.6 ms 🏆 | 33.4 ms 🏆 |
| 延遲最大 | 287.7 ms | 45.2 ms | 37 ms 🏆 |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | 0.1 ms ＝ | -0.1 ms ＝ | -0.2 ms ＝ |
| CPU | 27.3 % | 26 % | 7.6 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| RSS（結束） | 124.8 MB 🏆 | 141.5 MB | 145.2 MB |
| 實體記憶體（結束後） | 171.5 MB 🏆 | 191.3 MB | 199.1 MB |
| 整機 GPU | 19.6 % | 11.9 % 🏆 | 22.1 % |

## 高頻進價 s50k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 32.4 ms ＝ | 31.5 ms ＝ | 31 ms ＝ |
| 延遲 p99 | 36 ms | 33.6 ms 🏆 | 34.1 ms 🏆 |
| 延遲最大 | 348.3 ms | 35.2 ms 🏆 | 37.3 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | 0 ms ＝ | 0.1 ms ＝ | -0.2 ms ＝ |
| CPU | 31.3 % | 32.2 % | 9.9 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 59 fps ＝ |
| RSS（結束） | 124.4 MB 🏆 | 141.5 MB | 142.8 MB |
| 實體記憶體（結束後） | 171.1 MB 🏆 | 193.2 MB | 195 MB |
| 整機 GPU | 18.9 % | 11.9 % 🏆 | 21.4 % |

## 高頻進價 s100k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 33 ms ＝ | 31.4 ms ＝ | 32.2 ms ＝ |
| 延遲 p99 | 36.7 ms | 33.8 ms 🏆 | 37 ms |
| 延遲最大 | 330.2 ms | 36.8 ms 🏆 | 38.7 ms 🏆 |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | 0.1 ms ＝ | -0.2 ms ＝ | -0.2 ms ＝ |
| CPU | 33.9 % | 36.1 % | 27.3 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 58 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 59 fps ＝ | 57 fps ＝ |
| RSS（結束） | 122.6 MB 🏆 | 140.5 MB | 143.3 MB |
| 實體記憶體（結束後） | 169.4 MB 🏆 | 192.3 MB | 196.7 MB |
| 整機 GPU | 19.2 % | 11.9 % 🏆 | 20.2 % |

## 高頻進價 burst

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 31.3 ms ＝ | 30.6 ms ＝ | 31.3 ms ＝ |
| 延遲 p99 | 35.5 ms ＝ | 33.8 ms ＝ | 35.3 ms ＝ |
| 延遲最大 | 320.5 ms | 39.2 ms 🏆 | 37.3 ms 🏆 |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -1.1 ms ＝ | -0.4 ms ＝ | -0.2 ms ＝ |
| CPU | 28 % | 26.8 % | 9.4 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 59 fps ＝ |
| RSS（結束） | 124.9 MB 🏆 | 140.6 MB | 145.3 MB |
| 實體記憶體（結束後） | 171.6 MB 🏆 | 192.3 MB | 197.8 MB |
| 整機 GPU | 19.4 % | 12 % 🏆 | 21.9 % |

## 多視窗 ×1（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50（全部視窗） | 31.6 ms ＝ | 31.2 ms ＝ | 31.5 ms ＝ |
| 延遲 p99（全部視窗） | 35.2 ms ＝ | 33.6 ms ＝ | 34.8 ms ＝ |
| 最慢視窗 p99 | 35.2 ms ＝ | 33.6 ms ＝ | 34.8 ms ＝ |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 最慢視窗 FPS（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| 平均 FPS（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| CPU | 28.8 % | 26.9 % | 8.4 % 🏆 |
| RSS（結束） | 123.5 MB 🏆 | 140.4 MB | 145.4 MB |
| 實體記憶體（結束後） | 192.6 MB 🏆 | 201.7 MB 🏆 | 230.8 MB |
| 整機 GPU | 27.1 % | 14.8 % 🏆 | 25.5 % |

## 多視窗 ×4（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50（全部視窗） | 31.1 ms | 25.3 ms | 18 ms 🏆 |
| 延遲 p99（全部視窗） | 35.4 ms | 33.5 ms 🏆 | 33.4 ms 🏆 |
| 最慢視窗 p99 | 35.8 ms ＝ | 35.3 ms ＝ | 35.9 ms ＝ |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 最慢視窗 FPS（越高越好） | 61 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| 平均 FPS（越高越好） | 61 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| CPU | 32.7 % | 44.8 % | 24.6 % 🏆 |
| RSS（結束） | 135.8 MB 🏆 | 149.4 MB | 150.1 MB |
| 實體記憶體（結束後） | 211.1 MB ＝ | 217.2 MB ＝ | 207.9 MB ＝ |
| 整機 GPU | 32.4 % | 25.6 % 🏆 | 32.5 % |

## 量法

- 情境同 Linux／Windows 版（scripts/bench-*.sh、bench/windows）。主視窗／5 萬列／重圖表的行情來自 xq-demofeed（WryFeedHost --demo-ticks 的 Rust 移植，合成資料）；高頻與多視窗來自 xq-tickgen。
- RSS＝ps 的常駐記憶體；實體記憶體＝vmmap 的 Physical footprint（≈ 活動監視器「記憶體」欄，只在量測區間外取）。CPU：100%＝一顆核心。
- GPU＝ioreg 的整機 GPU 使用率（含其他程式），只當參考。FPS 是程式自己回報的每秒畫面數。
- 高頻情境沒有做「點報價列切換」的反應時間（Mac 上要自動點擊得開「輔助使用」權限，這版先不做）。
- 視窗位置由程式自己依 XQ_WIN_RECTS 開在指定螢幕，不需要任何權限；多視窗 ×4：螢幕夠就一個螢幕一個，不夠就平均分配後格狀鋪滿。
