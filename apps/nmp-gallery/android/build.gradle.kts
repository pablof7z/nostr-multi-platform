// Root build for the standalone NMP Gallery Android project. Pin the same
// AGP / Kotlin versions the Chirp project uses so the two projects stay
// build-compatible against a single local toolchain.
plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "1.9.24" apply false
}
