#!/usr/bin/env bash
# 網頁版建置：cargo（wasm32-unknown-unknown, release）→ wasm-bindgen（--target web）→ wasm-opt -O3 → dist/web/
# 需要：rustup target add wasm32-unknown-unknown；cargo install wasm-bindgen-cli --version <Cargo.lock 裡的 wasm-bindgen 版本>；
#       binaryen 的 wasm-opt（WASM_OPT=路徑；沒有就跳過最佳化）
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
cargo build --release --target wasm32-unknown-unknown
OUT=dist/web
rm -rf "$OUT" && mkdir -p "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" "$TARGET_DIR/wasm32-unknown-unknown/release/gpui-xq-demo.wasm"
WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
if [ -n "$WASM_OPT" ]; then
  "$WASM_OPT" -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$OUT/gpui-xq-demo_bg.wasm" -o "$OUT/gpui-xq-demo_bg.wasm"
fi
cp web/index.html "$OUT/"
ls -la "$OUT"
echo "gzip 後：$(gzip -9 -c "$OUT/gpui-xq-demo_bg.wasm" | wc -c) bytes（wasm）"
