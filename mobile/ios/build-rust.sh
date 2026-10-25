#!/bin/bash
# Xcode 每次建置前呼叫：用 cargo 編 Rust 靜態庫（永遠用 release 最佳化，不然手機上很慢）。
set -euo pipefail
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
cd "$(dirname "$0")/.."   # mobile/
if ! command -v cargo >/dev/null; then
  echo "error: 找不到 Rust（cargo）。請先雙擊 setup-mac.command 安裝。" >&2; exit 1
fi
if [ "${PLATFORM_NAME:-iphonesimulator}" = "iphonesimulator" ]; then T=aarch64-apple-ios-sim; else T=aarch64-apple-ios; fi
rustup target add "$T" >/dev/null 2>&1 || true
echo "Rust → $T (release)"
# Xcode 會帶一堆 SDK 相關環境變數，讓 cargo 自己挑 iOS SDK
unset SDKROOT LIBRARY_PATH
cargo rustc --lib --release --target "$T" --crate-type staticlib
