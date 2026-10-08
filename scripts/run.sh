#!/usr/bin/env bash
# 在 box 的 Xvfb（DISPLAY=:2）跑 release 版。WryFeedHost 只收單一 client（後連者勝），
# 若 wry-xq-demo 也開著，兩邊會互搶連線；測試時先暫停另一個（kill -STOP <pid>，測完 kill -CONT）。
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
DISPLAY="${DISPLAY:-:2}" XQ_METRICS="${XQ_METRICS:-1}" exec ./target/release/gpui-xq-demo
