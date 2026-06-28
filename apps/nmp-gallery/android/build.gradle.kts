// Root build for the standalone NMP Gallery Android project. Keep AGP / Kotlin
// pins local to the gallery so app repositories can choose their own toolchain.
plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "1.9.24" apply false
}
