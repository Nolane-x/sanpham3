pluginManagement {
    plugins {
        id("com.android.application") version "9.4.0"
        id("com.android.library") version "9.4.0"
    }

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

rootProject.name = "sanpham3-android-recovery-lab"
include(":app")
include(":android-host")
project(":android-host").projectDir =
    file("../../adapters/android-host")
