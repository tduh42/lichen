plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The host build of the core (scripts/build-android-core.sh), loaded by the JVM unit tests.
val hostCore: String = rootDir.resolve("../../target/release").canonicalPath

// One version for the core and both apps: the repository's VERSION file, the series. CI passes
// the release itself, MAJOR.MINOR.N (scripts/version.sh), so the APK says which GitHub release
// it is.
val lichenVersion: String = providers.environmentVariable("LICHEN_VERSION_NAME").orNull
    ?: rootDir.resolve("../../VERSION").readText().trim()

// CI numbers each build (the commit count) so a newer APK installs as an update.
val lichenVersionCode: Int = providers.environmentVariable("LICHEN_VERSION_CODE").orNull?.toIntOrNull() ?: 1

// The lasting release key (CI: the published APKs). Every APK must carry the same key, or
// Android refuses it as an update and the user would have to uninstall, losing the identity.
// Without it, a local release build is signed with the debug key, to try it on the emulator.
val releaseKeystore: String? = providers.environmentVariable("LICHEN_KEYSTORE").orNull
val releaseKeystorePassword: String? = providers.environmentVariable("LICHEN_KEYSTORE_PASSWORD").orNull

android {
    namespace = "com.tduhamel.lichen"
    compileSdk = 37
    // The sticker set (ADR-0025), shared with iOS: assets `noto/manifest.json` and the files.
    sourceSets.getByName("main").assets.srcDir("../../../assets/stickers")
    // Settings › About › Licences (scripts/licences.py): asset `licences.json`.
    sourceSets.getByName("main").assets.srcDir("../../../assets/licences")
    // The NDK the core is built with (CLAUDE.md); AGP strips the library's symbols with it.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.tduhamel.lichen"
        // Android 12: AVIF decoding by the system (ADR-0012), the Bluetooth permissions.
        minSdk = 31
        targetSdk = 37
        versionCode = lichenVersionCode
        versionName = lichenVersion
        ndk { abiFilters += "arm64-v8a" }
    }

    signingConfigs {
        if (releaseKeystore != null && releaseKeystorePassword != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = releaseKeystorePassword
                keyAlias = "lichen"
                keyPassword = releaseKeystorePassword
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    lint {
        warningsAsErrors = false
        error += "MissingTranslation"
    }

    testOptions {
        unitTests.all { it.systemProperty("jna.library.path", hostCore) }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.biometric)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.jna.aar) { artifact { type = "aar" } }
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    implementation(libs.zxing.core)
    implementation(libs.maplibre)
    implementation(libs.lottie.compose)
    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.jna)
    testImplementation(libs.kotlinx.coroutines.test)
}
