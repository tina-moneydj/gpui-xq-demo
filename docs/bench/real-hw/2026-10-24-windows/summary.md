# Windows 實測：GPUI vs egui（Dell Inspiron 3020，2026-10-23 21:32–22:24 台北時間）

[← 總覽與診斷](../README.md)

## 機器

| 項目 | 值 |
|---|---|
| 電腦 | Dell Inspiron 3020 桌機（win-i7-13700） |
| CPU | Intel Core i7-13700，16 核／24 執行緒 |
| 記憶體 | 15.7 GB |
| 系統 | Windows 11 Home 25H2（build 26200.9711） |
| 顯示 | 偵測到 Intel UHD Graphics 770（內顯）和 NVIDIA GeForce GTX 1650（入門獨顯）；**螢幕模式 1920×1200@59Hz 掛在 GTX 1650**，Intel 那張沒有顯示模式。Tina 說是內顯，實際由哪張卡繪圖**待確認** |
| 螢幕 | 2 個 1920×1200，縮放 100%，59 Hz |
| 電源 | 插電，「平衡」電源計畫 |
| 版本 | GPUI 958bc22（windows-bench-ci，含 10-23 優化 4ec0a12）；egui edf20a9（windows-bench-ci）；CI 編譯 |
| 繪圖後端 | GPUI＝DirectX 11；egui＝**只有 glow（OpenGL）**，沒有測 wgpu |

## 結論先看

- **延遲打平**：單一視窗每秒 1 千～10 萬筆＋開盤爆量，兩邊延遲都約 31 ms（p50）／34 ms（p99），0 掉筆，60 FPS。
- **egui 勝大多數資源項目**：CPU GPUI 是 egui 的 1.7–2.3 倍、GPU 3D 使用率 2.3–5 倍、GPU 專用記憶體 2.3–2.8 倍；開啟 GPUI 316 vs egui 192 ms。
- **4 視窗（兩個螢幕）**：GPUI 延遲 p50 43／p99 64.3 ms、57–58 FPS；egui 21.8／33.7 ms、60 FPS。
- **5 萬列捲動**：GPUI 51.7 FPS、卡頓 70 次；egui 60 FPS、0 次。
- **GPUI 勝**：單視窗 Working Set 略省（Private 24.9 vs 38.4 MB）、每秒 10 萬筆和開盤爆量時的最大延遲較低（35 vs 48–49 ms）。
- **重圖表：壓測包逾時，3 輪都沒有正式數字**（我們壓測包的錯，見總覽）；從 log 取的前 4 段兩邊都約 60 FPS。
- 比分（差距 5% 內算平手）：GPUI 13 勝／egui 33 勝／35 平。

## 詳細數據（3 輪中位數）

### 主視窗

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 啟動到第一筆報價 | 315.6 ms | **191.9 ms** 🏆 | egui |
| 啟動到第一幀 | 295.2 ms | **190.5 ms** 🏆 | egui |
| Working Set 60 秒 | 74.1 MB | 77.5 MB | 平手 |
| Working Set-Private 60 秒 | **24.9 MB** 🏆 | 38.4 MB | GPUI |
| Private Bytes 60 秒 | 93.4 MB | **66.4 MB** 🏆 | egui |
| CPU | 13.3 % | **8 %** 🏆 | egui |
| GPU 3D | 11.2 % | **2.2 %** 🏆 | egui |
| GPU 專用記憶體 | 40.5 MB | **17.7 MB** 🏆 | egui |
| FPS（越高越好） | 39.7 fps | 40.1 fps | 平手 |
| 執行緒 | 55 個 | **12 個** 🏆 | egui |

### 5 萬列報價表

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| CPU 跳價 | 19.3 % | **10.5 %** 🏆 | egui |
| CPU 捲動 | 20.3 % | **8.9 %** 🏆 | egui |
| GPU 3D 跳價 | 7.6 % | **2.8 %** 🏆 | egui |
| FPS 跳價（越高越好） | 45 fps | **60 fps** 🏆 | egui |
| FPS 捲動（越高越好） | 51.7 fps | **60 fps** 🏆 | egui |
| 捲動卡頓次數 | 70 次 | **0 次** 🏆 | egui |
| Working Set 跳價末 | **78.4 MB** 🏆 | 83.6 MB | GPUI |
| 表格重畫耗時 跳價 | 0.1 ms | 0.2 ms | 平手 |

### 高頻進價 s1k

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50 | 31 ms | 31.1 ms | 平手 |
| 延遲 p99 | 33.9 ms | 33.9 ms | 平手 |
| 延遲最大 | 46.6 ms | 46.4 ms | 平手 |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 20.8 % | **8.9 %** 🏆 | egui |
| GPU 3D | 7.4 % | **3 %** 🏆 | egui |
| FPS（越高越好） | 60 fps | 60 fps | 平手 |
| Working Set 結束 | **80.4 MB** 🏆 | 87.6 MB | GPUI |
| 點選切換 p50（egui 多數點擊沒對到，僅參考） | **29.8 ms** 🏆 | 39.6 ms | GPUI |

