# 網頁版內嵌字型

- `NotoSansTC-subset.otf`（約 3.0 MB）：Noto Sans CJK TC（`NotoSansCJK-Regular.ttc` 第 3 個字面）的子集——
  Big5 一級常用字 5,401 字＋Big5 符號區（標點、全形、注音符號 ㄅ–ㄩ 與聲調）＋程式碼裡出現的所有字，共約 6,760 字。
  用 fontTools `pyftsubset --no-hinting` 產生（指令見上層 README「網頁版」）。
- `NotoSansMonoTC-ascii.otf`（12 KB）：Noto Sans Mono CJK TC 的 ASCII＋常用符號子集（數字欄用）。

授權：Noto CJK 為 SIL Open Font License 1.1（https://openfontlicense.org），子集為衍生字型，仍適用 OFL。
不在子集內的字（Big5 二級罕用字等）會顯示成方塊；要完整字集就改成執行時下載完整字型（約 16 MB）。
