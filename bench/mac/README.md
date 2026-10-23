# XQ Mac 壓測包（GPUI vs egui）

**這是什麼：** 在你自己的 Mac 上，用同一套情境比較兩種看盤 UI 寫法，量它們的速度、記憶體和 CPU：
- **GPUI**：Zed 的框架，用 Metal 畫
- **egui**：分兩種跑法
  - wgpu／Metal
  - glow／OpenGL（跟 Linux、Windows 版的測試一樣）

wry 已經淘汰，不在這次的比較裡。

**你只要做三件事：**
1. 照下面的步驟裝好工具。
2. 雙擊（或在終端機執行）`run-bench.command`。
3. 跑完把產生的 **zip 檔傳回來**。

跑完後，結果資料夾裡的 `summary.md` 會先寫結論，每一項的較佳者標 🏆。

> 目前還沒有任何 Mac 實測數字，這個工具包就是要量出第一份。

---

## 步驟

### 0. 測試前準備（很重要）

| 項目 | 怎麼做 |
|---|---|
| 電源 | 筆電**插上電源**。到「系統設定 → 電池」把「低耗電模式」關掉 |
| 其他程式 | 關掉瀏覽器、LINE、Teams、會議軟體、XQ。它們會搶 CPU 和 GPU |
| 螢幕 | 有外接螢幕就接上，解析度和縮放維持平常的設定，不用特別調整 |
| 測試中 | **不要動滑鼠和鍵盤**，也不要把終端機拖到測試視窗上面。被蓋住的視窗 macOS 會停止重畫，數字就不準了 |

### 1. 安裝 Apple 開發工具（只要裝一次）

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

### 2. 安裝 Rust（只要裝一次）

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

裝好之後，**關掉終端機再開一個新的**。

### 3. 下載兩個程式的原始碼

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

### 4. 開始測試

```
cd ~/xq-mac/gpui-xq-demo/bench/mac
./run-bench.command
```

也可以在 Finder 裡雙擊 `run-bench.command`。

- **第一次**執行會先編譯三支程式，大約 10–20 分鐘，畫面會一直捲動文字，是正常的。之後再跑就不用重新編譯。
- 編譯完會自動開始測試。畫面上會顯示預估時間，測試視窗會自己開、自己關。
- 測試期間 Mac 不會進入睡眠，跑完就恢復原本的設定。

### 5. 傳回結果

跑完後，Finder 會自動選取結果 zip，檔名像 `results/20261024-101500-MacBook-Pro.zip`。**把這個 zip 傳回來就好。**

---

## 要多久

| 模式 | 指令 | 時間（編譯另計） | 內容 |
|---|---|---|---|
| 完整（預設） | `./run-bench.command` | 約 75 分鐘 | 3 輪中位數、三種 app、五大情境全跑 |
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

結果在 `bench/mac/results/<日期時間>-<電腦名稱>/`，旁邊還有一個同名的 `.zip`。

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
  xattr -dr com.apple.quarantine ~/xq-mac
  ```
  用 `git clone` 下載的不會有這個問題。

- **雙擊 run-bench.command 沒反應，或顯示「沒有權限」**
  在終端機執行 `chmod +x ~/xq-mac/gpui-xq-demo/bench/mac/*.command ~/xq-mac/gpui-xq-demo/bench/mac/*.sh`。

- **跳出「是否允許接受連入網路連線」**
  測試只用本機的 127.0.0.1，按「允許」或「拒絕」都不影響結果。

- **「127.0.0.1:47631 已被佔用」或「這些程式正在執行」**
  先關掉 XQ 或上次沒結束的測試程式，或改用別的埠：`./run-bench.command --port 47731`。

- **終端機在 Rosetta 下執行**
  工具會提醒你。請到 Finder 對「終端機」按「取得資訊」，取消勾選「使用 Rosetta 打開」。

- **想重新編譯**（例如程式更新了）
  先執行 `git pull`（兩個資料夾都要），再執行 `./run-bench.command --rebuild`。

- **某一項失敗**
  工具會自動重跑一次。還是失敗就跳過，其他項目照跑，失敗原因記在 `results.json` 的 `failures`。

---

## 給工程師

- **參數**
  ```
  python3 xq_bench_mac.py --help
  ```
  常用的有：
  - `--apps gpui,egui-wgpu,egui`
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
- **CI：** `ci/macos.yml` 是 GitHub Actions 的工作流程：
  - 在 macos-26 上產生 arm64＋x86_64 通用執行檔，加 ad-hoc 簽章，打包成 zip。
  - 推送用的 token 沒有 `workflow` 權限，所以還沒放進 `.github/workflows/`，也還沒啟用。
  - 有權限的人執行 `git mv bench/mac/ci/macos.yml .github/workflows/` 就能啟用。
  - 用 CI 產生的 zip 時：
    1. 先執行 `xattr -dr com.apple.quarantine <資料夾>`。
    2. 把 `egui-xq-demo` 放進 `bin/`。
    3. 執行 `python3 xq_bench_mac.py`。
- **自行編譯：** `./build-mac.sh` 只編本機的架構；`./build-mac.sh --universal` 用 lipo 合成通用執行檔。
