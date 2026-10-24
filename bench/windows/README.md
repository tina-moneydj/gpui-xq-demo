# XQ Windows 壓測包（GPUI vs egui）

**這是什麼：** 在你的 Windows 電腦上，用同一套情境比較兩種看盤 UI 寫法的速度、記憶體、CPU 和 GPU：
- **GPUI**：Zed 的框架，用 DirectX 11 畫
- **egui**：同一個 exe 跑兩次，一次用 OpenGL（glow）畫（app 名 `egui`），一次用 wgpu 畫（app 名 `egui-wgpu`，Windows 上走 DX12／Vulkan）

wry 已經淘汰，不在這次的比較裡。

> 第一份 Windows 實機數字在 2026-10-23（i7-13700＋GTX 1650，見 docs/bench/real-hw）；2026-10-24 修正了重圖表逾時、勝負判定、egui 點選漏抓（見文末）。跑完後 `summary.md` 會先寫結論，每一項的較佳者標 🏆。

### 這一版多了什麼：GPUI 優化第 2 批＋GPUI 跑兩次（XQ_DRAW_QUADS 關／開）

- **結論先講：** 這一版的 GPUI 是 gpui-opt2（多視窗報價同一幀到、報價表一列一次畫完、畫線可改用 quad）。請整套重跑一次，跟 2026-10-23 那份比；重圖表會多一欄「GPUI quad 畫線（XQ_DRAW_QUADS=1）」，拿它跟「GPUI」那欄比，就知道在 GTX 1650 上要不要打開這個優化。
- `XQ_DRAW_QUADS` 是什麼：圖表上水平線、垂直線、矩形框這些「橫平豎直」的畫線，改用便宜的方塊（quad）畫，不走比較貴的路徑（path）。在我們沒有顯示卡的測試機上反而變慢約一半（GPU 批次被切碎），所以**預設關**；真的顯示卡上結果可能不同。
- 現在 `XQ_DRAW_QUADS=1` 是 **A3 v2「分組」畫法**：照畫線的上下順序分組，盡量少切批次，也修好舊畫法「文字永遠蓋在線上面」的順序問題。沒有顯示卡的測試機上 v2 重圖表 FPS 20.8→10.3（跟 v1 差不多），截圖上下順序正確、只在半透明線重疊的邊緣差幾個點。細節：gpui-xq-demo 的 `docs/bench/2026-10-24-a3v2/`。想比舊版 A3 可設 `XQ_DRAW_QUADS=v1`。
- 工具包會自動做：「GPUI」那欄固定 `XQ_DRAW_QUADS=0`，app 名 `gpui-quads` 那欄是同一個 exe 加 `XQ_DRAW_QUADS=1`。quad 只影響畫線，所以只有重圖表多跑這一欄（多約 4 分鐘）。
- 不想多跑：`run-bench.cmd -Apps gpui,egui,egui-wgpu`。

## 你只要做四件事（不用裝 Rust、不用編譯）

