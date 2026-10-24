# xq-tokens — XQ 共用設計 token

**一句話**：設計師改 `tokens/` 裡的 JSON（或從 Figma 匯出蓋過去），egui 版和 GPUI 版的顏色、字級、圓角、線寬、版面尺寸就一起變，不用改程式。

## 檔案

| 檔案 | 內容 | 對應 Figma |
|---|---|---|
| `tokens/color.dark.tokens.json` | 深色主題全部顏色（83 個） | 「Color」collection 的 **Dark** mode |
| `tokens/color.light.tokens.json` | 淺色主題全部顏色（83 個，名稱和深色一一對應） | 「Color」collection 的 **Light** mode |
| `tokens/base.tokens.json` | 字型、字級、間距、圓角、線寬、版面尺寸（33 個，深淺共用） | 「Base」collection（單一 mode） |
| `tokens/xq.resolver.json` | 告訴程式「base ＋ 依主題選 dark／light」 | 不用動 |
| `tools/gen_tokens.py` | 第一次從程式碼盤點產生上面三個檔（之後不用再跑） | — |

格式是 **W3C Design Tokens（DTCG）2025.10**——2025-10-28 發布的第一個穩定版（Format／Color／Resolver 三個模組）。
顏色寫成 `{"colorSpace":"srgb","components":[…],"alpha":1,"hex":"#ff4d4f"}`，尺寸寫成 `{"value":22,"unit":"px"}`，
別名寫 `"{color.ui.accent}"`。

## 設計師流程（Figma）

1. Figma 的 Variables 開兩個 collection：**Color**（兩個 mode：Dark、Light）和 **Base**。變數名稱用斜線分層，跟 JSON 路徑一樣，例如 `color/market/up`、`layout/tableRow`。
   第一次可以直接把本資料夾的三個 JSON **拖進 Variables 視窗匯入**（Figma 原生支援 DTCG 匯入），名稱就對好了。
2. 改顏色／尺寸。
3. 在 collection 上按右鍵 → **Export modes**，每個 mode 會得到一個 JSON；改名成 `color.dark.tokens.json`、`color.light.tokens.json`、`base.tokens.json`，蓋掉本資料夾的檔案，送 PR。
4. 工程師（或 CI）`cargo build`：`build.rs` 會先檢查每個主題是不是每個 token 都在、格式對不對；有錯會**編譯失敗並列出 token 名稱**，例如 `[light] token「color.market.up」缺少`。
5. 想先看效果、不想等編譯：`XQ_TOKENS_DIR=/你的/tokens資料夾 XQ_TOKENS_WATCH=1 ./gpui-xq-demo`（egui 版同樣），存檔後約 0.3 秒畫面就換。

Figma 原生匯入／匯出的限制（Figma 說明文件）：顏色只吃 sRGB／HSL、尺寸只能 px、字型只能單一名稱、一個 mode 一個檔；
Figma REST API 拿不到 DTCG 格式（而且 Variables API 只有 Enterprise 能用），所以自動同步要靠外掛（Tokens Studio 有 W3C DTCG 格式選項）或手動匯出。
本資料夾的檔案都照這些限制寫，可以來回匯入匯出。

## 程式怎麼用

```rust
use xq_tokens::t;            // &'static Theme，熱路徑只是一次原子讀取
let up = t().market.up;      // 0xRRGGBB
let fill = t().chart.area_fill; // Rgba { rgb, a }
let row_h = t().layout.table_row; // f32 px
xq_tokens::toggle_mode();    // 深色 ↔ 淺色；generation() +1，App 看到就清快取重畫
```

- 內容在編譯時用 `include_str!` 內嵌（執行檔不需要附 JSON）；第一次呼叫 `t()` 解析（不到 1ms）。
- `XQ_THEME=light`：淺色啟動。`XQ_TOKENS_DIR`：改從磁碟讀。`XQ_TOKENS_WATCH=1`（或 debug 版）：熱更新。
- 新增 token：`src/theme.rs` 的 `group!` 加一行（欄位：種類 = "token 路徑"），三個 JSON 加同名 token。
- 支援：群組 `$type` 繼承、`$root`、`{…}` 別名（可串接）、整個 token 的 `$ref`、`$extensions`（忽略，Figma 的 `com.figma.*` 放這裡）。
  不支援（XQ 還用不到）：只引用部分欄位的 `$ref`、`$extends`、非 sRGB 色彩空間（有 `hex` 就沒問題）。

## 盤點（2026-10-23，兩版 main 寫死的值）

| 類別 | 原本寫在哪 | 個數 | 備註 |
|---|---|---|---|
| 介面色（底、面板、框線、文字、強調、選取） | 兩版 `theme.rs` | 8 | 兩版相同 |
| 行情色（漲紅 #ff4d4f、跌綠 #22c55e、平、閃爍底色、成交量、大單） | `theme.rs`、`table.rs`、`tick_tape.rs` | 7 | 閃爍底色原本是 `0x22` alpha 寫在表格程式裡 |
| 走勢圖色（K 棒漲 #e5484d／跌 #30a46c、格線、軸、十字線、價格標籤…） | `chart.rs` 常數＋散落的 16 進位 | 17 | **K 棒漲跌色和報價表漲跌色本來就是兩組**，保留成兩個 token |
| 疊加指標 20 色、副圖指標色（8 種） | `chart.rs` | 20＋13 | 副圖色多數是疊加色的別名 |
| 畫線（畫線色、選取、控制點、文字底、費波納契 7 色） | `drawings.rs` | 11 | |
| 工具列（提示框、按下、滑過、圖示三態、分隔線） | `chart.rs` | 7 | |
| 字型、字級 10／11／12／13／15 | `theme.rs`、各處 `text_size`／`FontId`／畫布文字 | 2＋5 | egui 讀字型**檔**（`XQ_CJK_FONT`），字型名稱 token 只有 GPUI 用 |
| 圓角 | GPUI `rounded_sm/md`（4／6px）、egui 按鈕 `corner_radius(2.0)` | 3 | **兩版按鈕圓角本來就不一樣**（GPUI 4px、egui 2px）：先各用一個 token 保持畫面不變，之後由設計決定統一 |
| 線寬 | egui 工具圖示 1.7、框線 1、選取中畫線 2 | 3 | 目前只有 egui 工具圖示線寬改讀 token；框線／選取線 token 已定義，程式還沒換 |
| 版面尺寸（標題列 26、狀態列 22、表格列高 22、成交明細 20／寬 276、價格軸 66、時間軸 18、工具列 34…） | 各檔常數 | 15 | |
| 間距 2／4／6／8／10 | 各處 `px(…)`、`item_spacing`、GPUI 的 `px_2()` 等 | 5 | **token 已定義，程式還沒全部換**（見下） |

刪掉的死碼：GPUI `theme.rs` 的 `grid()`（#1c2230）、`cross()`（#9aa4b2）、`ma_colors()` 沒有任何地方用（實際用的是 `chart.rs` 的那組）。

**還沒 token 化**（刻意留著，之後再收）：GPUI 的 `px_2()`／`gap_1()` 這類間距簡寫和零星的 `px(3.)`、egui 的 `item_spacing`、
畫布上的小幾何（價格標籤高 16、游標框高 22、控制點 9×9）、程式裡的透明度係數（成交量柱 `0xaa`、畫線填色 0.10／0.07）、視窗預設大小。
