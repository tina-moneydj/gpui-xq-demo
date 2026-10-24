#!/bin/bash
# gpui-opt2 交錯前後測：before＝main 6729f8d、after＝gpui-opt2；每輪 main（基準）／stress（5 萬列捲動）／multi（4 視窗）各跑 before、after（奇數輪 before 先、偶數輪 after 先），3 輪
B=/workspace/realhw/ilv2/raw; M=$B/markers; mkdir -p $B; touch $M
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG XQ_PROBE XQ_APPLY XQ_SYNC_CHECK XQ_TABLE_DIVS XQ_DRAW_QUADS
export BENCH_SHARED_BOX=1 FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll FEED_PORT=47861 TICK_ADDR=127.0.0.1:47871 MULTI_DISPLAY=:71
export WRY_DIR=/workspace/wry-xq-demo EGUI_DIR=/workspace/egui-xq-demo
declare -A DIR=([before]=/workspace/realhw/base [after]=/workspace/realhw/opt2)
S=/workspace/realhw/opt2/scripts
one() { one_ "$@" || { echo "RETRY r$1-$2-$3" | tee -a $B/loads.log; sleep 5; rm -rf $B/r$1-$2-$3; one_ "$@"; }; }
one_() {
  local r=$1 v=$2 k=$3 n="r$1-$2-$3"; grep -qx $n $M && return 0
  local o=$B/$n; mkdir -p $o; echo "RUN $n $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)" | tee -a $B/loads.log
  cd $S
  case $k in
    main)   env DISPLAY=:72 OUT=$o GPUI_DIR=${DIR[$v]} APPS_LIST=gpui FEED_RESTART=1 ./bench-main-window.sh 1 baseline > $o/stdout.log 2>&1 ;;
    stress) env DISPLAY=:72 OUT=$o GPUI_DIR=${DIR[$v]} APPS_LIST=gpui FEED_RESTART=1 ./bench-main-window.sh 1 stress > $o/stdout.log 2>&1 ;;
    multi)  env OUT=$o GPUI_DIR=${DIR[$v]} ./bench-multi.sh 1 4 gpui > $o/stdout.log 2>&1 ;;
  esac
  local rc=$?; echo "END $n rc=$rc $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)" | tee -a $B/loads.log
  [ $rc = 0 ] && echo $n >> $M
  return $rc
}
echo "START $(date '+%F %T %z')"
for r in 1 2 3; do
  if [ $((r % 2)) = 1 ]; then ORDER="before after"; else ORDER="after before"; fi
  for k in main stress multi; do for v in $ORDER; do one $r $v $k; done; done
done
echo "ALL_DONE $(date '+%T')"
