plugins {
    id("com.android.library") version "9.4.0"
    id("org.jetbrains.kotlin.android") version "2.4.10"
}

android {
    namespace = "dev.nolane.sanpham3.androidhost"
    compileSdk = 37

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
