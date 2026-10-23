#!/bin/bash
# XQ 壓測 macOS 版：在 Finder 雙擊這個檔案就會開終端機執行。
# 第一次會先編譯（build-mac.sh），之後直接測。參數會傳給 xq_bench_mac.py，例如：./run-bench.command --quick
cd "$(dirname "$0")" || exit 1
echo "=== XQ 壓測 macOS 版（GPUI vs egui）==="
if ! /usr/bin/xcrun --find python3 >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
  echo "需要 python3：請先執行  xcode-select --install  （Command Line Tools 內含 python3），裝好再雙擊一次。"
  read -r -p "按 Enter 關閉…" _; exit 1
fi
PY="$(command -v python3)"
need_build=0
for b in gpui-xq-demo egui-xq-demo xq-tickgen xq-demofeed; do [ -x "bin/$b" ] || need_build=1; done
for a in "$@"; do [ "$a" = "--rebuild" ] && need_build=1; done
if [ $need_build = 1 ]; then
  ./build-mac.sh || { echo; echo "編譯失敗，請把上面的訊息截圖傳回來。"; read -r -p "按 Enter 關閉…" _; exit 1; }
fi
args=(); for a in "$@"; do [ "$a" = "--rebuild" ] || args+=("$a"); done
# 測試期間不讓螢幕關閉／睡眠（caffeinate 只在這次執行期間有效，跑完自動恢復）
caffeinate -dimsu "$PY" xq_bench_mac.py ${args[@]+"${args[@]}"}
rc=$?
echo
[ $rc = 0 ] && echo "跑完了。結果 zip 已在 Finder 裡選取，請把它傳回來。" || echo "結束碼 $rc（若有錯誤訊息請截圖傳回來）。"
read -r -p "按 Enter 關閉…" _
