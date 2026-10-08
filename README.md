# gpui-xq-demo — XQ 風格看盤（Rust + GPUI 原生繪圖版）

與 [`wry-xq-demo`](https://github.com/tina-moneydj/wry-xq-demo)（tao + wry，HTML/JS 畫面）**接同一個行情來源**：
`WryFeedHost`（.NET，`127.0.0.1:47631`，little-endian 封包）。差別是這版整個主畫面都用 GPUI 原生繪圖，不用 WebView。

![分時](screenshot.png)

| 日線（合成）＋MA5/20/60＋成交量 | 壓力 5 萬列（虛擬化） |
|---|---|
| ![日線](screenshot-daily.png) | ![壓力](screenshot-stress-50k.png) |

## 功能（MVP）

- 版面：上＝走勢圖、左下＝報價表、右下＝資訊／效能格；**上下、左右分隔線都可拖曳**。
- 報價表：代號／名稱／成交／漲跌／幅度／總量，**漲紅跌綠**，成交價底色顯示上一筆 tick 方向；
  `uniform_list` 虛擬化，按「壓力 5 萬」加 5 萬列合成資料測試捲動與記憶體。
- 走勢圖（canvas 自繪）：
  - **分時**：引擎 `intraday` 快照＋`minutes` 增量分鐘線，昨收虛線、MA5/MA20、成交量。
  - **日線**：引擎不給日 K，與 wry 版相同做法「合成日線、最後一根跟著現價」，MA5/20/60、成交量；滾輪縮放、右鍵重設。
  - 十字線＋游標所在 K 棒的開高低收量／MA 讀值。
  - 點報價列切換走勢圖代號（重新訂閱 `chart` 代號）。
- 狀態列：連線狀態、FPS、RSS（每秒取樣）、引擎封包數→UI 套用次數。

右下的「網頁格」**不在範圍內**：GPUI 沒有官方 WebView；`gpui-wry` 的子視窗畫在 GPUI 之上、不受 ContentMask 裁切
（舊測試程式保留在 `legacy/gpui_wry_window_test.rs`，不參與編譯）。此格先放資訊與效能數字。

## 建置／執行

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release            # rust-toolchain.toml 用 stable（GPUI 依賴 oo7 需要 rustc ≥ 1.92）
DISPLAY=:2 XQ_METRICS=1 ./target/release/gpui-xq-demo
cargo test --release --workspace # 協議切包／解碼、增量 MA 測試
```

環境變數：

| 變數 | 用途 |
|---|---|
| `XQ_FEED_ADDR=127.0.0.1:47631` | 改行情位址（預設 47631） |
| `XQ_METRICS=1` | 每秒在 stderr 印 fps / rss / 封包數 / 套用耗時 |
| `XQ_NO_VIEW_CACHE=1` | 關掉 view cache（比較用） |
| `XQ_DEBUG=1` | 印滑鼠事件等除錯訊息 |

Linux 需要 Vulkan（或 GL）驅動；沒有 GPU 的機器裝 Mesa lavapipe：`sudo apt install mesa-vulkan-drivers vulkan-tools`。

> **WryFeedHost 只接受一個 client（後連者勝）**。GPUI 版和 wry 版同時開會互搶連線（各自 500ms 重連），
> 測試時先暫停另一個：`kill -STOP <wry pid>`，測完 `kill -CONT <wry pid>`。

## 架構

```
crates/xq-feed      ← 共用協議 crate（零依賴）：Subscribe 編碼、FrameReader 切包、型別化 decode、背景連線/重連執行緒
src/bridge.rs       ← 背景執行緒 → UI 的合併層（Inbox）
src/table.rs        ← 報價表 view（uniform_list）
src/chart.rs        ← 走勢圖 view（canvas 自繪、十字線、縮放）
src/series.rs       ← K 線序列＋增量 MA、合成日線
src/main.rs         ← Terminal 根 view：版面、分隔線、狀態列、資訊格、FPS/RSS 取樣
```

資料流：

1. `xq-feed` 背景執行緒讀 TCP → `FrameReader` 切包（半包保留）→ `decode` 成 `Msg`（不經 JSON）。
2. `bridge::merge` 在背景執行緒 O(1) 合併進 `Inbox`：**同代號報價只留最新**、分時快照只留最新、同分鐘的分鐘線只留最新。
   只有 Inbox 從空變非空時才送一次喚醒訊號。
3. UI 端一個 async task：被喚醒 → `take()` 整包取走 → 套用 → **睡 16ms 再收下一次**，所以引擎推再快 UI 最多 ~60 次/秒套用。
4. 套用只改有變的列（價／量相同就跳過），只有實際變動的 view 才 `notify`。
5. 報價表／走勢圖／資訊格是三個獨立 entity，用 `.cached()` 掛在根 view 下：報價 tick 只重畫報價表的**可見列**，
   走勢圖只有目前代號有 tick 時才重畫；字串只在 render 時為可見列格式化（5 萬列不預先建字串）。
6. 走勢圖序列放在 `Rc<Series>`，paint closure 只 clone Rc；tick 用 `Rc::make_mut` 只改最後一根，
   MA 只重算最後一個值（O(期數)），新分鐘才 append。換代號／週期才 O(n) 重建一次。

## 效能數字（本機 box：8 核、無 GPU、Mesa lavapipe 軟體 Vulkan、Xvfb 1280×800，demo-ticks 行情）

| 項目 | GPUI 版 | wry 版（同時段量測） |
|---|---|---|
| RSS（全部行程） | **≈ 184–196 MB**（單一行程） | 533 MB（主程式 85 + WebKitWebProcess 430 + Network 18）；先前量測右下空白時 615–650 MB |
| 加 5 萬列壓力資料 | +≈ 4.5 MB（191 MB） | — |
| 引擎封包 | 約 30–38 包/s、900–1,200 筆報價/s | 同一來源 |
| UI 套用 | 與封包數相同（≤ 60/s 上限），平均 **17–24 µs**/次 | — |
| 走勢圖 paint（CPU 端組 scene） | 分時 ≈ 0.3 ms、日線 ≈ 0.6–0.7 ms | — |
| FPS | 27–36（跟著 tick 頻率；無變動時不重畫） | — |
| CPU | ≈ 235%（**幾乎都是 lavapipe 軟體光柵**；有實體 GPU 時會低很多） | — |

注意：FPS 上限在這台機器上同時受「行情 ~30 包/s」與「lavapipe 軟體光柵每 frame 約 30ms」限制；
CPU 端每 frame 的工作（套用＋組 scene）不到 1 ms。

## 平台狀態（GPUI）

- **macOS**：GPUI 的主平台（Zed 編輯器），Metal 繪圖，最成熟。
- **Windows**：已有官方支援（DirectX 11 / DirectComposition），Zed Windows 版已釋出；中文輸入法、HiDPI 要實測。
- **Linux**：X11 與 Wayland 都支援，這個版本（`gpui-pre 0.3.7`）繪圖走 wgpu（Vulkan 為主）；需要可用的 Vulkan/GL 驅動。
- **iOS / iPadOS / Android**：GPUI **不支援**行動平台；全平台要另做行動端（共用 `xq-feed` 協議／Rust 核心）。
- 依賴：`gpui-pre` 是社群發佈的 Zed GPUI 快照（crates.io 上官方 `gpui` 0.2.x 版本較舊），API 仍在變動，版本鎖在 `=0.3.7`。

## 跟 wry 版相比還沒做的

- 右下嵌網頁（Yahoo 等）、多分頁、畫線工具、指標選單（KD／MACD／RSI…）、週/月線、設定存檔、安裝檔。
- 自選清單編輯、搜尋代號。
- `xq-feed` 已抽成獨立 crate；wry 版目前仍用自己的 `src/engine.rs`（同格式），之後可改成 path/git 依賴共用同一份實作。

## 已知行情來源問題（不是這個程式的 bug）

`WryFeedHost --demo-ticks` 的隨機漫步有向上漂移（每 tick 期望 +0.003%，40 次/s），跑久了價格會一路漲；
跑了數小時的 2330/2317/2454/0050 已經溢位成 ±21474836.48（`int` 分），表格顯示 `--`、走勢圖會忽略這種報價。
重開 feedhost 即可恢復正常數值。
