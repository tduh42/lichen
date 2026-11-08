plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The host build of the core (scripts/build-android-core.sh), loaded by the JVM unit tests.
val hostCore: String = rootDir.resolve("../../target/release").canonicalPath

android {
    namespace = "com.tduhamel.lichen"
    compileSdk = 37
    // The NDK the core is built with (CLAUDE.md); AGP strips the library's symbols with it.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.tduhamel.lichen"
        // Android 12: AVIF decoding by the system (ADR-0012), the Bluetooth permissions.
        minSdk = 31
        targetSdk = 37
        versionCode = 1
        versionName = "0.9"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
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
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.jna.aar) { artifact { type = "aar" } }
    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.jna)
    testImplementation(libs.kotlinx.coroutines.test)
}
