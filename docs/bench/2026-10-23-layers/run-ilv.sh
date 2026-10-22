#!/bin/bash
# 交錯重測：box 上另一個工作會間歇吃滿 CPU，所以每一輪內同一個程式 100 層／不分層緊接著跑（A/B 交錯），3 輪取中位數。
# 另外每輪跑一次 GPUI 改版前（360cb93）vs 改版後的 ticks／crosshair／pan（無 feed、合成資料），看有沒有退步。
B=/workspace/bench/layers-20261023
export DISPLAY=:5 FEED_PORT=47651 RUSTUP_TOOLCHAIN=stable
unset XQ_FEED_ADDR XQ_TAPE XQ_NO_VIEW_CACHE XQ_LAYER_MODE XQ_DEBUG
export WRY_DIR=/workspace/zorder/wry GPUI_DIR=/workspace/zorder/gpui EGUI_DIR=/workspace/zorder/egui
export FEED_DLL=/workspace/zorder/feedhost-bin/WryFeedHost.dll
cd /workspace/zorder/gpui/scripts
for r in 1 2 3; do
  for app in wry gpui egui; do
    for L in 100 1; do
      O=$B/ilv/r$r-$app-L$L; mkdir -p $O
      echo "r$r $app L$L $(date +%T) load=$(cut -d' ' -f1 /proc/loadavg)"
      LAYERS=$L OUT=$O APPS_LIST=$app FEED_RESTART=1 ./bench-main-window.sh 1 heavy > $O/stdout.log 2>&1 || echo "  FAIL $?"
    done
  done
  for v in base new; do
    d=/workspace/zorder/gpui; [ $v = base ] && d=/workspace/zorder/_gpui-base
    echo "ab r$r $v $(date +%T) load=$(cut -d' ' -f1 /proc/loadavg)"
    python3 /workspace/zorder/shared/ab.py gpui-$v $d ./target/release/gpui-xq-demo >> $B/ilv/ab.log 2>&1
  done
done
echo ILV_DONE $(date +%T)
