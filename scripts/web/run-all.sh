#!/usr/bin/env bash
# 網頁版全套：3 輪 × {main, stress, heavy, ticks×5} × {egui, gpui}，同一輪內兩個 app 輪流（減少時段差）。
# 用法：OUT=/path run-all.sh [runs=3]；結果在 $OUT/results.jsonl，摘要用 summarize.py。
set -u
RUNS="${1:-3}"; OUT="${OUT:?OUT=輸出資料夾}"; HERE="$(cd "$(dirname "$0")" && pwd)"
MODES="${MODES:-main stress heavy ticks:s1k ticks:s10k ticks:s50k ticks:s100k ticks:burst}"
APPS="${APPS:-egui gpui}"
mkdir -p "$OUT"; echo "START $(date '+%F %T %z')" >> "$OUT/progress.log"
for r in $(seq "${FIRST_RUN:-1}" "$RUNS"); do
  for m in $MODES; do
    for a in $APPS; do
      echo "$(date '+%T') run=$r $a $m load=$(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/progress.log"
      timeout 180 node "$HERE/bench-web.js" "$a" "$m" "$r" "$OUT" >> "$OUT/progress.log" 2>&1 || echo "  ！exit $?" >> "$OUT/progress.log"
      sleep 2
    done
  done
done
echo "DONE $(date '+%F %T %z')" >> "$OUT/progress.log"