1. **下載**完整版：[egui-xq-demo → windows-bench-latest → `xq-bench-windows.zip`](https://github.com/tina-moneydj/egui-xq-demo/releases/tag/windows-bench-latest)
   - egui-xq-demo 是**私人 repo**，要先在瀏覽器登入 GitHub（tina-moneydj）才看得到這個下載頁。
   - 沒有登入的人只能下載公開的 GPUI 版：[gpui-xq-demo → windows-bench-latest → `xq-bench-windows-gpui.zip`](https://github.com/tina-moneydj/gpui-xq-demo/releases/tag/windows-bench-latest)，這個版本只會測 GPUI。
2. **解壓縮**：在 zip 上按右鍵 →「全部解壓縮」，例如解到 `C:\xq-bench`。
3. **執行**：打開解出來的 `xq-bench-windows` 資料夾，雙擊 **`run-bench.cmd`**。
   - 如果跳出藍色的「Windows 已保護您的電腦」：按「其他資訊」→「仍要執行」。（程式沒有數位簽章，這是正常的。）
4. **傳回結果**：跑完會自動在檔案總管選取一個 `.zip`（在 `results\` 資料夾裡），把這個 zip 傳回來就好。

## 跑之前（很重要）

| 項目 | 怎麼做 |
|---|---|
| 電源 | 筆電**接上電源**，電源模式選「最佳效能」 |
| 其他程式 | 關掉瀏覽器、Teams/LINE、會議軟體、XQ。它們會吃 CPU 和 GPU，也可能佔用 127.0.0.1:47631 |
| 螢幕 | 有幾個就接幾個，解析度和縮放維持平常的設定 |
| 測試中 | **不要動滑鼠和鍵盤**。視窗會自己開關、自己移動，滑鼠也會自己點報價表。螢幕保護程式和睡眠先關掉或調長 |

## 要多久

| 模式 | 時間 | 怎麼跑 |
|---|---|---|
| 完整（預設） | 約 95 分鐘（GPUI、egui glow、egui wgpu 三個 app；重圖表再加 GPUI quad） | 雙擊 `run-bench.cmd` |
| 快速版 | 約 25 分鐘 | 在資料夾網址列輸入 `cmd` 按 Enter，再輸入 `run-bench.cmd -Quick` |

開始時畫面上會顯示這次的預估時間。

---

## 以下是細節（不用看也能跑）

### 量了什麼

| 情境 | 內容 |
|---|---|
| 主視窗 | 1200×720 邏輯像素（會乘上螢幕縮放）。量啟動到第一幀與第一筆報價的時間、記憶體（10 秒、60 秒）、CPU／GPU／FPS（第 30–60 秒） |
| 5 萬列 | 報價表 5 萬列 × 6 欄，每秒 3000 筆跳價 30 秒，接著邊跳價邊捲動 10 秒 |
| 重圖表 | 20 個疊加、8 個副圖、1000 筆畫線（100 層）。依序跑 7 段：跳價、加十字線、再加平移、拖曳畫線、框選畫線、調整圖層順序（各 10 秒）、上下順序截圖（3 秒），每次約 75 秒 |
| 高頻進價 | 每秒 1k、10k、50k、100k 筆，加上開盤爆量。量延遲 p50/p99、遺失、積壓、CPU/GPU/FPS，以及點報價列切換走勢圖的反應時間 |
| 多視窗 | 開 1 個和 4 個看盤視窗，行情每秒 10k 筆。4 個以上螢幕時每個視窗佔滿一個螢幕；不夠時平均分到各螢幕再格狀鋪滿 |

每個數字是 3 輪的中位數（快速版 1 輪）。
- **記憶體**：整棵程序樹，同時記 Working Set（≈ Linux RSS）、Working Set - Private（≈ 工作管理員「記憶體」欄）、Private Bytes。
- **CPU**：100% 等於一顆邏輯核心。
- **GPU**：效能計數器「GPU Engine」裡 3D 引擎的使用率，加上 GPU 專用記憶體。
- 主視窗／5 萬列／重圖表的行情來自 `xq-demofeed`（合成資料，約每 25ms 一批跳價）；高頻與多視窗來自 `xq-tickgen`。都不是真實行情。

### 結果檔

`results\<日期時間>-<電腦名稱>\` 資料夾和同名 `.zip`：

| 檔案 | 內容 |
|---|---|
| `summary.md` | 中文摘要：結論、贏家（🏆）、機器資訊、各情境表格 |
| `results.json` | 機器資訊、執行檔雜湊、每一輪原始數據、中位數 |
| `*.log` | 各程式的輸出 |
| `*.png` | 截圖，用來確認畫面和點擊位置 |

測試時程式的設定檔放在 `%TEMP%\xq-bench-work`，不會動到你自己的設定。

### 執行檔從哪來

完整版 zip 的 `bin\` 已經放好 `gpui-xq-demo.exe`、`egui-xq-demo.exe`、`xq-tickgen.exe`、`xq-demofeed.exe`，由 GitHub Actions 在 `windows-bench-ci` 分支自動編譯。
找不到時，腳本會依序找 `-BinDir <資料夾>` → `bin\` → 從 `windows-bench-latest` release 下載（egui 是私人 repo，需要 `gh auth login` 或環境變數 `GITHUB_TOKEN`）。`-Update` 會重新下載。

想自己從原始碼編譯（需要 Rust、Visual Studio Build Tools＋Windows SDK）：

```
run-bench.cmd -Build -SrcRoot D:\src      （D:\src 底下放 gpui-xq-demo、egui-xq-demo）
```

### 常用參數

```
run-bench.cmd -Quick                          快速版
run-bench.cmd -Runs 5                         5 輪中位數
run-bench.cmd -Scenarios ticks,multi          只跑高頻和多視窗（可選 main,stress,heavy,ticks,multi）
run-bench.cmd -Ticks s10k,s100k -Apps gpui    只測 GPUI（可選 gpui,gpui-quads,egui,egui-wgpu；gpui-quads 只跑重圖表）
run-bench.cmd -Scenarios heavy                只重跑重圖表（4 欄 × 3 輪，約 17 分鐘）
run-bench.cmd -Windows 1,2,4                  多視窗開幾個
```

### 常見問題

- **「這些程式正在執行」或「127.0.0.1:47631 已被佔用」**：先關掉 XQ 或上次沒結束的測試程式，再重跑。
- **SmartScreen 警告**：exe 沒有數位簽章。點「其他資訊 → 仍要執行」。腳本也會自動對 `bin\` 裡的 exe 解除封鎖。
- **點選切換顯示「沒對到」**：滑鼠沒點在報價列上，不影響其他數字。可以看截圖，再用環境變數 `GPUI_ROW0_Y`、`GPUI_ROW1_Y`、`EGUI_ROW0_Y`、`EGUI_ROW1_Y` 校正（內容區邏輯像素，Linux 校正值 462/484）。
- **某一項失敗**：腳本會自動重跑一次，仍失敗就跳過並記在 `results.json` 的 `failures`，其他項目照跑。

## 2026-10-24 修正

- 重圖表：app 已經是 7 段（多了拖曳、框選、調整順序、上下順序截圖），舊腳本只等 3 段，第 3 段後等 `cp:done` 只等 10 秒就逾時，所以 2026-10-23 那次 heavy 全部失敗。現在 7 段都量、`cp:done` 在最後一段後等。
- 勝負：每個 app 都和最好的比，差 5%（或 1 ms／1%／2 MB）以內都算贏、可以多個 🏆；全部都在範圍內才是平手；FPS 全部到螢幕更新率 95% 以上算平手。舊版只有數字完全相同才平手。
- 點選切換：先移滑鼠、停 0.1 秒再按，延遲從按下算起。舊版 SetCursorPos 後立刻按，egui（winit 按鍵事件不帶座標）常把這一下算在上一次點的列，8 次漏 5 次。
- 加 `egui-wgpu`（同一個 egui exe，`XQ_RENDERER=wgpu`）。
- 結果檔匿名化：資料夾名稱與 results.json 不記電腦名稱，家目錄／帳號換成 `C:\Users\user`／`user`。
