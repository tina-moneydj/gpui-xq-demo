
### 主視窗（1200×720 點、xq-demofeed 約 40Hz）

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 啟動到第一幀 | 190.5 ms | 214.5 ms | 213.2 ms | GPUI | GPUI |
| 啟動到第一筆報價 | 190.5 ms | 246.4 ms | 247.6 ms | GPUI | GPUI |
| RSS 第 10 秒 | 101 MB | 116.2 MB | 120.9 MB | GPUI | GPUI |
| RSS 第 60 秒 | 101.3 MB | 116.3 MB | 121 MB | GPUI | GPUI |
| 實體記憶體（活動監視器）第 60 秒 | 148.5 MB | 168.3 MB | 173.8 MB | GPUI | GPUI |
| CPU（第 30–60 秒） | 13.9 % | 12.8 % | 11.1 % | egui | egui |
| FPS（第 30–60 秒）（越高越好） | 31.7 fps | 31.3 fps | 31.3 fps | 平手 | 平手 |
| 整機 GPU 使用率 | 9.8 % | 9 % | 12 % | GPUI | egui |

### 5 萬列報價表（每秒 3000 筆跳價＋捲動）

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| CPU（跳價） | 19 % | 20.2 % | 15.9 % | egui | GPUI |
| CPU（跳價＋捲動） | 25.5 % | 22.5 % | 7.6 % | egui | egui |
| FPS（跳價）（越高越好） | 44.9 fps | 54.7 fps | 54.7 fps | egui | egui |
| FPS（捲動）（越高越好） | 59.8 fps | 60.1 fps | 83.2 fps | 平手 | 平手 |
| 每次重畫表格耗時（跳價） | 0.1 ms | 0.4 ms | 0.3 ms | 平手 | 平手 |
| 每次重畫表格耗時（捲動） | 0.1 ms | 0.5 ms | 0.2 ms | 平手 | 平手 |
| 捲動卡頓次數 | 2 次 | 0 次 | 0 次 | egui | egui |
| RSS（跳價末） | 105.4 MB | 122 MB | 126.3 MB | GPUI | GPUI |
| 實體記憶體（結束時） | 152.6 MB | 174.2 MB | 186.4 MB | GPUI | GPUI |
| 整機 GPU（跳價） | 17.8 % | 16.7 % | 21.1 % | GPUI | egui |

### 重圖表（20 疊加＋8 副圖＋1000 筆畫線／100 層）

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 啟動到第一個重圖表幀 | 607.6 ms | 613.6 ms | 592.6 ms | 平手 | 平手 |
| ticks：FPS（越高越好） | 60 fps | 60 fps | 62.1 fps | 平手 | 平手 |
| ticks：每幀圖表繪製 | 1.9 ms | 1.8 ms | 1.4 ms | egui | 平手 |
| ticks：CPU | 29.7 % | 36.9 % | 25.6 % | egui | GPUI |
| ticks：RSS | 321.3 MB | 130.8 MB | 131 MB | egui | egui |
| crosshair：FPS（越高越好） | 59.9 fps | 60 fps | 62.3 fps | 平手 | 平手 |
| crosshair：每幀圖表繪製 | 1.8 ms | 1.8 ms | 1.4 ms | 平手 | 平手 |
| crosshair：CPU | 28.3 % | 35.1 % | 23.8 % | egui | GPUI |
| crosshair：RSS | 321.6 MB | 133.5 MB | 133.8 MB | egui | egui |
| pan：FPS（越高越好） | 60 fps | 60 fps | 63.2 fps | 平手 | 平手 |
| pan：每幀圖表繪製 | 1.3 ms | 1.3 ms | 1 ms | 平手 | 平手 |
| pan：CPU | 27.1 % | 30.4 % | 21.3 % | egui | GPUI |
| pan：RSS | 328.7 MB | 133.7 MB | 137 MB | egui | egui |
| drag：FPS（越高越好） | 60 fps | 60 fps | 62 fps | 平手 | 平手 |
| drag：每幀圖表繪製 | 1.3 ms | 1 ms | 0.8 ms | egui | 平手 |
| drag：CPU | 30.2 % | 26.6 % | 18.5 % | egui | egui |
| drag：RSS | 325.1 MB | 133.9 MB | 138 MB | egui | egui |
| select：FPS（越高越好） | 60 fps | 60 fps | 62.7 fps | 平手 | 平手 |
| select：每幀圖表繪製 | 1.9 ms | 1.1 ms | 0.9 ms | egui | egui |
| select：CPU | 31.7 % | 26.7 % | 18.7 % | egui | egui |
| select：RSS | 325.7 MB | 133.9 MB | 140.5 MB | egui | egui |
| reorder：FPS（越高越好） | 60 fps | 60 fps | 61.9 fps | 平手 | 平手 |
| reorder：每幀圖表繪製 | 1.8 ms | 1.2 ms | 0.9 ms | egui | egui |
| reorder：CPU | 31.1 % | 28.2 % | 17.8 % | egui | egui |
| reorder：RSS | 325.8 MB | 134.8 MB | 140.7 MB | egui | egui |
| zshot：FPS（越高越好） | 60 fps | 60 fps | 64.4 fps | 平手 | 平手 |
| zshot：每幀圖表繪製 | 0.2 ms | 0.2 ms | 0.1 ms | 平手 | 平手 |
| zshot：CPU | 17.1 % | 15.2 % | 9.2 % | egui | egui |
| zshot：RSS | 325.8 MB | 131.5 MB | 140.7 MB | egui | egui |
| 實體記憶體（結束時） | 366.2 MB | 187.2 MB | 198.1 MB | egui | egui |

