# vendored gpui-mobile：來源與修補

來源：[longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile) `c24a130`（2026-10-14），授權沿用原專案
（GPL-3.0-or-later OR AGPL-3.0-or-later OR Apache-2.0，見 LICENSE-*）。只複製 `src/`、`Cargo.toml`、授權與 README；
`vendor/gpui-pre-wgpu`（只給 `offscreen` 功能用）沒帶，`offscreen` feature 與 `[patch.crates-io]` 也拿掉
（路徑依賴時 cargo 會檢查 feature 指向的套件，而那個 patch 只在本 crate 當 workspace 根時才生效）。

所有修補都標了 `修補（xq）`，可用 `rg "修補（xq）"` 找到。

## 1. 失焦就停畫（畫面凍住的原因）— `src/android/jni.rs`

**現象**：報價照收、指標照算，fps 掉到 0 而且不會恢復；heavy 壓測卡在某一段（各段靠畫幀推進）。

**原因**：主迴圈只在 `INIT_WINDOW_DONE && app_is_active` 時畫，而 `app_is_active` 跟著**輸入焦點**
（`MainEvent::LostFocus` → `set_active(false)`）。Android 上「沒焦點但還在畫面上」很常見：
系統／ANR 對話框、通知欄、多視窗、輸入法。模擬器上 SystemUI 常 ANR 被殺、重啟，之後
InputDispatcher 的 `FocusedWindows: <none>`，app 收到 `LostFocus` 卻永遠等不到 `GainedFocus`，
GPUI 就一直不畫（WindowManager 的 `mCurrentFocus` 仍是本 app）。egui 失焦照畫，所以沒有這問題。

另一個小問題：`PAUSE_PENDING`／`RESUME_PENDING` 分開處理、固定先 Pause 後 Resume，同一輪
先 `GainedFocus` 後 `LostFocus` 會被套成 active（剛好掩蓋了上面的問題），反過來則相反。

**修補**：
- 新增 `VISIBLE`（`Start`／`Resume` 設 true、`Stop` 設 false）；能不能畫改成
  `INIT_WINDOW_DONE && (app_is_active || VISIBLE)`。surface 不在時 `INIT_WINDOW_DONE` 本來就是 false。
- 焦點／前景事件改成「最後一個為準」（`ACTIVE_REQUEST`），`set_active` 仍照焦點更新給 GPUI 用。
- `src/android/host.rs`（Java 端主導的迴圈，本 app 沒用）有同樣的 `can_draw = win.is_active()`，沒改，列在 upstream issue。

## 2. 系統字型整檔讀進 heap — `src/android/platform.rs`、`Cargo.toml`

**原因**：`AndroidPlatform::new` 用 `std::fs::read` 把約 25 個 /system/fonts 檔讀進 heap
（本模擬器實際存在 11 個：NotoSansCJK-Regular.ttc 一個就 32,355,424 bytes），`DroidSans.ttf`、
`DroidSans-Bold.ttf` 是 `Roboto-Regular.ttf` 的捷徑，同一檔讀三次；為了看 NotoColorEmoji.ttf 有沒有 CBDT
表，又整檔（2.8 MB）讀一次。

**修補**：
- 預設改用 `memmap2` 唯讀映射並 leak 成 `&'static [u8]`，以 `Cow::Borrowed` 交給 `add_fonts`
  （`gpui-pre-wgpu` 0.3.9 的 `add_fonts` 只包 `Arc`，不複製）。檔案頁按需載入、跟其他程序共用、可回收。
- 依 `canonicalize` 後的實體路徑去重。
- emoji 表頭檢查也用 mmap。
- `GPUI_ANDROID_FONTS=檔名,…` 只載入指定字型；`GPUI_ANDROID_FONT_LOAD=read` 走原本行為（A/B 用）。

## 3. 診斷 — `src/android/frame_source.rs`、`src/frame_pacer.rs`

`XQ_PACER_LOG=1` 時 `gpui-pacer-log` 執行緒每秒印 `PACER|{posts,vsyncs,frames,sinceFrameMs,posted,due,canDraw,…}`，
平常不啟動、零成本（只有幾個 Relaxed atomic 計數）。

## 要給 upstream 的 issue（還沒開，等 Tina 決定）

**Title**：Android: rendering stops for good when the activity loses input focus while still visible

