# XQ Mac 壓測包（GPUI vs egui）

**這是什麼：** 在你自己的 Mac 上，用同一套情境比較兩種看盤 UI 寫法，量它們的速度、記憶體和 CPU：
- **GPUI**：Zed 的框架，用 Metal 畫
- **egui**：分兩種跑法
  - wgpu／Metal
  - glow／OpenGL（跟 Linux、Windows 版的測試一樣）

wry 已經淘汰，不在這次的比較裡。

**你只要做五件事（不用裝 Xcode、不用裝 Rust、不用編譯）：**

1. **下載**完整版：[egui-xq-demo → mac-bench-latest → `xq-bench-mac.zip`](https://github.com/tina-moneydj/egui-xq-demo/releases/tag/mac-bench-latest)
   - egui-xq-demo 是**私人 repo**，要先在瀏覽器登入 GitHub（tina-moneydj）才看得到這個下載頁。
   - 沒登入的人只能下載公開的 GPUI 版：[gpui-xq-demo → mac-bench-latest → `xq-bench-mac-gpui.zip`](https://github.com/tina-moneydj/gpui-xq-demo/releases/tag/mac-bench-latest)，這個版本只會測 GPUI。
2. **解壓縮**：在「下載項目」裡雙擊 zip，會出現 `xq-bench-mac` 資料夾。（Safari 可能已經自動解壓好了。）
3. **解除「從網路下載」的封鎖**：打開「終端機」（應用程式 → 工具程式），貼上這行，按 Enter：
   ```
   xattr -dr com.apple.quarantine ~/Downloads/xq-bench-mac
   ```
   資料夾不在「下載項目」的話，先打 `xattr -dr com.apple.quarantine `（最後有空格），再把資料夾拖進終端機視窗，按 Enter。
4. **執行**：在同一個終端機貼上這兩行：
   ```
   cd ~/Downloads/xq-bench-mac
   ./run-bench.command
   ```
   測試視窗會自己開、自己關，**跑完之前不要動滑鼠鍵盤**。
5. **傳回結果**：跑完 Finder 會自動選取結果 zip（在 `results/` 裡，檔名像 `20261024-101500-MacBook-Pro.zip`），**把這個 zip 傳回來就好。**

跑完後，結果資料夾裡的 `summary.md` 會先寫結論，每一項的較佳者標 🏆。

> 第一份 Mac 實機數字（2026-10-24，MacBook Air M4）在 `docs/bench/real-hw/2026-10-24-mac/`。這一版工具包換成 **GPUI 優化第 2 批（gpui-opt2：多視窗同幀、報價表一列一次畫完、畫線改 quad 開關）**，請重跑一次比較前後。

### 這一版多了什麼：GPUI 跑兩次（XQ_DRAW_QUADS 關／開）

- **結論先講：** 重圖表會多出一欄「GPUI quad 畫線（XQ_DRAW_QUADS=1）」。拿它跟「GPUI」那欄比，就知道在你的 Mac 上要不要打開這個優化。
- `XQ_DRAW_QUADS` 是什麼：圖表上水平線、垂直線、矩形框這些「橫平豎直」的畫線，改用便宜的方塊（quad）來畫，不走比較貴的路徑（path）。在我們沒有顯示卡的測試機上反而變慢約一半（因為 GPU 批次被切碎），所以**預設關**；你的 Mac 有真的 GPU，結果可能不同。
- 工具包會自動做：「GPUI」那欄固定用 `XQ_DRAW_QUADS=0`，「GPUI quad」那欄用 `XQ_DRAW_QUADS=1`。quad 只影響畫線，所以只有重圖表會多跑這一欄（多約 4 分鐘），其他情境不跑。
- 不想多跑：`./run-bench.command --apps gpui,egui-wgpu,egui`。
- 自己開 GPUI 看效果：`XQ_DRAW_QUADS=1 ./bin/gpui-xq-demo`（關掉就是不設或設 0）。

### 跑之前（很重要）

| 項目 | 怎麼做 |
|---|---|
| 電源 | 筆電**插上電源**。到「系統設定 → 電池」把「低耗電模式」關掉 |
| 其他程式 | 關掉瀏覽器、LINE、Teams、會議軟體、XQ。它們會搶 CPU 和 GPU |
| 螢幕 | 有外接螢幕就接上，解析度和縮放維持平常的設定，不用特別調整 |
| 測試中 | **不要動滑鼠和鍵盤**，也不要把終端機拖到測試視窗上面。被蓋住的視窗 macOS 會停止重畫，數字就不準了 |

Intel Mac：完整版的 egui 執行檔目前只編了 Apple Silicon（M 系列）版，Intel Mac 上 egui 那兩項會失敗，只會有 GPUI 的數字。需要 Intel 版的話，到 egui-xq-demo 的 Actions 手動執行 macos workflow 並勾選 universal。

---

## 要多久

| 模式 | 指令 | 時間 | 內容 |
|---|---|---|---|
| 完整（預設） | `./run-bench.command` | 約 80 分鐘 | 3 輪中位數、三種跑法（GPUI、egui wgpu、egui glow）、五大情境全跑；重圖表多跑一次 GPUI quad（XQ_DRAW_QUADS=1） |
| 快速 | `./run-bench.command --quick` | 約 20 分鐘 | 1 輪；高頻只跑 10k、100k、爆量 |
| 只比兩個 | `./run-bench.command --apps gpui,egui-wgpu` | 約 50 分鐘 | 只跑 GPUI 和 egui（Metal） |

想中途停止：在終端機按 `Control + C`。已經跑完的部分照樣會存檔。

## 量了什麼

每個情境 3 輪，取中位數。情境和量法跟 Linux、Windows 版一樣，所以結果可以互相比較。

| 情境 | 內容 |
|---|---|
| 主視窗 | 1200×720 點的視窗（Retina 螢幕實際是 2400×1440 像素），行情約每秒 40 次。量三件事：<br>• 啟動到第一幀、啟動到第一筆報價的時間<br>• 記憶體（第 10 秒、第 60 秒）<br>• 第 30–60 秒的 CPU 和 FPS |
| 5 萬列 | 報價表 5 萬列 × 6 欄，每秒 3000 筆跳價 30 秒，接著邊跳價邊捲動 10 秒 |
| 重圖表＋100 層畫線 | 圖表上有 20 條疊加線、8 個副圖、1000 筆畫線，畫線分成 100 層。依序跑七段：<br>跳價 → 十字線 → 平移 → 拖曳畫線 → 選取 → 調整上下順序 → 正確性檢查 |
| 高頻進價 | 每秒 1k、10k、50k、100k 筆，再加開盤爆量。量延遲 p50/p99、遺失筆數、積壓、CPU、FPS |
| 多視窗 | 開 1 個和 4 個看盤視窗，行情每秒 1 萬筆：<br>• 螢幕有 4 個以上：一個螢幕放一個視窗<br>• 螢幕不夠：平均分配到各螢幕再格狀鋪滿<br>每個螢幕的縮放（Retina ×2）和更新率（ProMotion 120Hz）都會記錄 |

各項數字的意思：

| 數字 | 意思 |
|---|---|
| RSS | 程式佔用的常駐記憶體 |
| 實體記憶體 | 跟「活動監視器」的「記憶體」欄一樣 |
| CPU | 100% 等於用滿一顆核心 |
| GPU | 整台 Mac 的 GPU 使用率，包含其他程式，只當參考 |
| FPS | 程式每秒畫出幾個畫面。到達螢幕更新率（60 或 120）就是頂了 |

**不需要**另外開權限：不用 sudo，也不用「輔助使用」或「螢幕錄製」。

## 結果檔

結果在 `xq-bench-mac/results/<日期時間>-<電腦名稱>/`（從原始碼跑是 `bench/mac/results/`），旁邊還有一個同名的 `.zip`。

| 檔案 | 內容 |
|---|---|
| `summary.md` | 中文摘要：結論、贏家 🏆、這台 Mac 的資訊（機型、晶片、螢幕）、各情境表格 |
| `results.csv` | 每個情境、每個指標：中位數和每一輪的數字。可以用 Numbers 或 Excel 打開 |
| `results.json` | 完整原始數據，包含失敗紀錄 |
| `*.log` | 各程式的輸出 |

## 常見問題

- **「無法打開，因為無法驗證開發者」或「已損毀」**
  檔案是用瀏覽器下載的，帶有隔離標記。請執行下面這行，再重跑：
  ```
  xattr -dr com.apple.quarantine ~/Downloads/xq-bench-mac
  ```
  （從原始碼跑的話換成 `~/xq-mac`。用 `git clone` 下載的不會有這個問題。）

- **雙擊 run-bench.command 沒反應，或顯示「沒有權限」**
  在終端機執行 `chmod +x ~/Downloads/xq-bench-mac/*.command ~/Downloads/xq-bench-mac/*.sh`（從原始碼跑的話是 `~/xq-mac/gpui-xq-demo/bench/mac/`）。

- **跳出「是否允許接受連入網路連線」**
  測試只用本機的 127.0.0.1，按「允許」或「拒絕」都不影響結果。

- **「127.0.0.1:47631 已被佔用」或「這些程式正在執行」**
  先關掉 XQ 或上次沒結束的測試程式，或改用別的埠：`./run-bench.command --port 47731`。

- **終端機在 Rosetta 下執行**
  工具會提醒你。請到 Finder 對「終端機」按「取得資訊」，取消勾選「使用 Rosetta 打開」。

- **程式更新了**
  重新下載最新的 zip 就好。（從原始碼跑的話：兩個資料夾都 `git pull`，再執行 `./run-bench.command --rebuild`。）

- **某一項失敗**
  工具會自動重跑一次。還是失敗就跳過，其他項目照跑，失敗原因記在 `results.json` 的 `failures`。

---


## 附錄：從原始碼自己編譯（一般不需要）

只有想測自己改過的程式才需要。預先編譯包已經用跟正式產品一樣的「預先編譯 Metal shader」。

#### 1. 安裝 Apple 開發工具（只要裝一次）

打開「終端機」（在「應用程式 → 工具程式」裡面），貼上這行，按 Enter：

```
xcode-select --install
```

會跳出安裝視窗，按「安裝」，大約 5–10 分鐘。這裡面已經包含 git 和 python3。

> **選擇性：** 如果你已經裝了完整的 Xcode，GPUI 會用跟正式產品一樣的「預先編譯 shader」。沒裝也能測，工具會自動改成「執行時編譯 shader」，只是 GPUI 的啟動時間會稍微多一點。結果檔會記錄這次用的是哪一種。
> 有 Xcode 的話，請再執行：
> ```
> sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
> xcodebuild -downloadComponent MetalToolchain
> ```

#### 2. 安裝 Rust（只要裝一次）

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

裝好之後，**關掉終端機再開一個新的**。

#### 3. 下載兩個程式的原始碼

```
mkdir -p ~/xq-mac && cd ~/xq-mac
git clone -b mac-bench https://github.com/tina-moneydj/gpui-xq-demo.git
git clone -b mac-bench https://github.com/tina-moneydj/egui-xq-demo.git
```

- `egui-xq-demo` 是**私人 repo**。第二行會要你輸入 GitHub 帳號：
  - Username 填 `tina-moneydj`
  - Password **不是**登入密碼，要填 GitHub 的 Personal Access Token
- 如果不想用 token，可以改用瀏覽器下載：
  1. 登入 GitHub，打開 tina-moneydj/egui-xq-demo。
  2. 左上角分支切到 `mac-bench`。
  3. 按綠色的 **Code**，選 **Download ZIP**。
  4. 解壓縮，資料夾改名成 `egui-xq-demo`，放進 `~/xq-mac/`。
  5. 打開終端機，執行 `xattr -dr com.apple.quarantine ~/xq-mac`，清除「從網路下載」的標記。

完成後，兩個資料夾要放在同一層：

```
~/xq-mac/gpui-xq-demo
~/xq-mac/egui-xq-demo
```

#### 4. 開始測試

```
cd ~/xq-mac/gpui-xq-demo/bench/mac
./run-bench.command
```

也可以在 Finder 裡雙擊 `run-bench.command`。

- **第一次**執行會先編譯三支程式，大約 10–20 分鐘，畫面會一直捲動文字，是正常的。之後再跑就不用重新編譯。
- 編譯完會自動開始測試。畫面上會顯示預估時間，測試視窗會自己開、自己關。
- 測試期間 Mac 不會進入睡眠，跑完就恢復原本的設定。

從原始碼跑的話，第一次會先編譯三支程式，大約 10–20 分鐘，之後再跑就不用重新編譯。

---

## 給工程師

- **參數**
  ```
  python3 xq_bench_mac.py --help
  ```
  常用的有：
  - `--apps gpui,gpui-quads,egui-wgpu,egui`（`gpui-quads`＝GPUI 加 `XQ_DRAW_QUADS=1`，只跑重圖表）
  - `--scenarios main,stress,heavy,ticks,multi`
  - `--ticks s1k,s10k,s50k,s100k,burst`
  - `--windows 1,4`
  - `--layers 100`（設 1 是不分層，當對照組）
  - `--runs 3`
  - `--port`
  - `--bin-dir`
  - `--no-footprint`
- **行情來源：**
  - 主視窗、5 萬列、重圖表用 `xq-demofeed`。它是 `WryFeedHost --demo-ticks` 的 Rust 版，同一個協議、同樣每 25ms 一批，Mac 上不需要 .NET。資料是合成的，不是真實行情。
  - 高頻、多視窗用 `xq-tickgen`。
- **視窗位置：**
  - 兩個 app 都讀環境變數 `XQ_WIN_RECTS="x,y,w,h,displayId,dx,dy;…"`，自己把視窗開在指定的螢幕和大小。
  - 用的是全域座標，單位是點，主螢幕左上角是 (0,0)。
  - 所以不需要「輔助使用」權限。
- **macOS 的程式改動**（只在 mac-bench 分支）：
  - GPUI：
    - 字型：蘋方-繁，等寬用 Menlo
    - 自身 RSS：`proc_pidinfo`
    - 新增 `runtime-shaders` feature
  - egui：
    - 字型：CoreText 找出蘋方-繁的檔案再 mmap
    - 自身 RSS：`proc_pidinfo`
  - 兩邊都加了 `XQ_WIN_RECTS`。
- **還沒做：** 高頻情境的「點報價列切換」反應時間。要自動點擊得開「輔助使用」權限，這版先略過。
- **CI：** `.github/workflows/macos.yml`（gpui-xq-demo、egui-xq-demo 各一份，推 `mac-bench` 分支就會跑）：
  - gpui-xq-demo：在 macos-26（Xcode 26）上編 GPUI＋`xq-tickgen`＋`xq-demofeed` 的 arm64＋x86_64 通用執行檔，預先編譯 Metal shader，ad-hoc 簽章，打包成 `xq-bench-mac-gpui.zip`，發佈公開預發行版 `mac-bench-latest`；並實際跑一輪 GPUI 主視窗＋重圖表當冒煙測試（artifact `smoke-mac-gpui`）。
  - egui-xq-demo：編 egui（預設只編 arm64，省私人 repo 的 macOS 分鐘數；手動執行可勾 universal），下載上面的 GPUI 包、加進 egui，發佈私人預發行版 `mac-bench-latest` 的 `xq-bench-mac.zip`；並跑一輪 egui 主視窗冒煙測試。
  - 兩個 repo 同時推時，egui 那邊可能拿到上一版 GPUI 包；要確定是最新的，等 gpui 的 run 跑完再推 egui（或手動重跑 egui 的 workflow）。
  - 下載的包會先清隔離標記（`xattr -dr com.apple.quarantine`）；`run-bench.command` 偵測到 `bin/` 已有執行檔就不會編譯。
- **自行編譯：** `./build-mac.sh` 只編本機的架構；`./build-mac.sh --universal` 用 lipo 合成通用執行檔。
