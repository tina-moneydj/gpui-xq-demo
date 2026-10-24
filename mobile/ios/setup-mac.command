#!/bin/bash
# XQ GPUI（iOS）一鍵準備：裝 Rust、先把 Rust 部分編好、設定 Bundle ID／開發團隊，最後打開 Xcode 專案。
# 可以重複執行（已經裝好的會跳過）。第一次大約 5～15 分鐘（看網路與 Mac 速度）。
set -uo pipefail
APP=XQGPUI
PREFIX=tw.moneydj.xqdemo.gpui
cd "$(dirname "$0")"
IOS_DIR="$(pwd)"
say() { printf '\n\033[1;34m▶ %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31m✗ %s\033[0m\n' "$*"; [ -n "${XQ_NO_PAUSE:-}" ] || read -r -p "按 Enter 關閉…" _; exit 1; }

say "1/5 檢查 Xcode"
if [ ! -d /Applications/Xcode.app ] && ! xcode-select -p 2>/dev/null | grep -q Xcode; then
  open "macappstore://apps.apple.com/app/xcode/id497799835" 2>/dev/null
  fail "還沒裝 Xcode。已幫你打開 App Store 的 Xcode 頁面：裝好、打開一次（同意授權、裝 iOS 元件）後，再執行這個檔案。"
fi
if ! xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; then
  fail "Xcode 還沒準備好：請打開 Xcode 一次，同意授權並安裝 iOS 平台（Xcode 會自己跳視窗），然後再執行這個檔案。"
fi
echo "Xcode：$(xcodebuild -version 2>/dev/null | head -1)"

say "2/5 安裝／更新 Rust"
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal || fail "Rust 安裝失敗（網路？）"
  export PATH="$HOME/.cargo/bin:$PATH"
fi
rustup target add aarch64-apple-ios aarch64-apple-ios-sim || fail "加入 iOS 編譯目標失敗"

say "3/5 設定 Bundle ID 與開發團隊（Local.xcconfig）"
# 開發團隊（Team ID）：先看 Xcode 專案裡選過的，再看 Xcode 已登入的帳號；都沒有就留空，等一下在 Xcode 裡選
TEAM=$(grep -o 'DEVELOPMENT_TEAM = [A-Z0-9]\{10\}' XQGPUI.xcodeproj/project.pbxproj 2>/dev/null | head -1 | awk '{print $3}')
[ -z "$TEAM" ] && TEAM=$(defaults read com.apple.dt.Xcode IDEProvisioningTeamByIdentifier 2>/dev/null | grep -o 'teamID = [A-Z0-9]*' | head -1 | awk '{print $3}')
if [ ! -f Local.xcconfig ]; then
  SUFFIX=$(uuidgen | tr -d '-' | cut -c1-6 | tr 'A-Z' 'a-z')
  { echo "// 這台 Mac 專用（不進 git）。Bundle ID 要全世界唯一，所以後面加了亂數。"; echo "XQ_BUNDLE_ID = $PREFIX.$SUFFIX"; echo "DEVELOPMENT_TEAM = $TEAM"; } > Local.xcconfig
elif [ -n "$TEAM" ] && grep -q '^DEVELOPMENT_TEAM = *$' Local.xcconfig; then
  sed -i '' "s/^DEVELOPMENT_TEAM = *$/DEVELOPMENT_TEAM = $TEAM/" Local.xcconfig
fi
cat Local.xcconfig

if [ ! -d XQGPUI.xcodeproj ]; then
  echo "還沒有 Xcode 專案檔，下載 XcodeGen 產生一個…"
  mkdir -p "$HOME/.xq-tools" && curl -fsSL -o /tmp/xcodegen.zip https://github.com/yonaskolb/XcodeGen/releases/latest/download/xcodegen.zip \
    && unzip -oq /tmp/xcodegen.zip -d "$HOME/.xq-tools" && "$HOME/.xq-tools/xcodegen/bin/xcodegen" generate || fail "產生 Xcode 專案失敗"
fi

say "4/5 先編 Rust（模擬器＋iPhone 各一次；第一次比較久）"
cd "$IOS_DIR/.."
for T in aarch64-apple-ios-sim aarch64-apple-ios; do
  echo "— $T"
  (unset SDKROOT; cargo rustc --lib --release --target "$T" --crate-type staticlib) || fail "Rust 編譯失敗（$T）。把這個視窗的內容截圖給開發的人。"
done

say "5/5 打開 Xcode 專案"
open "$IOS_DIR/$APP.xcodeproj"
cat <<MSG

完成！接下來在 Xcode 裡（照著說明文件的截圖）：
  1. 左邊點最上面的藍色專案圖示「$APP」→ 中間選 TARGETS 的「$APP」→ 上面「Signing & Capabilities」
  2. Team 選你的 Apple ID（Personal Team）
  3. 上方中間選你的 iPhone（或某台模擬器）→ 按左上角 ▶ 執行
MSG
[ -n "${XQ_NO_PAUSE:-}" ] || read -r -p "按 Enter 關閉這個視窗…" _
