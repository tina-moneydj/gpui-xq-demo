# XQ 看盤 demo：在 iPhone 上跑（GPUI 版／egui 版）

## 先講結論

- **兩個 app 都有 iOS 版了**（`mobile-ios` 分支）：GPUI 版叫「XQ GPUI」，egui 版叫「XQ egui」。功能跟 Android 版一樣：報價表（可捲動、點一列進走勢圖）、走勢圖（單指拖、雙指縮放、長按十字線）、搜尋框、上方 FPS／記憶體、下方封包計數。顏色、字級用同一份設計 token。
- **手機上沒有電腦的行情服務，所以 app 自己內建一個「合成行情」**（假資料，會一直跳價）。畫面上看到的價格**不是真的行情**。
- **中文字用 iPhone 內建的「蘋方」（PingFang TC）**，數字用 Menlo。字型檔是直接對應（mmap）系統檔案，不會整個複製進記憶體。
- **要放到你自己的 iPhone 上，最簡單的方式是「免費 Apple ID」**（下面的「路線一」）：一條 USB 線＋Xcode，按一個 ▶。缺點是**每 7 天要重新按一次 ▶**（免費帳號的限制）。
- 公司如果有**付費的 Apple Developer 帳號**，可以改走 **TestFlight**（「路線二」，附錄 B）：之後手機上用 TestFlight app 安裝／更新，不用接線、90 天才過期。這需要公司帳號的管理員給幾個金鑰，現在還沒設。
- 我這邊**只在 GitHub 的雲端 Mac 上編譯、在 iPhone 模擬器裡開起來截圖**過；**還沒有在真的 iPhone 上試過**。第一次在你的手機上跑，如果有問題，請截圖給我。

