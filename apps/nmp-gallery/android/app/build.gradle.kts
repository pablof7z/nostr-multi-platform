// App module for the standalone NMP Gallery. Links against the prebuilt
// `libnmp_app_gallery.so` placed in `src/main/jniLibs/<abi>/` by
// `cargo ndk build --target arm64-v8a -p nmp-app-gallery`. There is NO
// custom WebSocket/HTTP code in this app — all relay traffic is owned by
// the NMP kernel via JNI.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "org.nmp.gallery"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.nmp.gallery"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.14" }

    // Pre-built .so files live in `src/main/jniLibs/<abi>/`. Produced by:
    //   cargo ndk -t arm64-v8a -o apps/nmp-gallery/android/app/src/main/jniLibs \
    //       build --release -p nmp-app-gallery
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    // Kotlin sources live in `src/main/kotlin` rather than `src/main/java`.
    sourceSets["main"].java.srcDirs("src/main/kotlin")
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.2")
    implementation("androidx.navigation:navigation-compose:2.7.7")
    implementation("com.google.flatbuffers:flatbuffers-java:25.2.10")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    implementation("io.coil-kt:coil-compose:2.6.0")
}
