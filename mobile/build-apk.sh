#!/usr/bin/env bash
# 建 Android APK。用法：mobile/build-apk.sh [x86_64|arm64-v8a|"x86_64,arm64-v8a"]
# 需要：ANDROID_HOME、ANDROID_NDK_HOME、JAVA_HOME（JDK 17+）、cargo-ndk、rust android targets
set -euo pipefail
cd "$(dirname "$0")"
ABIS="${1:-x86_64}"
T=""; for a in ${ABIS//,/ }; do T="$T -t $a"; done
cargo ndk -P 26 $T -o android/app/src/main/jniLibs build --release
# cargo-ndk 會順手複製 gpui-mobile 的 rlib 動態副本，不需要打包
find android/app/src/main/jniLibs -name 'libgpui_mobile-*.so' -delete
(cd android && ./gradlew -q assembleRelease -Pabis="$ABIS")
ls -l android/app/build/outputs/apk/release/app-release.apk
