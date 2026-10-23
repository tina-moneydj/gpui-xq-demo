// 只負責把 cargo-ndk 編好的 libxq_mobile_gpui.so 包成 APK（GpuiInputActivity：NativeActivity＋輸入法 InputConnection，取自 longbridge/gpui-mobile）
buildscript {
    repositories { google(); mavenCentral() }
    dependencies { classpath("com.android.tools.build:gradle:9.1.0") }
}
