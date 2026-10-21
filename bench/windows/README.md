# XQ 三方壓測：Windows 測試包

這個工具包在 Windows 上比較三種看盤 UI 寫法：
- **wry**：WebView2 網頁
- **GPUI**：DirectX 原生繪圖
- **egui**：OpenGL 立即模式

情境和量法跟 Linux 版（`scripts/bench-*.sh`）相同。跑完會產生 `summary.md`（結論在前）、`results.json` 和一個 zip，**把 zip 傳回來就好**。

## 需要什麼

| 項目 | 說明 |
|---|---|
| Windows 10（1903 以後）或 Windows 11，64 位元 | 內建的 PowerShell 5.1 就能跑，不用安裝 Rust 或 .NET |
| WebView2 Runtime | Win11 內建，Win10 通常已經裝好（有 Edge 就有）。沒有的話 wry 那一項會失敗，其他照跑 |
| 執行檔 | 第一次執行會自動從 GitHub 的 `bench-latest` release 下載（見下方「執行檔從哪來」） |
| 螢幕 | 有幾個就接幾個，解析度和縮放維持平常的設定。多視窗 ×4 會分配到各個螢幕上 |
| 電源 | 筆電請**接上電源**，電源模式選「最佳效能」 |

## 怎麼跑

1. 把整個 `bench\windows` 資料夾放到本機，例如解壓縮到 `C:\xq-bench`。
2. **關掉其他程式**：瀏覽器、Teams/LINE、會議軟體、正在跑的 XQ 或 WryFeedHost。這些會吃 CPU 和 GPU，也可能佔用 127.0.0.1:47631。
3. 雙擊 **`run-bench.cmd`**。
4. **測試期間不要動滑鼠和鍵盤。** 視窗會自己開關、自己移動，滑鼠也會自己點報價表。
   螢幕保護程式和睡眠最好先關掉，或把時間調長。
5. 跑完後檔案總管會自動選取結果 zip，把它傳回來。

快速版：在命令提示字元輸入 `run-bench.cmd -Quick`。

## 要多久

| 模式 | 時間 | 內容 |
|---|---|---|
| 完整（預設） | 約 75 分鐘 | 3 輪中位數，五大情境全跑 |
| `-Quick` | 約 20 分鐘 | 1 輪；高頻只跑 10k、100k、爆量 |

開始時畫面上會顯示這次的預估時間。

## 量了什麼

| 情境 | 內容 |
|---|---|
| 主視窗 | 1200×720 邏輯像素（會乘上螢幕縮放）。量啟動到第一幀與第一筆報價的時間、記憶體（10 秒、60 秒）、CPU／GPU／FPS（第 30–60 秒） |
| 5 萬列 | 報價表 5 萬列 × 6 欄，每秒 3000 筆跳價 30 秒，接著邊跳價邊捲動 10 秒 |
| 重圖表 | 20 個疊加、8 個副圖、1000 筆畫線。依序跑三段，每段 10 秒：跳價、加十字線、再加平移 |
| 高頻進價 | 每秒 1k、10k、50k、100k 筆，加上開盤爆量（前 3 秒約 30 萬筆）。量延遲 p50/p99、遺失、積壓、CPU/GPU/FPS，以及點報價列切換走勢圖的反應時間 |
| 多視窗 | 開 1 個和 4 個看盤視窗，行情每秒 10k 筆。有 4 個以上螢幕時每個視窗佔滿一個螢幕；不夠時平均分到各螢幕，再格狀鋪滿。量各視窗 FPS、全部視窗合併的延遲、記憶體、CPU/GPU |

各項指標的定義：
- **記憶體**：整棵程序樹（wry 包含 `msedgewebview2.exe` 子程序）。同時記錄三種數字：
  - Working Set，約等於 Linux 的 RSS
  - Working Set - Private，約等於工作管理員的「記憶體」欄
  - Private Bytes
- **CPU**：100% 等於一顆邏輯核心。
- **GPU**：效能計數器「GPU Engine」裡 3D 引擎的使用率，加上 GPU 專用記憶體。

## 結果放哪裡

每次執行會產生一個資料夾 `bench\windows\results\<日期時間>-<電腦名稱>\` 和同名的 `.zip`。資料夾裡有：

| 檔案 | 內容 |
|---|---|
| `summary.md` | 中文摘要：結論、贏家（🏆）、機器資訊、各情境表格 |
| `results.json` | 機器資訊、執行檔雜湊、每一輪原始數據、中位數 |
| `*.log` | 各 app 的輸出 |
| `*.png` | 截圖，用來確認畫面和點擊位置 |

測試時 app 的設定檔會放在暫存資料夾 `%TEMP%\xq-bench-work`，不會動到你自己的設定。

## 執行檔從哪來

程式依下列順序找執行檔：
1. `-BinDir <資料夾>`
2. 本資料夾的 `bin\`
3. 從 GitHub release `bench-latest` 自動下載（由各 repo 的 Windows CI 產生）：
   - [gpui-xq-demo](https://github.com/tina-moneydj/gpui-xq-demo/releases/tag/bench-latest)：`gpui-xq-demo.exe`、`xq-tickgen.exe`，另有整包 `xq-bench-windows.zip`（工具包加上這兩個 exe）
   - [wry-xq-demo](https://github.com/tina-moneydj/wry-xq-demo/releases/tag/bench-latest)：`wry-xq-demo.exe`、`WryFeedHost.exe`（自帶 .NET，不用另外安裝）
   - egui-xq-demo 是**私人 repo**，要符合下列其中一項：
     - 已安裝 [GitHub CLI](https://cli.github.com/) 並執行過 `gh auth login`
     - 設定環境變數 `GITHUB_TOKEN`
     - 直接把 `egui-xq-demo.exe` 放進 `bin\`

     都沒有的話，這次只比 wry 和 GPUI。

`-Update` 會重新下載最新版。

想自己從原始碼編譯：

```
run-bench.cmd -Build -SrcRoot D:\src
```

這需要先準備：
- Rust、.NET 8 SDK
- Visual Studio Build Tools 加上 Windows SDK（GPUI 的 release 編譯需要 SDK 裡的 `fxc.exe`）

## 常用參數

```
run-bench.cmd -Quick                          快速版
run-bench.cmd -Runs 5                         5 輪中位數
run-bench.cmd -Scenarios ticks,multi          只跑高頻和多視窗（可選 main,stress,heavy,ticks,multi）
run-bench.cmd -Ticks s10k,s100k -Apps gpui,egui
run-bench.cmd -Windows 1,2,4                  多視窗開幾個
run-bench.cmd -BinDir D:\xq-exe -NoZip
```

## 常見問題

- **「這些程式正在執行」或「127.0.0.1:47631 已被佔用」**：先關掉 XQ、WryFeedHost 或上次沒結束的測試程式，再重跑。
- **出現 Windows Defender SmartScreen 警告**：exe 沒有數位簽章。點「其他資訊 → 仍要執行」，或在檔案內容勾選「解除封鎖」。
- **點選切換顯示「沒對到」**：版面跟預期不同，滑鼠沒點在報價列上。這不影響其他數字。可以看截圖，再用環境變數 `GPUI_ROW0_Y`、`GPUI_ROW1_Y`、`EGUI_ROW0_Y`、`WRY_ROW0_Y` 等校正。單位是內容區的邏輯像素，Linux 校正值是 462/484、462/484、501/524。
- **某一項失敗**：腳本會自動重跑一次。仍然失敗就跳過，記錄在 `results.json` 的 `failures`，其他項目照跑。
