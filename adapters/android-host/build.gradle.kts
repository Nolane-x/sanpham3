plugins {
    id("com.android.library") version "9.4.0"
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


dependencies {
    testImplementation("junit:junit:4.13.2")
}