### 高頻進價 s1k

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50 | 30.6 ms | 30.8 ms | 30.4 ms | 平手 | 平手 |
| 延遲 p99 | 35.3 ms | 33.5 ms | 33.8 ms | 平手 | egui |
| 延遲最大 | 319.1 ms | 35.9 ms | 39.8 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms | 0.2 ms | -0.2 ms | egui | GPUI |
| CPU | 29.8 % | 22.8 % | 6.1 % | egui | egui |
| FPS 中位數（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| FPS 最低（越高越好） | 60 fps | 60 fps | 60 fps | 平手 | 平手 |
| RSS（結束） | 124.8 MB | 140.4 MB | 141.9 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 172 MB | 190.4 MB | 197.2 MB | GPUI | GPUI |
| 整機 GPU | 23.8 % | 18.4 % | 26.5 % | GPUI | egui |

### 高頻進價 s10k

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50 | 31.6 ms | 30.9 ms | 31.2 ms | 平手 | 平手 |
| 延遲 p99 | 35.2 ms | 33.8 ms | 33.5 ms | 平手 | 平手 |
| 延遲最大 | 356.2 ms | 36.2 ms | 36.1 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms | 0 ms | -0.2 ms | egui | GPUI |
| CPU | 31.3 % | 26.1 % | 6.1 % | egui | egui |
| FPS 中位數（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| FPS 最低（越高越好） | 60 fps | 60 fps | 60 fps | 平手 | 平手 |
| RSS（結束） | 122.9 MB | 140.6 MB | 142.1 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 170.1 MB | 190.6 MB | 195 MB | GPUI | GPUI |
| 整機 GPU | 23.8 % | 16.5 % | 26.5 % | GPUI | egui |

### 高頻進價 s50k

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50 | 32.1 ms | 30.7 ms | 31.3 ms | 平手 | 平手 |
| 延遲 p99 | 35.9 ms | 33.7 ms | 33.4 ms | egui | egui |
| 延遲最大 | 401.4 ms | 36.1 ms | 36.5 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 積壓（後 5 秒−前 5 秒延遲） | -0.1 ms | -0.2 ms | -0.2 ms | egui | egui |
| CPU | 33.6 % | 30.6 % | 7.4 % | egui | egui |
| FPS 中位數（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| FPS 最低（越高越好） | 60 fps | 60 fps | 60 fps | 平手 | 平手 |
| RSS（結束） | 122.6 MB | 140.9 MB | 142.5 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 171.5 MB | 192.9 MB | 195.3 MB | GPUI | GPUI |
| 整機 GPU | 23.8 % | 18.4 % | 26.6 % | GPUI | egui |

