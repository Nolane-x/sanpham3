plugins {
    id("com.android.application") version "9.4.0"
}

fun currentGitSha(): String =
    runCatching {
        val repoRoot = rootDir.resolve("../..").canonicalFile
        ProcessBuilder("git", "rev-parse", "HEAD")
            .directory(repoRoot)
            .redirectErrorStream(true)
            .start()
            .let { process ->
                val output = process.inputStream
                    .bufferedReader()
                    .readText()
                    .trim()
                check(process.waitFor() == 0)
                output
            }
    }.getOrDefault("unknown")

android {
    namespace = "dev.nolane.sanpham3.recoverylab"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.nolane.sanpham3.recoverylab"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1-lab"

        buildConfigField(
            "String",
            "GIT_SHA",
            "\"${currentGitSha()}\"",
        )
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":android-host"))
    testImplementation("junit:junit:4.13.2")
}