> 目前狀態（雲端建置、截圖、已知問題）見最後的「[已知問題](#已知問題)」與「[雲端建置結果](#雲端建置結果)」。

---

## 你需要準備

| 東西 | 說明 |
|---|---|
| Mac | Apple 晶片（M1 以後）或較新的 Intel Mac 都可以；要能裝最新版 Xcode |
| Xcode | App Store 免費下載，大約 10～15 GB，下載要一段時間 |
| iPhone＋傳輸線 | 路線一才需要。iPhone 要 iOS 16 以後 |
| Apple ID | 就是你平常 iPhone 用的那個帳號即可（免費） |
| 網路 | 第一次會下載 Rust 編譯工具和程式庫，約 1～2 GB |

---

## 第 0 步：安裝 Xcode（只做一次）

1. 打開 Mac 的 **App Store**，搜尋 **Xcode**（藍色圖示、上面有一支鐵鎚）。按「取得」→「安裝」。等它下載完（可能要 30 分鐘以上）。
2. 從「應用程式」資料夾或 Launchpad 打開 **Xcode**。
   - 第一次會跳出**授權條款**：按「Agree（同意）」，再輸入 Mac 的登入密碼。
   - 接著會出現 **「Select the platforms you want to develop for」**（要裝哪些平台）的視窗：**勾 iOS**（macOS 已內建），按「Download & Install」。等進度條跑完。
   - 最後會看到「Welcome to Xcode」的歡迎視窗，左邊有「Create New Project…」等按鈕。到這裡就可以把 Xcode 關掉了（左上角選單 Xcode → Quit Xcode）。

---

## 第 1 步：一行指令，自動準備好一切

這一步只要**貼一行字**，其他都自動完成：下載程式碼、安裝 Rust、先編譯一次、打開 Xcode 專案。

1. 打開「**終端機**」（Terminal）：按 `⌘ 空白鍵` 叫出 Spotlight，打「終端機」或「Terminal」，按 Enter。會出現一個白色或黑色、有一行文字游標的小視窗。
2. 把下面**整行**複製、貼到終端機裡，按 **Enter**：

   GPUI 版：
   ```
   curl -fsSL https://raw.githubusercontent.com/tina-moneydj/gpui-xq-demo/mobile-ios/bench/ios/get.sh | bash -s gpui
   ```
   egui 版（想兩個都裝，就等第一個跑完再貼這行）：
   ```
   curl -fsSL https://raw.githubusercontent.com/tina-moneydj/gpui-xq-demo/mobile-ios/bench/ios/get.sh | bash -s egui
   ```
3. 畫面會一段一段出現藍色的「▶ 1/5 …」「▶ 2/5 …」到「▶ 5/5 打開 Xcode 專案」。
   - **第一次大約 10～20 分鐘**（主要是下載和編譯 Rust）。期間會一直有字在跑，這是正常的。
   - 如果出現**紅色的 ✗**，照它的說明做（通常是「還沒打開過 Xcode」或網路問題），然後再貼一次同一行。
4. 跑完時 **Xcode 會自動打開**，終端機最後顯示「完成！」。按 Enter 關掉終端機視窗即可。

> 程式碼放在你的「家目錄 → XQ-iOS」資料夾。**之後要更新到最新版，再貼一次同一行就好**（你選過的開發團隊會保留）。
>
> 進階：資料夾裡的 `mobile/ios/setup-mac.command` 也可以直接雙擊重跑第 1 步。第一次雙擊時 macOS 可能說「無法打開，因為來自未識別的開發者」：到「系統設定 → 隱私權與安全性」，往下捲，按「強制打開／仍要打開」。用上面那一行指令就不會遇到這個。

---

## 第 2 步（先試試看）：在 Mac 上的 iPhone 模擬器跑

不用接手機，先確定一切正常。

1. 看 Xcode 視窗**最上面中間**：有一個寫著「XQGPUI」（或「XQEGUI」）的按鈕，右邊接著一個裝置名稱（例如「Any iOS Device」或某支 iPhone）。
2. 點那個**裝置名稱**，會拉出一個清單，在「iOS Simulators」底下選一支，例如「**iPhone 17**」。
3. 按 Xcode **左上角的 ▶（三角形播放鍵）**。
   - 上方中間的狀態列會顯示「Building…」，然後「Running XQGPUI on iPhone 17」。第一次 Build 要幾分鐘。
   - 會跳出一個 iPhone 樣子的**模擬器**視窗，開機後自動打開 app。
4. 你應該看到：深色背景，最上面一列寫「XQ GPUI 手機版」（或「XQ egui 手機版」）和綠色的「● 行情連線」，右邊是「FPS … RSS … MB」；下面是搜尋框和一長串股票報價，數字會一直跳。點任一列就會進走勢圖，左上角「‹ 報價」回來。
5. 要停止：按 Xcode 左上角 ▶ 旁邊的 **■（正方形）**。

> 模擬器的速度、記憶體跟真手機不一樣，**FPS／RSS 數字不能拿來代表手機**。

---

## 路線一：放到你自己的 iPhone（免費 Apple ID，每 7 天重裝）

### 1-1. 在 Xcode 登入 Apple ID（只做一次）

1. Xcode 最上面選單列：**Xcode → Settings…**（或按 `⌘ ,`）。
2. 上方一排圖示點 **Accounts**（人頭圖示）。
3. 左下角按 **＋** → 選 **Apple Account**（或 Apple ID）→ Continue → 輸入你的 Apple ID 和密碼（可能還要在 iPhone 上按「允許」並輸入 6 位數驗證碼）。
4. 登入後左邊清單會出現你的帳號，右邊「Team」那一欄會有一個「**你的名字 (Personal Team)**」。關掉這個設定視窗。

### 1-2. 在專案裡選你的團隊（只做一次）

1. Xcode 視窗**最左邊**一欄（檔案清單），點**最上面那個藍色圖示**「XQGPUI」（或「XQEGUI」）。
2. 中間會出現設定畫面。在中間偏左的小清單裡，「TARGETS」底下點「**XQGPUI**」（或「XQEGUI」）。
3. 中間上方一排分頁點「**Signing & Capabilities**」。
4. 確認「**Automatically manage signing**」有打勾。
5. 「**Team**」下拉選單選「**你的名字 (Personal Team)**」。
6. 下面「Bundle Identifier」已經自動填好一個像 `tw.moneydj.xqdemo.gpui.a1b2c3` 的名稱（後面是亂數，避免跟別人撞名），**不用改**。
7. 等幾秒，「Signing Certificate」那行變成「Apple Development: 你的 Apple ID」就好了。如果出現紅字錯誤，按旁邊的「Try Again」。

### 1-3. 接上 iPhone、打開「開發者模式」（只做一次）

1. 用傳輸線把 iPhone 接到 Mac，**解鎖 iPhone**。
2. iPhone 跳出「**要信任這部電腦嗎？**」→ 按「**信任**」→ 輸入 iPhone 密碼。
3. 回到 Xcode，上方中間的裝置清單裡，最上面「iOS Device」底下會出現你的 iPhone 名字，**選它**。
   - 第一次 Xcode 會顯示「Preparing iPhone for development」（準備中），可能要幾分鐘。
4. iPhone 上打開「**設定 → 隱私權與安全性**」，捲到最下面，點「**開發者模式**」，打開開關。
   - 如果找不到「開發者模式」：先在 Xcode 按一次 ▶（會失敗沒關係），它就會出現。
5. iPhone 會要求**重新開機**→ 按「重新啟動」。開機後會再問一次「要打開開發者模式嗎？」→ 按「**打開**」→ 輸入密碼。

### 1-4. 安裝並打開 app

1. 確認 Xcode 上方裝置選的是你的 iPhone，按左上角 **▶**。
2. 第一次會出現兩種提示之一，照做：
   - Mac 上問要不要讓「codesign」使用鑰匙圈 → 輸入 **Mac 的登入密碼**，按「**永遠允許**」。
   - Xcode 說「**Could not launch "XQGPUI"**…未受信任的開發者」：app 已裝好但還不能開，做下一步。
3. iPhone 上：「**設定 → 一般 → VPN 與裝置管理**」→「開發者 App」底下點你的 Apple ID → 按「**信任「Apple Development: …」**」→ 再按一次「信任」。
4. 回 Xcode 再按一次 **▶**，app 就會在 iPhone 上打開。之後也可以直接點 iPhone 主畫面上的「XQ GPUI」圖示（紅色）或「XQ egui」圖示（藍色）。

### 1-5. 每 7 天：重新裝一次

免費 Apple ID 簽的 app **7 天後會打不開**（點了閃一下就關掉）。這時：接上 iPhone → 打開 Xcode（「最近使用」裡有這個專案，或再貼一次第 1 步的指令）→ 按 ▶。資料不會不見。

> 免費帳號其他限制：同時最多 3 個自己簽的 app；一週內新的 Bundle ID 最多 10 個（正常使用不會碰到）。

---

## 不用 Xcode 的快速試看：下載雲端編好的模擬器版

每次程式更新，GitHub 會自動編一個**模擬器用**的 app（**不能裝到真的 iPhone**）。

1. 打開 https://github.com/tina-moneydj/gpui-xq-demo/releases/tag/ios-latest （egui 版：https://github.com/tina-moneydj/egui-xq-demo/releases/tag/ios-latest ）。不用登入。
2. 在「Assets」底下點 **XQGPUI-simulator.zip**（或 XQEGUI-simulator.zip）下載，雙擊解壓縮，得到 **XQGPUI.app**。
3. 打開模擬器：Xcode 選單 **Xcode → Open Developer Tool → Simulator**。等 iPhone 畫面開機完成。
4. 把 **XQGPUI.app 拖到模擬器的 iPhone 畫面上**放開，就會安裝。在模擬器主畫面點它的圖示打開。
   - 也可以在終端機打：`xcrun simctl install booted ~/Downloads/XQGPUI.app`
5. 同一頁也有雲端模擬器開起來的截圖（`*-list.png`、`*-chart.png`）。

---

## 已知問題

| | GPUI 版 | egui 版 |
|---|---|---|
| 怎麼接 iOS | Longbridge gpui-mobile（已放進 repo，`mobile/vendor/gpui-mobile`）的「嵌入模式」，由我們的 Swift 外殼建立畫面 | eframe／winit 還不支援 iOS 27 規定的 UIScene，所以**不用 eframe**：Swift 外殼建 Metal 畫布，egui 核心＋egui-wgpu 直接畫 |
| 中文輸入（注音／拼音） | gpui-mobile 用系統的文字輸入元件接鍵盤，有處理「組字中」的字（注音／拼音應該可用），**待真機確認** | 只接了「打好的字」（UIKeyInput），**注音／拼音的組字列與選字可能不正常**；英數字、數字代號可用 |
| 「自訂指標腳本」 | 兩個 repo 目前**都還沒有這個功能**（任何分支都找不到）。手機版直接共用桌面版的程式碼，等桌面版做好就會自動帶進來 | 同左 |
| 失焦就停畫（Android 版的凍結問題） | 不看焦點、只看有沒有東西要畫；控制中心／通知蓋住時照畫，只有進背景才停。**需在真機上確認** | 同左 |
| 真機 | 還沒在真的 iPhone 上跑過 | 同左 |
| 行情 | 預設是 app 內建的**合成行情**（假資料）。要連 Mac 上的行情服務：Xcode → Product → Scheme → Edit Scheme… → Run → Arguments → Environment Variables 加 `XQ_FEED_ADDR` = `Mac的IP:47631`（iPhone 會問「區域網路」權限，按允許） | 同左 |
| 橫向 | 可以轉，但版面是照直向設計的 | 同左 |

其他技術備註（給開發的人）：

- Xcode 27／iOS 27 SDK 起，沒有 UIScene 生命週期的 app 會開不起來；兩個外殼都用 `UIApplicationSceneManifest`＋`SceneDelegate`。
- Rust 一律用 release 編（`mobile/ios/build-rust.sh`，Xcode 每次 Build 前自動跑；Scheme 的 Run 也設成 Release）。iOS 用 `cargo rustc --crate-type staticlib`，Android 的 `cdylib` 不受影響。
- Bundle ID／Team 放在 `mobile/ios/Local.xcconfig`（每台 Mac 自己的，不進 git）；`XQ.xcconfig` 是預設值。
- Xcode 專案由 XcodeGen 產生（`mobile/ios/project.yml`），產生好的 `.xcodeproj` 也放進 repo，所以不用裝 XcodeGen。
- 在 Linux 上可以先 `cargo check --lib --target aarch64-apple-ios-sim` 快速檢查（不用 Mac）。
- `mobile/src/lib.rs` 的 `bridge.taker().take()`：main 上 A1 合併後 `FeedBridge::take` 被拿掉，手機版（含 Android）在 main 上編不過，這個分支順便修了（GPUI）。

---

## 附錄 A：常見畫面與處理

| 你看到 | 怎麼辦 |
|---|---|
| 終端機：「還沒裝 Xcode」 | 做第 0 步，打開 Xcode 一次後再貼指令 |
| 終端機：「Xcode 還沒準備好」 | 打開 Xcode，同意授權並裝 iOS 平台 |
| Xcode 紅字：「Signing for "XQGPUI" requires a development team」 | 做 1-2，選 Personal Team |
| Xcode 紅字：「No profiles for '…' were found」或「Failed to register bundle identifier」 | 1-2 的畫面按「Try Again」；還不行就把 `XQ-iOS/…/mobile/ios/Local.xcconfig` 刪掉，再貼一次第 1 步的指令（會換一個新的亂數 Bundle ID） |
| Xcode：「Developer Mode disabled」 | 做 1-3 的第 4～5 點 |
| Xcode：「Could not launch … untrusted developer」 | 做 1-4 的第 3 點 |
| Xcode：Build 失敗，紅字有「Rust 編譯失敗」或「cargo」 | 把 Xcode 左邊的紅色錯誤（Report navigator，最右邊的對話框圖示）截圖給開發的人 |
| iPhone 上 app 一打開就閃退（之前可以） | 7 天到了，接上電腦按 ▶ 重裝 |

---

## 附錄 B：TestFlight（公司付費帳號，之後再設）

TestFlight 的好處：手機不用接線，用「TestFlight」app 安裝；每次 GitHub 有新版就自動上傳；一個版本 90 天才過期；可以邀請同事一起測。

**需要公司帳號管理員做的事（一次）**：

1. 在 [App Store Connect](https://appstoreconnect.apple.com) → 我的 App → ＋ 新增 App：平台 iOS、名稱（例：XQ GPUI demo）、Bundle ID（例：`tw.moneydj.xqdemo.gpui`，要先在 developer.apple.com 的 Identifiers 建立）。
2. App Store Connect → 使用者與存取權限 → 整合 → App Store Connect API → 產生一把 **API 金鑰**（權限選「App 管理」或「管理」，雲端自動簽章需要）。下載 `.p8` 檔（只能下載一次），記下 **Key ID** 和 **Issuer ID**。
3. 記下公司的 **Team ID**（developer.apple.com → Membership，10 碼英數字）。

**放到 GitHub（repo 管理者做，一次）**：repo → Settings → Secrets and variables → Actions：

| 種類 | 名稱 | 內容 |
|---|---|---|
| Secret | `APPLE_TEAM_ID` | Team ID |
| Secret | `ASC_KEY_ID` | API 金鑰的 Key ID |
| Secret | `ASC_ISSUER_ID` | Issuer ID |
| Secret | `ASC_KEY_P8` | `.p8` 檔內容做 base64（Mac 終端機：`base64 -i AuthKey_XXXX.p8 \| pbcopy`，再貼上） |
| Variable | `XQ_BUNDLE_ID_GPUI`（egui repo 是 `XQ_BUNDLE_ID_EGUI`） | 第 1 點的 Bundle ID |
| Variable | `XQ_TESTFLIGHT` | `1`（設了才會上傳） |

設好之後，`mobile-ios` 分支每次更新，CI（`.github/workflows/ios.yml` 的 `testflight` 工作）會：用雲端管理的發行憑證自動簽章 → 封存 → 上傳到 TestFlight。上傳後 App Store Connect 要處理 10～30 分鐘，再到 TestFlight 分頁把自己加進「內部測試」。

**你的 iPhone 上**：App Store 安裝「TestFlight」→ 打開收到的邀請信或連結 → 按「安裝」。

> 這一段**還沒有實際跑過**（需要公司帳號的金鑰）。金鑰不要貼在聊天室或 email 裡，直接由管理員填進 GitHub。

---

## 雲端建置結果

（每次 `mobile-ios` 更新都會自動跑，見 Actions 頁面的「ios」工作。）

- GPUI：https://github.com/tina-moneydj/gpui-xq-demo/actions/workflows/ios.yml
- egui：https://github.com/tina-moneydj/egui-xq-demo/actions/workflows/ios.yml
