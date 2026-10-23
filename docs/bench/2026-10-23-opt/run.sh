#!/bin/bash
# GPUI 優化前後重測（2026-10-23）：before＝7954c22（/workspace/zorder/gpui-base），after＝4ec0a12（/workspace/zorder/gpui-after，同一份程式碼的 release binary）。
# wry 不再比較。egui 只當參考（最後跑）。可重跑：有 marker 的段落跳過；每段跑完把原始檔複製到 docs/bench/2026-10-23-opt/ 並 commit＋push。
# 畫面：主視窗／高頻進價用 :63（1920×1200 Xvfb＋xfwm4＋picom，和之前 :2 桌面同樣的視窗管理員＋合成器）；
#       多視窗用 :62（3840×2160 Xvfb、不開視窗管理員，同之前 :42）。feed 埠 47761（WryFeedHost）／47781（xq-tickgen），不碰 47631。
B=/workspace/bench/opt-20261023
D=/workspace/zorder/gpui/docs/bench/2026-10-23-opt
M=$B/markers; mkdir -p $B; touch $M
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG XQ_PROBE XQ_APPLY XQ_SYNC_CHECK
export WRY_DIR=/workspace/zorder/wry EGUI_DIR=/workspace/zorder/egui
export FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll FEED_PORT=47761 TICK_ADDR=127.0.0.1:47781 MULTI_DISPLAY=:62
BEFORE=/workspace/zorder/gpui-base AFTER=/workspace/zorder/gpui-after
cd /workspace/zorder/gpui/scripts
save() {
  mkdir -p $D/$1 && cp -r $B/$1/. $D/$1/ && rm -rf $D/$1/xdg && cd /workspace/zorder/gpui && git add docs/bench/2026-10-23-opt &&
  git commit -q -m "bench 2026-10-23 優化前後：原始資料 $1" && git push -q origin main; echo "SAVED $1 $? $(date '+%T')"; cd /workspace/zorder/gpui/scripts
}
step() { # name cmd...
  local n=$1; shift
  grep -qx $n $M && return
  mkdir -p $B/$n; echo "RUN $n $(date '+%T') load=$(cut -d' ' -f1-3 /proc/loadavg)"
  if (cd /workspace/zorder/gpui/scripts && "$@" > $B/$n/stdout.log 2>&1); then echo $n >> $M; save $n; else echo "FAIL $n $?"; fi
}
echo "START $(date '+%F %T %z')"
step main-before  env DISPLAY=:63 OUT=$B/main-before  GPUI_DIR=$BEFORE APPS_LIST=gpui FEED_RESTART=1 ./bench-main-window.sh 3 all
step main-after   env DISPLAY=:63 OUT=$B/main-after   GPUI_DIR=$AFTER  APPS_LIST=gpui FEED_RESTART=1 ./bench-main-window.sh 3 all
step multi-before env OUT=$B/multi-before GPUI_DIR=$BEFORE ./bench-multi.sh 3 1,4 gpui
step multi-after  env OUT=$B/multi-after  GPUI_DIR=$AFTER  ./bench-multi.sh 3 1,4 gpui
step ticks-before env DISPLAY=:63 OUT=$B/ticks-before GPUI_DIR=$BEFORE ./bench-ticks.sh 3 s1k,s10k,s50k,s100k,burst gpui
step ticks-after  env DISPLAY=:63 OUT=$B/ticks-after  GPUI_DIR=$AFTER  ./bench-ticks.sh 3 s1k,s10k,s50k,s100k,burst gpui
# 拆解：同一個 after binary 切回舊量法／舊套用時機，看各自貢獻多少
step ticks-ab-probe env XQ_PROBE=nextframe DISPLAY=:63 OUT=$B/ticks-ab-probe GPUI_DIR=$AFTER ./bench-ticks.sh 3 s10k gpui
step ticks-ab-apply env XQ_APPLY=timer     DISPLAY=:63 OUT=$B/ticks-ab-apply GPUI_DIR=$AFTER ./bench-ticks.sh 3 s10k gpui
step multi-ab-probe env XQ_PROBE=nextframe OUT=$B/multi-ab-probe GPUI_DIR=$AFTER ./bench-multi.sh 3 4 gpui
# 報價同步＋遮蔽驗證（3 輪）
step sync bash -c "for i in 1 2 3; do D=:62 ADDR=$TICK_ADDR python3 ./bench-sync-occlude.py $AFTER $B/sync/sync-run\$i.json > /dev/null || exit 1; sleep 2; done"
# egui 參考
step egui-ref-main  env GPUI_DIR=$AFTER DISPLAY=:63 OUT=$B/egui-ref-main APPS_LIST=egui FEED_RESTART=1 ./bench-main-window.sh 3 all
step egui-ref-multi env GPUI_DIR=$AFTER OUT=$B/egui-ref-multi ./bench-multi.sh 3 1,4 egui
step egui-ref-ticks env GPUI_DIR=$AFTER DISPLAY=:63 OUT=$B/egui-ref-ticks ./bench-ticks.sh 3 s1k,s10k,s100k egui
echo "ALL_DONE $(date '+%T')"
