#!/bin/bash
# 網頁版注音測試：DISPLAY=:9 上起 dbus＋fcitx5（新酷音），Chrome 開網頁版（搜尋框預設開），xdotool 打注音，整個畫面截圖（含候選窗）
# 用法：ime-web.sh egui|gpui <outdir>
set -u
APP=$1; OUT=$2; mkdir -p $OUT
export DISPLAY=:9
PORT=$([ $APP = egui ] && echo 47772 || echo 47773)
shot() { ffmpeg -loglevel error -y -f x11grab -video_size 1300x800 -i :9+0,0 -frames:v 1 $OUT/$APP-$1.png; }
B=/workspace/web/gpui-xq-demo/target/release/xq-ws-bridge
$B --listen 127.0.0.1:$PORT --feed 127.0.0.1:47781 --www /workspace/web/$APP-xq-demo/dist/web > $OUT/$APP-bridge.log 2>&1 & BP=$!
GTK_IM_MODULE=fcitx XMODIFIERS=@im=fcitx /usr/bin/google-chrome --no-first-run --no-default-browser-check --user-data-dir=/tmp/xq-ime-$APP \
  --window-size=1200,720 --window-position=0,0 --ignore-gpu-blocklist --disable-features=Translate --password-store=basic --app="http://127.0.0.1:$PORT/index.html?XQ_PERIOD=D" > $OUT/$APP-chrome.log 2>&1 & CP=$!
echo "bridge=$BP chrome=$CP"; echo "$BP $CP" > $OUT/$APP.pids
