# gpui-fast 實驗（2026-10-25，不合併）

把 GPUI 換成長橋的 [gpui-fast](https://github.com/longbridge/gpui-fast)（commit `124f79b`，用它的 `compat/gpui-pre` 同名同版替身，`[patch.crates-io]` 三行），程式碼一行沒改就能編譯，測試全過。

## 方法

- Windows CI（`windows-profile.yml` 矩陣）：同一份程式碼編三版各取樣 25 秒，兩情境（main＝一般畫面、stress＝5 萬列 × 6 欄、每秒 3000 筆跳價＋捲動），跑兩輪。
  - `main`：main 的 Cargo.toml/lock（gpui-pre 0.3.7）
  - `pre039`：上游 gpui-pre 0.3.9（隔離「升版」本身的效果）
  - `fast`：gpui-fast（patch 成 gpui-pre 0.3.9）
- CI 沒 GPU（WARP 軟體繪圖），全程序 CPU 被 WARP 主導；只看主執行緒。
- run：[第 1 輪](https://github.com/tina-moneydj/gpui-xq-demo/actions/runs/40236117899)、[第 2 輪](https://github.com/tina-moneydj/gpui-xq-demo/actions/runs/40237269449)

## 結果：主執行緒 CPU（ms／約 27 秒取樣，兩輪）

| 情境 | main（0.3.7） | 上游 0.3.9 | gpui-fast | fast vs main | fast vs 0.3.9 |
|---|--:|--:|--:|--:|--:|
| 一般畫面 | 2240 / 2178 | 1587 / 1929 | 1607 / 1183 | −37% | −21% |
| 5 萬列壓測 | 2580 / 2305 | 1814 / 2278 | 1599 / 1365 | −39% | −28% |

- 三版 fps 都約 30（WARP 卡住的上限），所以每幀成本也是同比例下降。
- taffy 排版：main 約 14–19% → gpui-fast 約 1%（版面節點跨幀保留，效果最明顯）。
- gpui-fast 的 chart 重畫次數少約 30%（約 90 vs 125 次／秒），畫面一樣。
- 注意：上游 0.3.9 把「送 GPU（renderer.draw）」放在主執行緒（約 15%），gpui-fast 在主執行緒上看不到這段（應是移到別的執行緒），所以「fast vs 0.3.9」有一部分是搬走、不是省掉。
- 全程序 CPU（含 WARP）三版差不多（約 290–310%），CI 看不出 GPU 端有沒有變輕。
- 兩輪差距最大到 ±20%，CI 機器雜訊大；方向一致，但精確比例要在實機重量。

## 其他觀察

- 雲端 Linux（Xvfb＋llvmpipe）跑 gpui-fast 會卡住：fps=1、行情一直 Connecting；上游版正常。Windows 正常。gpui-fast 在 X11 無合成器的環境可能有相容性問題，未深究。
