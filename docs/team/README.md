# 團隊文件（XQNext／XQ XT：Rust + GPUI 翻寫，全 AI 開發）

XQNext（原 WPF，尚未上線，面向海外華人）正在用 Rust + GPUI 對齊功能翻寫；之後 XQ XT 用同一套底層、不同介面翻寫。
這個資料夾是給加入的成員（多數是 C#／WPF 背景）用的：怎麼學、怎麼用 AI 開發、照什麼範例寫。

| 文件 | 給誰 | 內容 |
|---|---|---|
| [learning-roadmap.md](learning-roadmap.md) | 所有人 | 4 週學習路線（架構負責人深入線／功能開發者讀＋審線），每項都有本 repo 的練習、自我檢查、連結；C# → Rust／GPUI 對照表 |
| [ai-dev-guidelines.md](ai-dev-guidelines.md) | 所有人 | 開發流程（規格 → 測試／行為基準 → 實作 → 驗證 → 審查 → 合併）、AI 必守規則、審查清單、效能預算、WPF 行為對齊流程、模組負責人、分支與 PR 慣例 |
| [example-module.md](example-module.md) | 寫功能的人 | 範例模組「成交明細」的說明：五個模式、檔案、測試／bench 結果、照著做新面板的清單 |
| [prompt-templates.md](prompt-templates.md) | 用 AI 的人 | 8 個可直接複製的 prompt：WPF 截圖做面板、移植 C# 類別、寫行為基準、效能審查、修編譯錯誤、XS 功能、寫 bench、解釋 Rust 給 C# 開發者 |
| [`../../AGENTS.md`](../../AGENTS.md) | AI 代理 | 給 AI 看的精簡規則（`CLAUDE.md`、`.cursor/rules/project.mdc` 都指向它） |

範例程式碼：

- 純邏輯：[`crates/xq-tape/`](../../crates/xq-tape/)（單元測試、行為基準 `tests/baseline/`、micro-bench `benches/`）
- GPUI 面板：[`src/tick_tape.rs`](../../src/tick_tape.rs)（`XQ_TAPE=1` 開啟）
- 既有的參考實作：行情協議 [`crates/xq-feed`](../../crates/xq-feed/)、背景合併 [`src/bridge.rs`](../../src/bridge.rs)、虛擬化報價表 [`src/table.rs`](../../src/table.rs)、三層快取走勢圖 [`src/chart.rs`](../../src/chart.rs)、增量指標 [`src/indicators.rs`](../../src/indicators.rs)

建議第一天：讀這頁 → `AGENTS.md` → 照 `learning-roadmap.md`〈第 0 天〉把專案跑起來 → 讀 `example-module.md` 並把成交明細開起來玩。
