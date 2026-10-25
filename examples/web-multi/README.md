# 一頁嵌多個 GPUI 元件（可行性範例）

一個 html 頁面（2×2 grid）放 GPUI 元件和一般 HTML：預設左上報價組合（50 列）、右上公告、左下下單匣（一般 HTML）、右下技術分析（K 線）；
`?layout=canvas4` 是 4 個 GPUI 元件（報價表、K 線、兩個計數器）。gpui_web 一個 Application 只能開一個視窗，所以**每個元件各自一個 wasm instance**（同一份 wasm 只下載、編譯一次）。
`page.js`（版面、網頁格、自動壓測）與 egui-xq-demo 的同名檔案內容相同。結果與說明：[docs/web-multi.md](../../docs/web-multi.md)。

```bash
WASM_OPT=/path/to/wasm-opt ./build.sh                   # → ../../dist/web-multi/
python3 -m http.server -d ../../dist/web-multi 8080      # 瀏覽器開 http://127.0.0.1:8080/
node verify.js ../../dist/web-multi 輸出資料夾 XQ_WEB_BACKEND=webgl   # 無頭 Chromium 驗證（Playwright；兩個 repo 同一支）
```

網址參數：`?layout=canvas4`、`?only=table,chart`、`?bench=1`（自動壓測，結果 POST 到 `/__bench`、也印在 console）、`?XQ_WEB_BACKEND=webgl|webgpu`。
