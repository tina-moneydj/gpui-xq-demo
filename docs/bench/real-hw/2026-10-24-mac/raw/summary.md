# XQ 壓測結果（macOS）：GPUI vs egui

> 這份是**在這台 Mac 上實際量到的數字**（3 輪取中位數）。🏆＝該項較好；差距 5% 以內算平手（＝）。

## 結論

- 共比較 123 項：GPUI（Metal） 勝 21 項、egui（glow／OpenGL） 勝 21 項、egui（wgpu／Metal） 勝 15 項、平手 66 項。
- **主視窗（1200×720 點、xq-demofeed 約 40Hz）**：GPUI（Metal） 較好（5/8 項）
- **5 萬列報價表（每秒 3000 筆跳價＋捲動）**：egui（glow／OpenGL） 較好（3/10 項）
- **重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）**：egui（glow／OpenGL） 較好（9/30 項）
- **高頻進價 s1k**：GPUI（Metal） 較好（2/11 項）
- **高頻進價 s10k**：GPUI（Metal） 較好（2/11 項）
- **高頻進價 s50k**：GPUI（Metal） 較好（2/11 項）
- **高頻進價 s100k**：egui（wgpu／Metal） 較好（3/11 項）
- **高頻進價 burst**：GPUI（Metal） 較好（3/11 項）
- **多視窗 ×1（每秒 1 萬筆）**：GPUI（Metal） 較好（2/10 項）
- **多視窗 ×4（每秒 1 萬筆）**：egui（wgpu／Metal） 較好（2/10 項）
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
- 編譯：{"gpui_rev": "b1db788", "gpui_branch": "mac-bench", "shaders": "precompiled", "targets": "universal", "xcode": "Xcode 26.6", "built": "2026-10-23T05:12:18Z", "ci": true, "egui_rev": "6c638c1", "egui_branch": "mac-bench", "egui_targets": "arm64"}

## 主視窗（1200×720 點、xq-demofeed 約 40Hz）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 啟動到第一幀 | 190.5 ms 🏆 | 214.5 ms | 213.2 ms |
| 啟動到第一筆報價 | 190.5 ms 🏆 | 246.4 ms | 247.6 ms |
| RSS 第 10 秒 | 101 MB 🏆 | 116.2 MB | 120.9 MB |
| RSS 第 60 秒 | 101.3 MB 🏆 | 116.3 MB | 121 MB |
| 實體記憶體（活動監視器）第 60 秒 | 148.5 MB 🏆 | 168.3 MB | 173.8 MB |
| CPU（第 30–60 秒） | 13.9 % | 12.8 % | 11.1 % 🏆 |
| FPS（第 30–60 秒）（越高越好） | 31.7 fps ＝ | 31.3 fps ＝ | 31.3 fps ＝ |
| 整機 GPU 使用率 | 9.8 % ＝ | 9 % ＝ | 12 % ＝ |

## 5 萬列報價表（每秒 3000 筆跳價＋捲動）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| CPU（跳價） | 19 % | 20.2 % | 15.9 % 🏆 |
| CPU（跳價＋捲動） | 25.5 % | 22.5 % | 7.6 % 🏆 |
| FPS（跳價）（越高越好） | 44.9 fps ＝ | 54.7 fps ＝ | 54.7 fps ＝ |
| FPS（捲動）（越高越好） | 59.8 fps | 60.1 fps | 83.2 fps 🏆 |
| 每次重畫表格耗時（跳價） | 0.1 ms ＝ | 0.4 ms ＝ | 0.3 ms ＝ |
| 每次重畫表格耗時（捲動） | 0.1 ms ＝ | 0.5 ms ＝ | 0.2 ms ＝ |
| 捲動卡頓次數 | 2 次 ＝ | 0 次 ＝ | 0 次 ＝ |
| RSS（跳價末） | 105.4 MB 🏆 | 122 MB | 126.3 MB |
| 實體記憶體（結束時） | 152.6 MB 🏆 | 174.2 MB | 186.4 MB |
| 整機 GPU（跳價） | 17.8 % | 16.7 % 🏆 | 21.1 % |

