#!/bin/bash
# S=<輸出資料夾> ./run.sh <執行檔> <標籤> [額外環境變數…]；本次：XQ_STRESS_TICK_MS=3000（跳價縮短）、捲動 10 秒，同 bench-main-window.sh 的捲動段
BIN=$1; L=$2; shift 2
cd "$(dirname "$0")/../../.."
env DISPLAY=:2 XQ_GROUP_STRESS=1 XQ_STRESS_COLS=6 XQ_STRESS_TICK_HZ=3000 XQ_STRESS_TICK_MS=3000 XQ_STRESS_SCROLL_MS=10000 "$@" $BIN 2>$S/$L.log >/dev/null &
P=$!
for i in $(seq 1 60); do sleep 1; grep -q '"phase":"done"' $S/$L.log && break; done
kill $P 2>/dev/null; wait $P 2>/dev/null
grep -o 'GROUPPERF|{"phase":"done".*' $S/$L.log
