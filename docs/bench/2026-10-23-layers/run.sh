#!/bin/bash
# 圖層／上下順序重測（2026-10-23）：同 2026-10-21-parity 的 heavy harness，多了 drag／select／reorder／zshot 四段。
# 1) LAYERS=100：1000 條畫線分 100 層；2) LAYERS=1：同樣 1000 條不分層（對照：100 層多花多少）。各 3 輪取中位數。
# 同機有別的工作佔 :2 與 47631/47641，所以用 DISPLAY=:5、feed 埠 47651。
B=/workspace/bench/layers-20261023
export DISPLAY=:5 FEED_PORT=47651
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG
export WRY_DIR=/workspace/zorder/wry GPUI_DIR=/workspace/zorder/gpui EGUI_DIR=/workspace/zorder/egui
export FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll
export RUSTUP_TOOLCHAIN=stable
cd /workspace/zorder/gpui/scripts
echo "START $(date '+%F %T %z') load=$(cut -d' ' -f1-3 /proc/loadavg)"
LAYERS=100 OUT=$B/layers100 APPS_LIST=wry,gpui,egui FEED_RESTART=1 ./bench-main-window.sh 3 heavy > $B/layers100/stdout.log 2>&1
echo "L100_DONE $? $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)"
LAYERS=1 OUT=$B/flat APPS_LIST=wry,gpui,egui FEED_RESTART=1 ./bench-main-window.sh 3 heavy > $B/flat/stdout.log 2>&1
echo "FLAT_DONE $? $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)"
