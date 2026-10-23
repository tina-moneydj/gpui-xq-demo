#!/bin/bash
# 第二版（正式數據）：交錯跑。第一版 run.sh 是 before 全跑完才跑 after，但 11:55 之後同機其他工作（Android 模擬器、Mac 版編譯）
# 把 load 拉到 9–10（8 核），before／after 遇到的干擾不一樣，1 視窗 FPS 同一個 binary 就在 9～30 之間跳，所以改成：
# 每一輪 before、after、egui 各跑一次（輪流換先後順序），3 輪取中位數；每次記 loadavg。每次跑完就 commit＋push 原始檔。
B=/workspace/bench/opt-20261023/ilv
D=/workspace/zorder/gpui/docs/bench/2026-10-23-opt/ilv
M=$B/markers; mkdir -p $B; touch $M
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG XQ_PROBE XQ_APPLY XQ_SYNC_CHECK
export WRY_DIR=/workspace/zorder/wry EGUI_DIR=/workspace/zorder/egui
export FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll FEED_PORT=47761 TICK_ADDR=127.0.0.1:47781 MULTI_DISPLAY=:62
declare -A DIR=([before]=/workspace/zorder/gpui-base [after]=/workspace/zorder/gpui-after [egui]=/workspace/zorder/gpui-after)
save() {
  mkdir -p $D && cp -r $B/. $D/ && find $D -name xdg -type d -prune -exec rm -rf {} + ; cd /workspace/zorder/gpui && git add docs/bench/2026-10-23-opt &&
  git commit -q -m "bench 2026-10-23 優化前後（交錯）：$1" && git push -q origin main; echo "SAVED $1 $? $(date '+%T')"
}
one() { # round ver kind
  local r=$1 v=$2 k=$3 n="r$1-$2-$3"; grep -qx $n $M && return
  local app=gpui; [ $v = egui ] && app=egui
  local o=$B/$n; mkdir -p $o; echo "RUN $n $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)" | tee -a $B/loads.log
  cd /workspace/zorder/gpui/scripts
  case $k in
    main)  env DISPLAY=:63 OUT=$o GPUI_DIR=${DIR[$v]} APPS_LIST=$app FEED_RESTART=1 ./bench-main-window.sh 1 all > $o/stdout.log 2>&1 ;;
    multi) env OUT=$o GPUI_DIR=${DIR[$v]} ./bench-multi.sh 1 1,4 $app > $o/stdout.log 2>&1 ;;
    ticks) if [ $v = egui ]; then S=s1k,s10k,s100k; else S=s1k,s10k,s50k,s100k,burst; fi
           env DISPLAY=:63 OUT=$o GPUI_DIR=${DIR[$v]} ./bench-ticks.sh 1 $S $app > $o/stdout.log 2>&1 ;;
  esac
  local rc=$?; echo "END $n rc=$rc $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)" | tee -a $B/loads.log
  [ $rc = 0 ] && echo $n >> $M
}
echo "START $(date '+%F %T %z')"
for r in 1 2 3; do
  if [ $((r % 2)) = 1 ]; then ORDER="before after egui"; else ORDER="after before egui"; fi
  for k in main multi ticks; do
    for v in $ORDER; do one $r $v $k; done
  done
  save "第 $r 輪"
done
echo "ALL_DONE $(date '+%T')"