### 高頻進價 s100k

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50 | 32.7 ms | 31.2 ms | 32.1 ms | 平手 | 平手 |
| 延遲 p99 | 36.2 ms | 35.4 ms | 36.7 ms | 平手 | 平手 |
| 延遲最大 | 369 ms | 37.1 ms | 39.4 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 積壓（後 5 秒−前 5 秒延遲） | 0 ms | -1.4 ms | -0.2 ms | egui | egui |
| CPU | 35.1 % | 35.2 % | 28.4 % | egui | 平手 |
| FPS 中位數（越高越好） | 61 fps | 60.5 fps | 59 fps | 平手 | 平手 |
| FPS 最低（越高越好） | 60 fps | 59 fps | 58 fps | 平手 | 平手 |
| RSS（結束） | 122.4 MB | 140.9 MB | 144.5 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 169.6 MB | 192.9 MB | 196.5 MB | GPUI | GPUI |
| 整機 GPU | 23.8 % | 18.3 % | 24.5 % | 平手 | egui |

### 高頻進價 burst

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50 | 31.3 ms | 31 ms | 30.7 ms | 平手 | 平手 |
| 延遲 p99 | 35.6 ms | 33.8 ms | 35 ms | 平手 | egui |
| 延遲最大 | 367.8 ms | 36.9 ms | 44.8 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 積壓（後 5 秒−前 5 秒延遲） | -1.4 ms | -0.1 ms | -0.2 ms | GPUI | GPUI |
| CPU | 31.6 % | 26.5 % | 7.1 % | egui | egui |
| FPS 中位數（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| FPS 最低（越高越好） | 60 fps | 60 fps | 59 fps | 平手 | 平手 |
| RSS（結束） | 123.8 MB | 140.8 MB | 142.9 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 171 MB | 192.8 MB | 196.3 MB | GPUI | GPUI |
| 整機 GPU | 23.8 % | 18.3 % | 26.4 % | GPUI | egui |

### 多視窗 ×1（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50（全部視窗） | 31.4 ms | 31 ms | 31 ms | 平手 | 平手 |
| 延遲 p99（全部視窗） | 35.3 ms | 33.5 ms | 33.1 ms | egui | egui |
| 最慢視窗 p99 | 35.3 ms | 33.5 ms | 33.1 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 最慢視窗 FPS（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| 平均 FPS（越高越好） | 61 fps | 61 fps | 61 fps | 平手 | 平手 |
| CPU | 31.8 % | 26 % | 5.5 % | egui | egui |
| RSS（結束） | 122.5 MB | 141.1 MB | 142.4 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 192.1 MB | 202.7 MB | 223 MB | GPUI | GPUI |
| 整機 GPU | 34.4 % | 20.7 % | 30.1 % | egui | egui |

### 多視窗 ×4（每秒 1 萬筆）

| 項目 | GPUI（Metal） | egui wgpu（Metal） | egui glow（OpenGL） | GPUI vs egui-glow | GPUI vs egui-wgpu |
|---|---:|---:|---:|---|---|
| 延遲 p50（全部視窗） | 44.8 ms | 24.4 ms | 17.6 ms | egui | egui |
| 延遲 p99（全部視窗） | 49.6 ms | 33.6 ms | 33.4 ms | egui | egui |
| 最慢視窗 p99 | 50.1 ms | 34.8 ms | 38.2 ms | egui | egui |
| 遺失 | 0 筆 | 0 筆 | 0 筆 | 平手 | 平手 |
| 最慢視窗 FPS（越高越好） | 61 fps | 60 fps | 60 fps | 平手 | 平手 |
| 平均 FPS（越高越好） | 61 fps | 60 fps | 60 fps | 平手 | 平手 |
| CPU | 33.2 % | 44.1 % | 26 % | egui | GPUI |
| RSS（結束） | 137.5 MB | 147.5 MB | 150 MB | GPUI | GPUI |
| 實體記憶體（結束後） | 211 MB | 215.9 MB | 208 MB | 平手 | 平手 |
| 整機 GPU | 37.8 % | 32.3 % | 38.3 % | 平手 | egui |