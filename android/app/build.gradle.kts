import java.net.URI
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.DigestInputStream
import java.security.MessageDigest
import java.util.Properties

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

// Resolve the key relative to its private properties file, never to a checked-in path.
val uploadProperties = providers.environmentVariable("HEX_ANDROID_KEYSTORE_PROPERTIES").orNull
    ?.let(::file) ?: File(System.getProperty("user.home"), ".config/hex-android/keystore.properties")
val upload = Properties().apply {
    if (uploadProperties.isFile) uploadProperties.inputStream().use(::load)
}
val requireUploadKey by tasks.registering {
    doLast {
        check(listOf("storeFile", "storePassword", "keyAlias", "keyPassword").all { !upload.getProperty(it).isNullOrBlank() } &&
            uploadProperties.parentFile.resolve(upload.getProperty("storeFile")).isFile) {
            "Release packaging requires a local upload keystore and keystore.properties."
        }
    }
}

// Hex Preview (the `preview` build type) is signed with its own key, never the Play upload key, read the same way.
val previewProperties = providers.environmentVariable("HEX_ANDROID_PREVIEW_KEYSTORE_PROPERTIES").orNull
    ?.let(::file) ?: File(System.getProperty("user.home"), ".config/hex-android-preview/keystore.properties")
val previewKey = Properties().apply {
    if (previewProperties.isFile) previewProperties.inputStream().use(::load)
}
val requirePreviewKey by tasks.registering {
    doLast {
        check(listOf("storeFile", "storePassword", "keyAlias", "keyPassword").all { !previewKey.getProperty(it).isNullOrBlank() } &&
            previewProperties.parentFile.resolve(previewKey.getProperty("storeFile")).isFile) {
            "Hex Preview packaging requires the preview keystore and its keystore.properties."
        }
    }
}
// A Hex Preview build's number: android/scripts/preview-release.sh passes the count of dev's commits up to the
// commit it builds, which only grows. A local build without it is build 1 and updates to any published one.
val previewBuild = providers.gradleProperty("hexPreviewBuild").orNull?.toInt() ?: 1
val previewChannel = providers.gradleProperty("hexPreviewChannel").orNull
    ?: "https://raw.githubusercontent.com/anomalyco/hex/android-preview/latest.json"
// Send feedback's Bellhop report key (gradle.properties, or -PhexFeedbackKey=…): publishable, it can only send feedback
// to Hex's inbox, and kept out of the source so it can be rotated. Empty: the build has no Send feedback.
val feedbackKey = providers.gradleProperty("hexFeedbackKey").orNull?.trim().orEmpty()
check(feedbackKey.isEmpty() || Regex("bh_pub_[0-9a-f]{64}").matches(feedbackKey)) { "hexFeedbackKey must be a Bellhop report key (bh_pub_…)" }

