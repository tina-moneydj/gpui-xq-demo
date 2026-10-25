
## 網頁：一頁嵌多個元件（同一頁放 canvas 元件和一般 HTML）

- 瀏覽器：chrome（全新設定檔、沒有擴充功能）。Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/153.0.0.0 Safari/537.36
- egui 共用一個 wasm instance；GPUI 每個元件各自一個 instance（同一份 wasm 只下載一次）。各輪中位數。
- 「幀數」是每一格自己數的每秒畫面數（網頁格用 requestAnimationFrame）；只要 > 0 就代表那格有在動。
- 瀏覽器 RSS／CPU 是整個瀏覽器（含 GPU、分頁行程）在 5 秒量測區間的數字；CPU 100%＝一顆核心。

### 穿插（2 個元件＋2 個網頁格）

| 項目 | egui（WebGL2） | GPUI（WebGPU 優先） |
|---|---|---|
| 啟動到四格都好（頁面自己量） | 1442 ms | 3876 ms |
| 幀數：webA | 24.2 fps | 5.4 fps |
| 幀數：webB | 24.2 fps | 5.4 fps |
| 幀數：table | 24 fps | 5.4 fps |
| 幀數：chart | 23.2 fps | 5.4 fps |
| JS heap（已用） | 2 MB | 3.5 MB |
| wasm 記憶體（全部 instance 加總） | 8.9 MB | 22.3 MB |
| 瀏覽器全部行程 RSS | 1357.4 MB | 1501.5 MB |
| 瀏覽器 CPU（5 秒量測區間） | 339.6 % | 359.3 % |
| 頁面總下載量（gzip） | 4 MB | 6.2 MB |
| 移除 webB 後：webA | 26.7 fps | 5.6 fps |
| 移除 webB 後：webB | 0 fps | 0 fps |
| 移除 webB 後：table | 26.3 fps | 5.6 fps |
| 移除 webB 後：chart | 26.3 fps | 5.6 fps |
| 報價表 panic 後：webA | 31.7 fps | 9 fps |
| 報價表 panic 後：webB | 0 fps | 0 fps |
| 報價表 panic 後：table | 0 fps | 0 fps |
| 報價表 panic 後：chart | 31.3 fps | 9 fps |

### 4 個元件

| 項目 | egui（WebGL2） | GPUI（WebGPU 優先） |
|---|---|---|
| 啟動到四格都好（頁面自己量） | 1195 ms | 8343 ms |
| 幀數：table | 18 fps | 4 fps |
| 幀數：chart | 17.6 fps | 4 fps |
| 幀數：c1 | 18.2 fps | 4 fps |
| 幀數：c2 | 18.2 fps | 4 fps |
| JS heap（已用） | 2 MB | 5.1 MB |
| wasm 記憶體（全部 instance 加總） | 15.1 MB | 39.1 MB |
| 瀏覽器全部行程 RSS | 1422.2 MB | 1544.9 MB |
| 瀏覽器 CPU（5 秒量測區間） | 339.3 % | 358.7 % |
| 頁面總下載量（gzip） | 4 MB | 6.2 MB |
| 移除 c2 後：table | 20.3 fps | 4.7 fps |
| 移除 c2 後：chart | 20 fps | 4.7 fps |
| 移除 c2 後：c1 | 20.3 fps | 4.7 fps |
| 移除 c2 後：c2 | 0 fps | 0 fps |
| 報價表 panic 後：table | 0 fps | 0 fps |
| 報價表 panic 後：chart | 27.7 fps | 7.7 fps |
| 報價表 panic 後：c1 | 29 fps | 7.3 fps |
| 報價表 panic 後：c2 | 0 fps | 0 fps |

### 1 個元件（報價表）

| 項目 | egui（WebGL2） | GPUI（WebGPU 優先） |
|---|---|---|
| 啟動到四格都好（頁面自己量） | 1168 ms | 1702 ms |
| 幀數：table | 39.6 fps | 10.8 fps |
| JS heap（已用） | 2.1 MB | 2.7 MB |
| wasm 記憶體（全部 instance 加總） | 7.4 MB | 10.2 MB |
| 瀏覽器全部行程 RSS | 1350.3 MB | 1417.2 MB |
| 瀏覽器 CPU（5 秒量測區間） | 339.9 % | 378.8 % |
| 頁面總下載量（gzip） | 4 MB | 6.1 MB |

- 判讀：移除／panic 後，被移除或 panic 的那格應該是 0 或空白，其他格要 > 0；如果其他格也變 0，就是互相拖垮。
- WebGL 繪圖器（瀏覽器回報）：ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)；navigator.gpu（WebGPU）：有。
