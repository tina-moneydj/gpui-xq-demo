#!/bin/bash
# XQ 壓測 macOS 版：從原始碼編譯 GPUI、egui、行情產生器，放到 bench/mac/bin/。
#
# 需要：Xcode Command Line Tools（xcode-select --install）、Rust（rustup）。
#   GPUI 的 Metal shader：有完整 Xcode＋Metal 工具鏈就預先編譯（跟正式產品一樣）；
#   沒有就自動改用 runtime-shaders（程式啟動時才編 shader，啟動會慢一點），build-info.json 會記錄用了哪一種。
# egui-xq-demo（公開 repo）：依序找 $EGUI_SRC、和 gpui-xq-demo 同一層的 egui-xq-demo 資料夾；都沒有就用 gh 或 git clone 下載。
#
# 用法：./build-mac.sh            編譯本機架構（Apple Silicon＝arm64、Intel＝x86_64）
#       ./build-mac.sh --universal 另外編成 arm64＋x86_64 通用執行檔（lipo；比較慢，只有要拿去別台 Mac 跑才需要）
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
GPUI_SRC="$(cd "$HERE/../.." && pwd)"
BIN="$HERE/bin"
BRANCH="${XQ_BRANCH:-mac-bench}"
UNIVERSAL=0; [ "${1:-}" = "--universal" ] && UNIVERSAL=1
say() { printf '\033[1;36m[build]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[build] %s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "這支只在 macOS 上跑"
if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
  die "終端機目前在 Rosetta（Intel 轉譯）下執行。請關掉「使用 Rosetta 打開」後重開終端機。"
fi
xcode-select -p >/dev/null 2>&1 || die "還沒裝 Xcode Command Line Tools：請先執行  xcode-select --install  ，裝好再重跑"
if ! command -v cargo >/dev/null 2>&1; then
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
command -v cargo >/dev/null 2>&1 || die "還沒裝 Rust：請先執行
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
再開一個新的終端機視窗重跑"

# ---- egui 原始碼
EGUI_SRC="${EGUI_SRC:-}"
if [ -z "$EGUI_SRC" ]; then
  for d in "$GPUI_SRC/../egui-xq-demo" "$GPUI_SRC/../egui-xq-demo-$BRANCH" "$GPUI_SRC/../egui-xq-demo-main"; do
    [ -f "$d/Cargo.toml" ] && EGUI_SRC="$(cd "$d" && pwd)" && break
  done
fi
if [ -z "$EGUI_SRC" ]; then
  EGUI_SRC="$GPUI_SRC/../egui-xq-demo"
  say "找不到 egui-xq-demo，下載到 $EGUI_SRC"
  if command -v gh >/dev/null 2>&1; then
    gh repo clone tina-moneydj/egui-xq-demo "$EGUI_SRC" -- -b "$BRANCH" || die "gh 下載失敗：請檢查網路"
  else
    git clone -b "$BRANCH" https://github.com/tina-moneydj/egui-xq-demo.git "$EGUI_SRC" || die "git clone 失敗。
  可以改用瀏覽器：打開 https://github.com/tina-moneydj/egui-xq-demo → 切到 $BRANCH 分支 → Code → Download ZIP，
  解壓縮後把資料夾改名成 egui-xq-demo，放在 $(cd "$GPUI_SRC/.." && pwd)/ 底下，再重跑。"
  fi
fi
[ -f "$EGUI_SRC/Cargo.toml" ] || die "egui-xq-demo 原始碼不完整：$EGUI_SRC"
say "gpui 原始碼：$GPUI_SRC"
say "egui 原始碼：$EGUI_SRC"

# ---- Metal shader 預先編譯可不可以
SHADERS=runtime
tmp="$(mktemp -d)"
echo 'kernel void k(device float *a [[buffer(0)]]) { a[0] = 1.0; }' > "$tmp/t.metal"
if xcrun -sdk macosx metal -c "$tmp/t.metal" -o "$tmp/t.air" >/dev/null 2>&1; then
  SHADERS=precompiled
else
  say "這台沒有 Metal 編譯器（只有 Command Line Tools、或 Xcode 26 還沒下載 Metal Toolchain）→ GPUI 改用 runtime-shaders。"
  say "  想跟正式產品一樣預先編譯：裝 Xcode，然後  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer  、 xcodebuild -downloadComponent MetalToolchain"
fi
rm -rf "$tmp"
FEAT=(); [ "$SHADERS" = runtime ] && FEAT=(--features runtime-shaders)

ARCH="$(uname -m)"; [ "$ARCH" = arm64 ] && NATIVE=aarch64-apple-darwin || NATIVE=x86_64-apple-darwin
TARGETS=("$NATIVE")
if [ $UNIVERSAL = 1 ]; then
  TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
fi
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
mkdir -p "$BIN"
t0=$(date +%s)
for T in "${TARGETS[@]}"; do
  say "編譯 GPUI＋行情產生器（$T、shader=$SHADERS）…第一次約 5–15 分鐘"
  (cd "$GPUI_SRC" && cargo build --release --locked --target "$T" -p gpui-xq-demo -p xq-feed --bins ${FEAT[@]+"${FEAT[@]}"})
  say "編譯 egui（$T）…"
  (cd "$EGUI_SRC" && cargo build --release --locked --target "$T")
done
for b in gpui-xq-demo xq-tickgen xq-demofeed; do
  if [ $UNIVERSAL = 1 ]; then lipo -create -output "$BIN/$b" "$GPUI_SRC"/target/{aarch64,x86_64}-apple-darwin/release/$b
  else cp "$GPUI_SRC/target/$NATIVE/release/$b" "$BIN/$b"; fi
done
if [ $UNIVERSAL = 1 ]; then lipo -create -output "$BIN/egui-xq-demo" "$EGUI_SRC"/target/{aarch64,x86_64}-apple-darwin/release/egui-xq-demo
else cp "$EGUI_SRC/target/$NATIVE/release/egui-xq-demo" "$BIN/egui-xq-demo"; fi
# 本機編的不會有隔離標記；保險起見做 ad-hoc 簽章（Apple Silicon 執行檔一定要有簽章，lipo 合併後要重簽）
for b in "$BIN"/*; do [ -x "$b" ] && [ -f "$b" ] && codesign --force -s - "$b" >/dev/null 2>&1 || true; done

rev() { git -C "$1" rev-parse --short HEAD 2>/dev/null || echo "?"; }
branch() { git -C "$1" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "?"; }
cat > "$BIN/build-info.json" <<JSON
{"gpui_rev":"$(rev "$GPUI_SRC")","gpui_branch":"$(branch "$GPUI_SRC")","egui_rev":"$(rev "$EGUI_SRC")","egui_branch":"$(branch "$EGUI_SRC")",
 "shaders":"$SHADERS","targets":"${TARGETS[*]}","rustc":"$(rustc --version)","xcode":"$(xcodebuild -version 2>/dev/null | head -1 || echo CLT-only)",
 "sdk":"$(xcrun --show-sdk-version 2>/dev/null || echo ?)","built":"$(date '+%Y-%m-%dT%H:%M:%S%z')","seconds":$(( $(date +%s) - t0 ))}
JSON
say "完成（$(( $(date +%s) - t0 )) 秒）：$BIN"
ls -la "$BIN"
