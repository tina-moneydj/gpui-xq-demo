# web：一頁嵌多個 GPUI 元件可行性（2026-10-25）

範例：[`examples/web-multi/`](../examples/web-multi/)｜原始數據與截圖：[`docs/bench/web/2026-10-25-multi-gpui/`](bench/web/2026-10-25-multi-gpui/)｜egui 版與完整的 egui 結果：egui-xq-demo 的 `docs/web-multi.md`

## 結論先看

**可行，但一個元件要一個 wasm instance。** 同一個 html 頁面用一般 html 版面（2×2 grid）放 GPUI 元件和一般 HTML 穿插：
左上報價組合（GPUI）、右上公告（HTML）、左下下單匣（HTML，有輸入框和按鈕）、右下技術分析（GPUI K 線）。四格都畫得出來、一起持續更新，HTML 輸入框打字、按鈕都正常。

- **為什麼要各自一個 instance**：gpui_web（`gpui-pre` 0.3.7 的 `WebPlatform`）一個 Application 只能開一個視窗，第二次 `open_window` 會回 `AlreadyOpen`；而且它自己建立 canvas 加到 `<body>`，不能指定放在哪。
  範例的做法：wasm 只下載、編譯一次（`WebAssembly.compileStreaming`），每個元件用不同 query 載入一次 wasm-bindgen 膠水（`xq_web_multi_gpui.js?inst=table`），各自 instance 化，再把它建出來的 canvas 搬進該格。
- **代價是記憶體**：每個元件各自一份 wasm 線性記憶體，每多一個元件約多 10 MB。穿插版面（2 個 GPUI 元件）wasm 記憶體 **24.8 MB**，egui 同樣版面共用一份是 **8.9 MB**。
- **好處是天生隔離**：一格 panic 只停那一格，其他格照跑（egui 共用 instance 要另外換 panic hook 才做得到）。
- **移除元件要呼叫 `destroy()`**（範例裡會 `remove_window`）；只把 canvas 從 DOM 拿掉，那個元件還在背景照畫（實測幀數繼續增加）。

> 這台沒有 GPU：Chromium 153 無頭模式、軟體繪圖。GPUI 預設先試 WebGPU，但無頭 SwiftShader 的 WebGPU 截圖是空白的，所以驗證時加 `?XQ_WEB_BACKEND=webgl` 走 WebGL2。**幀數只供參考**（軟體繪圖下 GPUI 約 5 fps、egui 約 24 fps）。

![穿插版面](bench/web/2026-10-25-multi-gpui/gpui-1-mixed.png)

## 驗證結果（`node examples/web-multi/verify.js dist/web-multi 輸出 XQ_WEB_BACKEND=webgl`）

| 項目 | 結果 | 證據（`results.json`） |
|---|---|---|
| 穿插版面四格都在動 | ✅ 3 秒前後幀數：報價 39→59、K 線 37→57、兩個網頁格 59→79 | `mixed.alive` |
| HTML 輸入 | ✅ 下單匣輸入「2330」按送出，顯示「已送出（模擬）：2330 × 1 張」；之後四格照跑 | `mixed.typed`、`gpui-2-…png` |
| 移除一個網頁格 | ✅ 其他三格照跑 | `mixed.afterRemoveWebB` |
| 報價表 panic | ✅ 只停報價表；K 線、公告照跑 | `mixed.afterPanicTable`、`gpui-3-…png` |
| 4 個 GPUI 元件（`?layout=canvas4`） | ✅ 四格都在動；移除計數器 2 後其他三格照跑；計數器 1 panic 只停它；接著報價表 panic，K 線仍照跑 | `canvas4.*` |
| 只拿掉 canvas、不呼叫 `destroy()` | ⚠️ 被拿掉的那格也還在跑（14→32 幀），會白白吃 CPU | `removeNoDestroy` |
| 主控台 | ✅ 正常執行期間 0 個錯誤；故意 panic 之後，該 instance 會再印一次「RefCell already borrowed」（它的事件迴圈還在被呼叫，但已經 trap，不影響其他格） | `console-*.log` |
| 啟動時間（頁面載入到元件都在動，檔案在本機） | 穿插 1.2 秒；4 個元件 2.6 秒（egui 約 0.2 秒） | `*.boot.readyMs` |

### 記憶體與下載（無頭環境，3 次中位數）

| 設定 | JS heap 已用 | wasm 記憶體（各 instance 加總） | 頁面總下載量（gzip） |
|---|---:|---:|---:|
| 1 個元件（報價表） | 2.3 MB | 11.4 MB | 6.49 MB |
| 穿插（2 個 GPUI 元件＋2 個網頁格） | 3.2 MB | **24.8 MB** | 6.51 MB |
| 4 個 GPUI 元件 | 4.7 MB | **40.4 MB** | 6.55 MB |

