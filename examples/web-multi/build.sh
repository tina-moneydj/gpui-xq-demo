#!/usr/bin/env bash
# 一頁嵌 4 個 GPUI 元件（可行性範例）：cargo（wasm32-unknown-unknown, release）→ wasm-bindgen（--target web）→ wasm-opt -O3 → dist/web-multi/
# 需要的工具與根目錄 web/build.sh 相同（wasm32 目標、wasm-bindgen-cli 0.2.129、可選的 wasm-opt：WASM_OPT=路徑）。
set -euo pipefail
cd "$(dirname "$0")"
ROOT=../..
cargo build --release --target wasm32-unknown-unknown
OUT=$ROOT/dist/web-multi
rm -rf "$OUT" && mkdir -p "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" target/wasm32-unknown-unknown/release/xq_web_multi_gpui.wasm
cp "$OUT/xq_web_multi_gpui_bg.wasm" "$OUT/xq_web_multi_gpui_bg.noopt.wasm"
WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
if [ -n "$WASM_OPT" ]; then
  "$WASM_OPT" -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$OUT/xq_web_multi_gpui_bg.wasm" -o "$OUT/xq_web_multi_gpui_bg.wasm"
fi
cp index.html page.js "$OUT/"
ls -la "$OUT"
for f in xq_web_multi_gpui_bg.noopt.wasm xq_web_multi_gpui_bg.wasm xq_web_multi_gpui.js index.html; do
  echo "${f}：原始 $(wc -c < "$OUT/$f") bytes，gzip -9 後 $(gzip -9 -c "$OUT/$f" | wc -c) bytes"
done