### 高頻進價 s10k

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50 | 31 ms | 31.2 ms | 平手 |
| 延遲 p99 | 33.9 ms | 33.9 ms | 平手 |
| 延遲最大 | 47.9 ms | 48.8 ms | 平手 |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 22.4 % | **10.8 %** 🏆 | egui |
| GPU 3D | 7.3 % | **3.2 %** 🏆 | egui |
| FPS（越高越好） | 60.5 fps | 60 fps | 平手 |
| Working Set 結束 | **80.8 MB** 🏆 | 87.6 MB | GPUI |
| 點選切換 p50（egui 多數點擊沒對到，僅參考） | **29.8 ms** 🏆 | 38 ms | GPUI |

### 高頻進價 s50k

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50 | 31.3 ms | 31.3 ms | 平手 |
| 延遲 p99 | 34 ms | 33.9 ms | 平手 |
| 延遲最大 | 45.9 ms | 48.1 ms | 平手 |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 24.5 % | **12.4 %** 🏆 | egui |
| GPU 3D | 7.3 % | **2.9 %** 🏆 | egui |
| FPS（越高越好） | 60 fps | 60 fps | 平手 |
| Working Set 結束 | **80.8 MB** 🏆 | 87 MB | GPUI |
| 點選切換 p50（egui 多數點擊沒對到，僅參考） | **31.4 ms** 🏆 | 38 ms | GPUI |

### 高頻進價 s100k

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50 | 31.5 ms | 31.5 ms | 平手 |
| 延遲 p99 | 34.1 ms | 33.9 ms | 平手 |
| 延遲最大 | **35 ms** 🏆 | 48.4 ms | GPUI |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 25.2 % | **13.6 %** 🏆 | egui |
| GPU 3D | 7.4 % | **3.1 %** 🏆 | egui |
| FPS（越高越好） | 60.5 fps | 60 fps | 平手 |
| Working Set 結束 | **80.6 MB** 🏆 | 88 MB | GPUI |
| 點選切換 p50（egui 多數點擊沒對到，僅參考） | 32 ms | 33.1 ms | 平手 |

### 高頻進價 burst

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50 | 31.1 ms | 31.1 ms | 平手 |
| 延遲 p99 | 33.9 ms | 33.9 ms | 平手 |
| 延遲最大 | **35 ms** 🏆 | 49.2 ms | GPUI |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 21.9 % | **10.4 %** 🏆 | egui |
| GPU 3D | 7.2 % | **3 %** 🏆 | egui |
| FPS（越高越好） | 61 fps | 60 fps | 平手 |
| Working Set 結束 | **80.5 MB** 🏆 | 87.5 MB | GPUI |
| 點選切換 p50（egui 多數點擊沒對到，僅參考） | **33.1 ms** 🏆 | 35.9 ms | GPUI |

### 多視窗 ×1（每秒 1 萬筆）

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50（全部視窗） | 31 ms | 31 ms | 平手 |
| 延遲 p99（全部視窗） | 33.9 ms | 33.8 ms | 平手 |
| 最慢視窗 p99 | 33.9 ms | 33.8 ms | 平手 |
| 最慢視窗 FPS（越高越好） | 61 fps | 60 fps | 平手 |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 25.4 % | **15.2 %** 🏆 | egui |
| GPU 3D | 13.1 % | **2.6 %** 🏆 | egui |
| GPU 專用記憶體 | 79.2 MB | **32.2 MB** 🏆 | egui |
| Working Set 結束 | 86 MB | 88.4 MB | 平手 |

### 多視窗 ×4（每秒 1 萬筆）

| 項目 | GPUI（DirectX 11） | egui glow（OpenGL） | 勝出 |
|---|---:|---:|---|
| 延遲 p50（全部視窗） | 43 ms | **21.8 ms** 🏆 | egui |
| 延遲 p99（全部視窗） | 64.3 ms | **33.7 ms** 🏆 | egui |
| 最慢視窗 p99 | 67.1 ms | **35.1 ms** 🏆 | egui |
| 最慢視窗 FPS（越高越好） | 57 fps | 60 fps | 平手 |
| 遺失 | 0 筆 | 0 筆 | 平手 |
| CPU | 54.2 % | **25.3 %** 🏆 | egui |
| GPU 3D | 19.6 % | **5.1 %** 🏆 | egui |
| GPU 專用記憶體 | 161.1 MB | **57.6 MB** 🏆 | egui |
| Working Set 結束 | 112.9 MB | **90.4 MB** 🏆 | egui |

### 重圖表（壓測包逾時，以下取自程式自己的 log，只當參考）

| 段落 | GPUI FPS | egui FPS | GPUI 每幀繪圖 (ms) | egui 每幀繪圖 (ms) | GPUI RSS (MB) | egui RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| 跳價 | 59.95 | 59.95 | 2.24 | 1.63 | 100.45 | 78.0 |
| 十字線 | 59.9 | 60.0 | 2.3 | 1.71 | 100.5 | 78.7 |
| 平移 | 60.0 | 60.0 | 1.8 | 1.32 | 101.15 | 79.3 |
| 拖曳畫線 | 59.95 | 60.0 | 1.59 | 1.08 | 115.45 | 78.05 |