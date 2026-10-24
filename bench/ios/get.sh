#!/bin/bash
# 一行裝好：在「終端機」貼上
#   curl -fsSL https://raw.githubusercontent.com/tina-moneydj/gpui-xq-demo/mobile-ios/bench/ios/get.sh | bash -s gpui
# （要 egui 版就把最後的 gpui 換成 egui）
# 做的事：把程式碼下載（或更新）到「家目錄/XQ-iOS」，再執行該 app 的 setup-mac.command（裝 Rust、編譯、打開 Xcode）。
set -uo pipefail
WHICH="${1:-gpui}"
case "$WHICH" in gpui) REPO=gpui-xq-demo; APP=XQGPUI ;; egui) REPO=egui-xq-demo; APP=XQEGUI ;; *) echo "用法：… | bash -s gpui 或 egui"; exit 1 ;; esac
BRANCH="${XQ_BRANCH:-mobile-ios}"
ROOT="$HOME/XQ-iOS"
DIR="$ROOT/$REPO"
if ! xcode-select -p >/dev/null 2>&1; then
  echo "還沒裝 Xcode（或它的命令列工具）。請先從 App Store 安裝 Xcode 並打開一次，再貼一次這行指令。"
  open "macappstore://apps.apple.com/app/xcode/id497799835" 2>/dev/null; exit 1
fi
mkdir -p "$ROOT"
if [ -d "$DIR/.git" ]; then
  echo "▶ 更新 $DIR"
  cd "$DIR"
  PBX="mobile/ios/$APP.xcodeproj/project.pbxproj"
  # 你在 Xcode 選的開發團隊先記到 Local.xcconfig，再把專案檔還原成最新版（不然 git 會說有衝突）
  TEAM=$(grep -o 'DEVELOPMENT_TEAM = [A-Z0-9]\{10\}' "$PBX" 2>/dev/null | head -1 | awk '{print $3}')
  LX="mobile/ios/Local.xcconfig"
  if [ -n "$TEAM" ] && [ -f "$LX" ] && grep -q '^DEVELOPMENT_TEAM = *$' "$LX"; then sed -i '' "s/^DEVELOPMENT_TEAM = *$/DEVELOPMENT_TEAM = $TEAM/" "$LX"; fi
  git checkout -- "mobile/ios/$APP.xcodeproj" 2>/dev/null || true
  git fetch -q origin "$BRANCH" && git checkout -q "$BRANCH" && git reset -q --hard "origin/$BRANCH" || { echo "更新失敗（網路？）"; exit 1; }
else
  echo "▶ 下載到 $DIR"
  git clone -q -b "$BRANCH" "https://github.com/tina-moneydj/$REPO.git" "$DIR" || { echo "下載失敗（網路？）"; exit 1; }
fi
echo "版本：$(git -C "$DIR" log -1 --format='%h %s' | cut -c1-80)"
exec bash "$DIR/mobile/ios/setup-mac.command" </dev/tty