## 重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 啟動到第一個重圖表幀 | 607.6 ms ＝ | 613.6 ms ＝ | 592.6 ms ＝ |
| ticks：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 62.1 fps ＝ |
| ticks：每幀圖表繪製 | 1.9 ms ＝ | 1.8 ms ＝ | 1.4 ms ＝ |
| ticks：CPU | 29.7 % | 36.9 % | 25.6 % 🏆 |
| ticks：RSS | 321.3 MB ＝ | 130.8 MB ＝ | 131 MB ＝ |
| crosshair：FPS（越高越好） | 59.9 fps ＝ | 60 fps ＝ | 62.3 fps ＝ |
| crosshair：每幀圖表繪製 | 1.8 ms ＝ | 1.8 ms ＝ | 1.4 ms ＝ |
| crosshair：CPU | 28.3 % | 35.1 % | 23.8 % 🏆 |
| crosshair：RSS | 321.6 MB ＝ | 133.5 MB ＝ | 133.8 MB ＝ |
| pan：FPS（越高越好） | 60 fps | 60 fps | 63.2 fps 🏆 |
| pan：每幀圖表繪製 | 1.3 ms ＝ | 1.3 ms ＝ | 1 ms ＝ |
| pan：CPU | 27.1 % | 30.4 % | 21.3 % 🏆 |
| pan：RSS | 328.7 MB ＝ | 133.7 MB ＝ | 137 MB ＝ |
| drag：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 62 fps ＝ |
| drag：每幀圖表繪製 | 1.3 ms ＝ | 1 ms ＝ | 0.8 ms ＝ |
| drag：CPU | 30.2 % | 26.6 % | 18.5 % 🏆 |
| drag：RSS | 325.1 MB ＝ | 133.9 MB ＝ | 138 MB ＝ |
| select：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 62.7 fps ＝ |
| select：每幀圖表繪製 | 1.9 ms ＝ | 1.1 ms ＝ | 0.9 ms ＝ |
| select：CPU | 31.7 % | 26.7 % | 18.7 % 🏆 |
| select：RSS | 325.7 MB ＝ | 133.9 MB ＝ | 140.5 MB ＝ |
| reorder：FPS（越高越好） | 60 fps ＝ | 60 fps ＝ | 61.9 fps ＝ |
| reorder：每幀圖表繪製 | 1.8 ms ＝ | 1.2 ms ＝ | 0.9 ms ＝ |
| reorder：CPU | 31.1 % | 28.2 % | 17.8 % 🏆 |
| reorder：RSS | 325.8 MB ＝ | 134.8 MB ＝ | 140.7 MB ＝ |
| zshot：FPS（越高越好） | 60 fps | 60 fps | 64.4 fps 🏆 |
| zshot：每幀圖表繪製 | 0.2 ms ＝ | 0.2 ms ＝ | 0.1 ms ＝ |
| zshot：CPU | 17.1 % | 15.2 % | 9.2 % 🏆 |
| zshot：RSS | 325.8 MB | 131.5 MB 🏆 | 140.7 MB |
| 實體記憶體（結束時） | 366.2 MB | 187.2 MB 🏆 | 198.1 MB |

## 高頻進價 s1k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 30.6 ms ＝ | 30.8 ms ＝ | 30.4 ms ＝ |
| 延遲 p99 | 35.3 ms ＝ | 33.5 ms ＝ | 33.8 ms ＝ |
| 延遲最大 | 319.1 ms | 35.9 ms 🏆 | 39.8 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms ＝ | 0.2 ms ＝ | -0.2 ms ＝ |
| CPU | 29.8 % | 22.8 % | 6.1 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| RSS（結束） | 124.8 MB 🏆 | 140.4 MB | 141.9 MB |
| 實體記憶體（結束後） | 172 MB 🏆 | 190.4 MB | 197.2 MB |
| 整機 GPU | 23.8 % | 18.4 % 🏆 | 26.5 % |

## 高頻進價 s10k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 31.6 ms ＝ | 30.9 ms ＝ | 31.2 ms ＝ |
| 延遲 p99 | 35.2 ms ＝ | 33.8 ms ＝ | 33.5 ms ＝ |
| 延遲最大 | 356.2 ms ＝ | 36.2 ms ＝ | 36.1 ms ＝ |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms ＝ | -0 ms ＝ | -0.2 ms ＝ |
| CPU | 31.3 % | 26.1 % | 6.1 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| RSS（結束） | 122.9 MB 🏆 | 140.6 MB | 142.1 MB |
| 實體記憶體（結束後） | 170.1 MB 🏆 | 190.6 MB | 195 MB |
| 整機 GPU | 23.8 % | 16.5 % 🏆 | 26.5 % |

## 高頻進價 s50k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 32.1 ms ＝ | 30.7 ms ＝ | 31.3 ms ＝ |
| 延遲 p99 | 35.9 ms ＝ | 33.7 ms ＝ | 33.4 ms ＝ |
| 延遲最大 | 401.4 ms ＝ | 36.1 ms ＝ | 36.5 ms ＝ |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms ＝ | -0.2 ms ＝ | -0.2 ms ＝ |
| CPU | 33.6 % | 30.6 % | 7.4 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| RSS（結束） | 122.6 MB 🏆 | 140.9 MB | 142.5 MB |
| 實體記憶體（結束後） | 171.5 MB 🏆 | 192.9 MB | 195.3 MB |
| 整機 GPU | 23.8 % | 18.4 % 🏆 | 26.6 % |