- 下載量：wasm 只下載一次（gzip 6.46 MB），每多一個元件只多一次 23 KB 的 js 膠水。這組數字是沒跑 wasm-opt 的檔案；`-O3` 後 wasm gzip 6.35 MB。egui 同樣頁面約 4.2 MB（兩邊都內嵌 3 MB 中文字型子集）。
- **沒有量**瀏覽器行程的記憶體與 CPU：這部分放進實機壓測包量（下一節）。

## 放進實機壓測包

Mac（`bench/mac/xq_bench_mac.py`，`main` 分支）和 Windows（`bench/windows/bench-windows.ps1`，`windows-bench-ci` 分支）的 `web` 情境：

- 工具包的 `web/egui`、`web/gpui` 是兩個 repo main 分支 `examples/web-multi/build.sh` 的產出，由 CI 編好放進 zip。
- 用 Chrome（沒有就 Edge）開全新暫存設定檔，網址加 `?bench=1`：頁面自己照固定流程量（暖機 3 秒 → 5 秒幀數 → 記憶體 → 移除一格 → 報價表 panic），把結果 POST 到壓測包內建的小伺服器（只聽 127.0.0.1、會 gzip）；壓測包另外量頁面總下載量、整個瀏覽器的記憶體與 5 秒量測區間的 CPU。
- 版面 × 框架：穿插、4 個元件、1 個元件 × egui、GPUI，各 3 輪。只跑這項約 8 分鐘。
- 雲端用 Linux 跑過一輪（`bench-pack-cloud.md`）；**還沒在實機上跑過**。

## 怎麼做的

- `examples/web-multi/` 是獨立 crate（自己的 `[workspace]` 與 `Cargo.lock`，根目錄 `Cargo.toml` 的 `exclude` 加了它），報價表、走勢圖用 `#[path]` 直接引用 `src/` 的檔案，**沒改任何 `src/` 程式碼**。
- wasm 匯出 `start(kind)`、`frames()`、`panic_next_frame()`、`destroy()`、`wasm_memory_bytes()`；每個 instance 一個元件。
- 行情：每個元件在自己裡面跑一份合成行情（規則同 `xq-demofeed`），**不是真實行情，也沒接 WebSocket**。
- 頁面邏輯 `page.js` 與 egui-xq-demo 的同名檔案內容相同；`index.html` 只放 GPUI 的啟動方式（編譯一次、instance 化 N 次、把 canvas 搬進格子）。
- 網址參數：`?layout=canvas4`、`?only=table,chart`、`?bench=1`、`?XQ_WEB_BACKEND=webgl|webgpu`。

```bash
# 工具同根目錄網頁版：rustup target add wasm32-unknown-unknown；cargo install wasm-bindgen-cli --version 0.2.129；wasm-opt 可省略
WASM_OPT=/path/to/wasm-opt ./examples/web-multi/build.sh        # 輸出到 dist/web-multi/
python3 -m http.server -d dist/web-multi 8080                     # 瀏覽器開 http://127.0.0.1:8080/
node examples/web-multi/verify.js dist/web-multi 輸出資料夾 XQ_WEB_BACKEND=webgl   # 無頭 Chromium 驗證（Playwright）
```

## 遇到的問題和繞法

| 問題 | 繞法 |
|---|---|
| 一個 Application 只能開一個視窗 | 每個元件各自一個 wasm instance（同一份 wasm 只編譯一次） |
| gpui_web 自己把 canvas 加到 `<body>`，不能指定位置 | 啟動後輪詢（25 ms 一次）把新出現的 canvas 搬進該格；canvas 換了父節點後 GPUI 會依新的大小重畫 |
| 字型找不到 `.SystemUIFont` 而 panic | 根 div 指定 `font_family(ui_font())`（內嵌的中文字型） |
| 無頭 WebGPU 截圖是空白 | 驗證時加 `?XQ_WEB_BACKEND=webgl`；實機壓測包用預設（WebGPU 優先） |

## 沒驗的事

- **實機數字**：雲端沒有顯卡，幀數、CPU 都不準；壓測包的 `web` 情境還沒在實機跑過。
- **WebGPU**：只在無頭環境確認 WebGPU 會畫（但截圖空白），畫面正確性是用 WebGL2 驗的。
- **真的瀏覽器**：只跑 Chromium 153 無頭版；沒試 Edge、Firefox、Safari、手機瀏覽器。
- **鍵盤焦點在 GPUI 格和 HTML 格之間切換**、注音組字：只驗了 HTML 輸入框打字。
- **跨元件溝通**（點報價表讓 K 線換代號）：各自 instance 只能經過 JS 傳，沒做。
- **destroy 後記憶體有沒有還**：沒量（wasm 線性記憶體本來就不會縮；整個 instance 丟掉之後瀏覽器會不會回收沒驗）。
- **窗格縮放、DPR 改變**時 canvas 大小是否跟著變：沒測。
