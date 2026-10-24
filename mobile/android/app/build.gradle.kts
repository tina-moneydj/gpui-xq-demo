plugins { id("com.android.application") }

android {
    namespace = "tw.sysjust.xq.gpui"
    compileSdk = 35
    defaultConfig {
        applicationId = "tw.sysjust.xq.gpui"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        // ABI 由 -Pabis=x86_64,arm64-v8a 決定（預設兩個都包；量大小時各包一個）
        ndk { abiFilters += ((project.findProperty("abis") as String?) ?: "x86_64,arm64-v8a").split(",") }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            // 測試用：用 debug 金鑰簽，才能直接 adb install
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    sourceSets { getByName("main") { jniLibs.srcDirs("src/main/jniLibs") } }
    packaging { jniLibs { useLegacyPackaging = false } }
    lint { abortOnError = false; checkReleaseBuilds = false }
}