## 高頻進價 s100k

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 32.7 ms ＝ | 31.2 ms ＝ | 32.1 ms ＝ |
| 延遲 p99 | 36.2 ms ＝ | 35.4 ms ＝ | 36.7 ms ＝ |
| 延遲最大 | 369 ms | 37.1 ms 🏆 | 39.4 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -0 ms | -1.4 ms 🏆 | -0.2 ms |
| CPU | 35.1 % | 35.2 % | 28.4 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 60.5 fps ＝ | 59 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 59 fps ＝ | 58 fps ＝ |
| RSS（結束） | 122.4 MB 🏆 | 140.9 MB | 144.5 MB |
| 實體記憶體（結束後） | 169.6 MB 🏆 | 192.9 MB | 196.5 MB |
| 整機 GPU | 23.8 % | 18.3 % 🏆 | 24.5 % |

## 高頻進價 burst

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50 | 31.3 ms ＝ | 31 ms ＝ | 30.7 ms ＝ |
| 延遲 p99 | 35.6 ms ＝ | 33.8 ms ＝ | 35 ms ＝ |
| 延遲最大 | 367.8 ms | 36.9 ms 🏆 | 44.8 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 積壓（後 5 秒−前 5 秒延遲） | -1.4 ms 🏆 | -0.1 ms | -0.2 ms |
| CPU | 31.6 % | 26.5 % | 7.1 % 🏆 |
| FPS 中位數（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| FPS 最低（越高越好） | 60 fps ＝ | 60 fps ＝ | 59 fps ＝ |
| RSS（結束） | 123.8 MB 🏆 | 140.8 MB | 142.9 MB |
| 實體記憶體（結束後） | 171 MB 🏆 | 192.8 MB | 196.3 MB |
| 整機 GPU | 23.8 % | 18.3 % 🏆 | 26.4 % |

## 多視窗 ×1（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50（全部視窗） | 31.4 ms ＝ | 31 ms ＝ | 31 ms ＝ |
| 延遲 p99（全部視窗） | 35.3 ms ＝ | 33.5 ms ＝ | 33.1 ms ＝ |
| 最慢視窗 p99 | 35.3 ms ＝ | 33.5 ms ＝ | 33.1 ms ＝ |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 最慢視窗 FPS（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| 平均 FPS（越高越好） | 61 fps ＝ | 61 fps ＝ | 61 fps ＝ |
| CPU | 31.8 % | 26 % | 5.5 % 🏆 |
| RSS（結束） | 122.5 MB 🏆 | 141.1 MB | 142.4 MB |
| 實體記憶體（結束後） | 192.1 MB 🏆 | 202.7 MB | 223 MB |
| 整機 GPU | 34.4 % | 20.7 % 🏆 | 30.1 % |

## 多視窗 ×4（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui（wgpu／Metal） | egui（glow／OpenGL） |
|---|---|---|---|
| 延遲 p50（全部視窗） | 44.8 ms | 24.4 ms | 17.6 ms 🏆 |
| 延遲 p99（全部視窗） | 49.6 ms ＝ | 33.6 ms ＝ | 33.4 ms ＝ |
| 最慢視窗 p99 | 50.1 ms | 34.8 ms 🏆 | 38.2 ms |
| 遺失 | 0 筆 ＝ | 0 筆 ＝ | 0 筆 ＝ |
| 最慢視窗 FPS（越高越好） | 61 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| 平均 FPS（越高越好） | 61 fps ＝ | 60 fps ＝ | 60 fps ＝ |
| CPU | 33.2 % | 44.1 % | 26 % 🏆 |
| RSS（結束） | 137.5 MB 🏆 | 147.5 MB | 150 MB |
| 實體記憶體（結束後） | 211 MB ＝ | 215.9 MB ＝ | 208 MB ＝ |
| 整機 GPU | 37.8 % | 32.3 % 🏆 | 38.3 % |

## 量法

- 情境同 Linux／Windows 版（scripts/bench-*.sh、bench/windows）。主視窗／5 萬列／重圖表的行情來自 xq-demofeed（WryFeedHost --demo-ticks 的 Rust 移植，合成資料）；高頻與多視窗來自 xq-tickgen。
- RSS＝ps 的常駐記憶體；實體記憶體＝vmmap 的 Physical footprint（≈ 活動監視器「記憶體」欄，只在量測區間外取）。CPU：100%＝一顆核心。
- GPU＝ioreg 的整機 GPU 使用率（含其他程式），只當參考。FPS 是程式自己回報的每秒畫面數。
- 高頻情境沒有做「點報價列切換」的反應時間（Mac 上要自動點擊得開「輔助使用」權限，這版先不做）。
- 視窗位置由程式自己依 XQ_WIN_RECTS 開在指定螢幕，不需要任何權限；多視窗 ×4：螢幕夠就一個螢幕一個，不夠就平均分配後格狀鋪滿。
