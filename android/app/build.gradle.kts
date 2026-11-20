import java.net.URI
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.DigestInputStream
import java.security.MessageDigest

plugins {
    id("com.android.application")
    kotlin("android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Resolved at build time only. A clean install works with airplane mode on.
val modelAssets = layout.buildDirectory.dir("generated/whisper/assets")
val modelRevision = "5359861c739e955e79d9a303bcbc70fb988958b1"
val modelSha256 = "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7"
val prepareSpeechModel by tasks.registering {
    val modelUrl = "https://huggingface.co/ggerganov/whisper.cpp/resolve/$modelRevision/ggml-tiny-q5_1.bin"
    inputs.property("url", modelUrl)
    inputs.property("sha256", modelSha256)
    val destination = modelAssets.map { it.file("whisper-tiny.bin") }
    outputs.file(destination)
    doLast {
        val file = destination.get().asFile
        file.parentFile.mkdirs()
        val part = Files.createTempFile(file.parentFile.toPath(), "whisper", ".part")
        try {
            val digest = MessageDigest.getInstance("SHA-256")
            URI(modelUrl).toURL().openConnection().apply {
                connectTimeout = 30_000
                readTimeout = 120_000
            }.getInputStream().use { stream ->
                DigestInputStream(stream, digest).use { input ->
                    Files.newOutputStream(part).use { output -> input.copyTo(output) }
                }
            }
            check(digest.digest().joinToString("") { "%02x".format(it) } == modelSha256) {
                "Whisper model checksum mismatch"
            }
            Files.move(part, file.toPath(), StandardCopyOption.REPLACE_EXISTING)
        } finally {
            Files.deleteIfExists(part)
        }
    }
}

android {
    namespace = "ly.anoma.hex"
    compileSdk = 35
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "ly.anoma.hex"
        minSdk = 31
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
        externalNativeBuild.cmake {
            arguments += listOf("-DANDROID_STL=c++_static", "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON")
            targets += "hexspeech"
        }
    }
    externalNativeBuild.cmake {
        path = file("src/main/cpp/CMakeLists.txt")
        version = "3.22.1"
    }
    buildFeatures { compose = true }
    androidResources { noCompress += "bin" }
    sourceSets["main"].assets.srcDir(modelAssets)
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin.compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
tasks.named("preBuild").configure { dependsOn(prepareSpeechModel) }

dependencies {
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.compose.material3:material3")
    testImplementation("junit:junit:4.13.2")
}
