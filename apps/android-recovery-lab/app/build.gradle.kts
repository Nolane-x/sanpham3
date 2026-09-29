plugins {
    id("com.android.application")
}

android {
    namespace = "dev.nolane.sanpham3.recoverylab"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.nolane.sanpham3.recoverylab"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":android-host"))
}
