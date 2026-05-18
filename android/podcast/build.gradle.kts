import org.gradle.internal.os.OperatingSystem

// NmpPodcast Android shell (T157 step 1).
//
// Sibling to `:app` (NmpPulse) and `:gallery` — a minimal Kotlin/Compose host
// that links the SAME `libnmp_android_ffi.so` cdylib the :app module already
// builds. The kernel is the source of truth; this module owns only:
//   - Compose UI surfaces (LibraryScreen, MainActivity, theme)
//   - A JNI bridge mirror (PodcastKernelBridge) that hands the snapshot stream
//     up as a StateFlow
//   - Zero podcast business logic (D0/D5).
//
// The cargoNdk task here is intentionally a NO-OP — the `:app` module already
// produces `crates/nmp-android-ffi/target/...` into a jniLibs directory we
// reuse. Reusing one .so build keeps multi-module configuration simple and
// avoids double-compiles. See `android/build-podcast-apk.sh` for the
// end-to-end pipeline that ensures the .so exists before this assembles.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "com.podcast.app.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.podcast.app.android"
        minSdk = 26
        targetSdk = 34
        versionCode = 3
        versionName = "0.3.0-T-podcast-android-3"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
        debug {
            // T157 Pablo sideload: rename `app-debug.apk` to `podcast-debug.apk`
            // so the always-built artifact has a stable, descriptive name.
            applicationIdSuffix = ""
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.14" }

    // Reuse the SAME .so the :app module's cargoNdk task produces. cargo-ndk is
    // configured (via :app:cargoNdk) to write into `app/src/main/jniLibs`, so
    // we point this module's source set at that directory directly. The build
    // script (`android/build-podcast-apk.sh`) invokes :app:cargoNdk before
    // :podcast:assembleDebug to guarantee freshness.
    sourceSets["main"].jniLibs.srcDirs(
        "${rootProject.projectDir}/app/src/main/jniLibs",
    )

    // Rename `app-debug.apk` → `podcast-debug.apk` at the variant level so the
    // artifact path is predictable. Single-version constraint (see T157 step 4):
    // older APKs are removed by the shell wrapper before each build.
    applicationVariants.all {
        outputs.all {
            val variantOutput =
                this as com.android.build.gradle.internal.api.BaseVariantOutputImpl
            variantOutput.outputFileName = "podcast-debug.apk"
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")
}

// Delegate native build to :app's cargoNdk task — the .so is shared between
// modules. This dependsOn ensures `:podcast:assembleDebug` always sees a
// fresh build of `libnmp_android_ffi.so` regardless of which module Pablo
// runs first.
tasks.named("preBuild") {
    dependsOn(":app:cargoNdk")
}
