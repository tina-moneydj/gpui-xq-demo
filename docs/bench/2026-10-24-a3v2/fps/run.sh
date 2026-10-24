#!/bin/bash
# A3 v2 重圖表 FPS：同一個 gpui-opt2 執行檔，XQ_DRAW_QUADS=0（全 path）／1（A3 v2 分組）／v1（A3 v1）交錯 3 輪
B=${B:-$(dirname "$0")/raw}; mkdir -p $B
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG XQ_PROBE XQ_APPLY XQ_SYNC_CHECK XQ_TABLE_DIVS XQ_DRAW_QUADS XQ_SHAPE_GEN
export BENCH_SHARED_BOX=1 FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll FEED_PORT=47862
export WRY_DIR=/workspace/wry-xq-demo EGUI_DIR=/workspace/egui-xq-demo
cd /workspace/realhw/opt2/scripts
echo "START $(date '+%F %T %z')"
for r in 1 2 3; do
  case $r in 1) ORDER="0 1 v1";; 2) ORDER="1 v1 0";; 3) ORDER="v1 0 1";; esac
  for q in $ORDER; do
    o=$B/r$r-q$q; mkdir -p $o
    echo "RUN $o $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)"
    env DISPLAY=:72 OUT=$o GPUI_DIR=/workspace/realhw/opt2 APPS_LIST=gpui FEED_RESTART=1 XQ_DRAW_QUADS=$q ./bench-main-window.sh 1 heavy > $o/stdout.log 2>&1
    echo "END rc=$? $(date '+%T')"
  done
done
echo ALL_DONE