> On Android, `run_event_loop` only draws while `INIT_WINDOW_DONE && app_is_active`, and `app_is_active`
> follows input focus (`MainEvent::LostFocus` → `set_active(false)`). A visible-but-unfocused activity
> (system/ANR dialog, notification shade, multi-window, or after SystemUI restarts and InputDispatcher
> reports `FocusedWindows: <none>`) therefore stops drawing; if `GainedFocus` never comes the window stays
> frozen while the app keeps running. Repro: emulator under load, SystemUI ANR → killed → app gets
> `LostFocus` with no `GainedFocus`; `dumpsys window` still shows the app as `mCurrentFocus`.
> Also, `PAUSE_PENDING`/`RESUME_PENDING` are applied pause-then-resume each iteration, so the final state
> can contradict the event order. Suggested fix: gate drawing on visibility (`Start`/`Stop`) + surface, not
> focus; apply only the latest focus request. Same gate exists in `android/host.rs` (`can_draw = win.is_active()`).
> Patch: see this repo's `mobile/vendor/gpui-mobile` (`rg "修補（xq）"`).

**Title**：Android: system fonts are read into the heap (~40 MB with Noto CJK)

> `AndroidPlatform::new` reads every listed /system/fonts file with `std::fs::read` (NotoSansCJK alone is
> 32 MB; DroidSans*.ttf are symlinks to Roboto-Regular.ttf, so it is read three times) and reads
> NotoColorEmoji.ttf fully just to look at the table directory. `add_fonts` accepts `Cow<'static, [u8]>`
> and does not copy, so memory-mapping the files (memmap2, leaked `&'static [u8]`) makes them file-backed,
> shared and reclaimable. Dedupe by canonical path. Optionally an env/config allow-list.

## 4. iOS：關掉預設 feature 時編不過 — `src/ios/platform_view.rs`

`default-features = false` 時 `packages::video_player`／`packages::camera` 不存在，但 iOS 的 platform view 直接呼叫
`ios_get_player`／`ios_get_session`。改成 `#[cfg(feature = …)]`，沒開時當作找不到（原生 view 照建、只是不掛播放器／相機預覽）。

## 5. iOS 26：所有字型都「Failed to read traits」→ 找不到字型而 panic — `src/ios/text_system.rs`

**現象**（GitHub macOS runner、Xcode 26.6、iOS 26.2 模擬器）：啟動後立刻
`failed to resolve font 'PingFang TC' or any of the fallbacks: … Helvetica … Arial` panic，app 閃退。
log 顯示 PingFang TC、Helvetica、Arial 的每個字面都 `Failed to read traits`。

**原因**：`load_family` 為了避免 font-kit `Font::properties()` 的 unwrap panic，先檢查
`CTFontCopyTraits` 有沒有 symbolic／width／weight／slant 四個數字；iOS 26 的字典少了其中某些鍵，
所以每個字型都被跳過。

**修補**：不再跳過；`font_id` 改用 `lenient_properties()`（缺的鍵用預設值：一般粗細、正常寬度、不斜體；
粗細換算跟 font-kit 同一張表）。缺哪些鍵用 `log::debug!` 記下。

## 6. iOS 27（Xcode 27）：第一次畫畫面就中止 — `src/ios/ffi.rs`、`src/ios/window.rs`、`src/ios/text_system.rs`

**現象**（Tina 的 Mac，Xcode 27、iPhone 18 Pro／iPhone Duo 模擬器）：app 一開就停住，Xcode 顯示
`Thread 1: hit program assert`，堆疊 `viewDidLayoutSubviews → gpui_ios_request_frame → … → __pthread_kill`；
行情執行緒（xq-feed、xq-demofeed）已經在跑。雲端 runner 只有 Xcode 26.6／iOS 26.2，同一份程式正常。

**原因**：`gpui_ios_request_frame` 是 `extern "C"`，GPUI 畫第一幀時只要有任何 panic，Rust 不能讓它跨過 C 邊界，
就直接 abort（所以只看到 assert，看不到哪裡錯）。確切的 panic 訊息在 Xcode 主控台，雲端重現不了，還沒拿到。

**修補**：
- `guarded()`／`catch_unwind`：`request_frame`、`layout_view`、`handle_touch`、四個生命週期入口、`run_demo` 的 panic 都接住；
  訊息（app 的 panic hook 呼叫 `record_panic`，含檔名行號）由新的 `gpui_ios_fatal_error()` 取得，
  Swift 外殼停掉 CADisplayLink、在畫面上顯示紅底訊息。`request_frame` 一定把 callback 放回去，連續 3 次 panic 才判定失敗。
- `handle_layout_change` 的縮放倍率改用 `view.traitCollection.displayScale` → `view.window.screen.scale` → `UIScreen.mainScreen`，
  都是 0 就沿用舊值（UIScene 下 `mainScreen` 已淘汰）；`is_active` 改問 `view.window.isKeyWindow`，不用淘汰的 `UIApplication.keyWindow`。
- `font_id`：家族載不到（或每個字面都被跳過）時不回錯（GPUI 收到錯誤會 panic），改用 CoreText 依名稱建字型，
  再退到 PingFang TC／Helvetica／系統 UI 字型。
