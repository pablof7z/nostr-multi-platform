// Standalone NMP Gallery Android project. NOT a module of the Chirp
// multi-module project in `android/` — this is its own root build.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "NmpGalleryAndroid"
include(":app")