android {
    namespace = "ly.anoma.hex"
    compileSdk = 36
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "ly.anoma.hex"
        minSdk = 31
        targetSdk = 36
        versionCode = 5
        versionName = "0.2.2"
        buildConfigField("String", "FEEDBACK_KEY", "\"$feedbackKey\"")
        // Labs › Diagnostics: Hex Preview and debug builds (below), never the Play release.
        buildConfigField("boolean", "DIAGNOSTICS", "false")
        ndk { abiFilters += "arm64-v8a" }
        externalNativeBuild.cmake {
            arguments += listOf("-DANDROID_STL=c++_static", "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON")
            targets += listOf("hexspeech", "hexenglish")
        }
    }
    externalNativeBuild.cmake {
        path = file("src/main/cpp/CMakeLists.txt")
        version = "3.22.1"
    }
    signingConfigs {
        create("upload") {
            storeFile = upload.getProperty("storeFile")?.let { uploadProperties.parentFile.resolve(it) }
            storePassword = upload.getProperty("storePassword")
            keyAlias = upload.getProperty("keyAlias")
            keyPassword = upload.getProperty("keyPassword")
        }
        create("preview") {
            storeFile = previewKey.getProperty("storeFile")?.let { previewProperties.parentFile.resolve(it) }
            storePassword = previewKey.getProperty("storePassword")
            keyAlias = previewKey.getProperty("keyAlias")
            keyPassword = previewKey.getProperty("keyPassword")
        }
    }
    buildTypes {
        debug { buildConfigField("boolean", "DIAGNOSTICS", "true") }
        release {
            isDebuggable = false
            isJniDebuggable = false
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("upload")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
        // Hex Preview: the release build as an APK outside Google Play, beside the Play app, with its own updater
        // (src/preview). Play builds get src/play's empty one: Google Play forbids an app updating itself.
        create("preview") {
            initWith(getByName("release"))
            applicationIdSuffix = ".preview"
            signingConfig = signingConfigs.getByName("preview")
            matchingFallbacks += "release"
            resValue("string", "update_channel", previewChannel)
            buildConfigField("boolean", "DIAGNOSTICS", "true")
        }
    }
    sourceSets["debug"].java.srcDir("src/play/java")
    sourceSets["release"].java.srcDir("src/play/java")
    // Labs › Diagnostics' speed-test clip ships only where the page does.
    sourceSets["debug"].assets.srcDir("src/diagnostics/assets")
    sourceSets["preview"].assets.srcDir("src/diagnostics/assets")
    packaging.resources.excludes += "DebugProbesKt.bin"
    buildFeatures { compose = true; buildConfig = true }
    androidResources { noCompress += listOf("bin", "hextm", "valid", "capitals") }
    sourceSets["main"].assets.srcDir(modelAssets)
    // The same plain-text Android policy ships in Settings and in the Play listing drafts.
    sourceSets["main"].assets.srcDir(rootProject.file("play/policy"))
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

androidComponents {
    onVariants(selector().withBuildType("preview")) { variant ->
        val name = "${android.defaultConfig.versionName}-preview.$previewBuild"
        variant.outputs.forEach { it.versionCode.set(previewBuild); it.versionName.set(name) }
    }
}

kotlin.compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
tasks.matching { it.name in setOf("packageRelease", "packageReleaseBundle", "signReleaseBundle", "bundleRelease", "assembleRelease") }
    .configureEach { dependsOn(requireUploadKey) }
tasks.matching { it.name in setOf("packagePreview", "assemblePreview") }.configureEach { dependsOn(requirePreviewKey) }
// Unmodified InterVariable.ttf from rsms/inter v4.1, stored locally for offline builds.
val verifyInter by tasks.registering {
    val font = file("src/main/res/font/inter_variable.ttf")
    inputs.file(font)
    doLast {
        check(MessageDigest.getInstance("SHA-256").digest(font.readBytes()).joinToString("") { "%02x".format(it) }
            == "4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf") { "Inter v4.1 checksum mismatch" }
    }
}
// Pin the licensed HXTM files. An older prototype word-pair list cannot slip into an APK.
val verifyTypingModels by tasks.registering {
    val dir = file("src/main/assets/typing")
    val names = listOf("en.hextm", "es.hextm", "de.hextm", "nb.hextm", "fr.hextm", "it.hextm", "en.valid",
        "es.valid", "de.valid", "nb.valid", "fr.valid", "it.valid",
        "en.capitals", "es.capitals", "de.capitals", "fr.capitals", "it.capitals")
    val manifest = dir.resolve("SHA256SUMS")
    inputs.files(names.map(dir::resolve) + manifest)
    doLast {
        val entries = manifest.readLines().map { it.trim().split(Regex("\\s+")) }.associate { it.last() to it.first() }
        check(entries.keys == names.toSet())
        for (name in names) {
            val actual = MessageDigest.getInstance("SHA-256").digest(dir.resolve(name).readBytes()).joinToString("") { "%02x".format(it) }
            check(entries[name] == actual) { "Unverified typing model: $name" }
        }
    }
}
tasks.named("preBuild").configure { dependsOn(prepareSpeechModel, verifyInter, verifyTypingModels) }
// Every native library the APK ships, Hex's own and AndroidX's prebuilt ones, gets the check Hex's get when they are built
// (src/main/cpp/check-imports.cmake): no call that starts, replaces or traces a process or loads a shared object, and
// nothing exported but JNI entry points.
tasks.matching { it.name.matches(Regex("merge(Debug|Release|Preview)NativeLibs")) }.configureEach {
    val script = file("src/main/cpp/check-imports.cmake")
    val cmake = android.sdkDirectory.resolve("cmake/3.22.1/bin/cmake")
    val nm = android.ndkDirectory.resolve("toolchains/llvm/prebuilt").listFiles().orEmpty().map { it.resolve("bin/llvm-nm") }
    doLast {
        val tool = nm.first { it.canExecute() }
        val libraries = outputs.files.asFileTree.matching { include("**/*.so") }.files
        check(libraries.isNotEmpty()) { "No native libraries to check" }
        for (library in libraries) {
            val process = ProcessBuilder(cmake.path, "-DNM=${tool.path}", "-DLIB=${library.path}", "-P", script.path)
                .redirectErrorStream(true).start()
            val output = process.inputStream.bufferedReader().readText()
            check(process.waitFor() == 0) { output.trim() }
        }
    }
}

dependencies {
    implementation("androidx.activity:activity-compose:1.10.1")
    // The style bundle Android's password managers require to render inline suggestions in the strip.
    implementation("androidx.autofill:autofill:1.1.0")
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.compose.material3:material3")
    // The keyboard's emoji panel (Apache 2.0): categories, recents and skin tones, all kept on the device.
    implementation("androidx.emoji2:emoji2-emojipicker:1.7.0")
    testImplementation("junit:junit:4.13.2")
}
