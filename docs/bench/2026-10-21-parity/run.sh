#!/bin/bash
# 三方對等版重測（2026-10-21 parity）。同 three-way-20261021 的 harness／設定；可重跑：有 marker 的段落跳過，ticks 用 RESUME=1 接續。
# 每段跑完把原始檔複製到 egui-xq-demo/docs/bench/2026-10-21-parity/ 並 commit＋push（box 重開也不會丟）。
B=/workspace/bench/three-way-20261021-parity
M=$B/markers; touch $M
export DISPLAY=:2
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG
cd /workspace/gpui-xq-demo/scripts
save() {
  local d=/workspace/egui-xq-demo/docs/bench/2026-10-21-parity
  mkdir -p $d && cp -r $B/$1 $d/ && cp $B/run.sh $d/ && cd /workspace/egui-xq-demo && git add docs/bench/2026-10-21-parity &&
  git -c user.name=tina-moneydj -c user.email=tina-moneydj@users.noreply.github.com commit -q -m "bench: 對等版三方重測原始數據 $1（2026-10-21）" && git push -q origin main; echo "SAVED $1 $?"; cd /workspace/gpui-xq-demo/scripts
}
echo "START $(date '+%F %T %z')"
if ! grep -qx main $M; then
  OUT=$B/main APPS_LIST=wry,gpui,egui FEED_RESTART=1 ./bench-main-window.sh 3 all > $B/main/stdout.log 2>&1 && echo main >> $M
  echo "MAIN_DONE $? $(date '+%T')"; grep -qx main $M && save main
fi
if ! grep -qx heavy $M; then
  OUT=$B/heavy APPS_LIST=wry,gpui,egui FEED_RESTART=1 ./bench-main-window.sh 3 heavy > $B/heavy/stdout.log 2>&1 && echo heavy >> $M
  echo "HEAVY_DONE $? $(date '+%T')"; grep -qx heavy $M && save heavy
fi
if ! grep -qx ticks $M; then
  RESUME=1 OUT=$B/ticks ./bench-ticks.sh 3 s1k,s10k,s50k,s100k,burst gpui,wry,egui >> $B/ticks/stdout.log 2>&1 && echo ticks >> $M
  echo "TICKS_DONE $? $(date '+%T')"; grep -qx ticks $M && save ticks
fi
echo "ALL_DONE $(date '+%T')"
