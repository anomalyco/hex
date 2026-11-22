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
        ndk { abiFilters += "arm64-v8a" }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        externalNativeBuild.cmake {
            arguments += listOf("-DANDROID_STL=c++_static", "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON")
            targets += "hexspeech"
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
    }
    buildTypes {
        release {
            isDebuggable = false
            isJniDebuggable = false
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("upload")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    packaging.resources.excludes += "DebugProbesKt.bin"
    buildFeatures { compose = true }
    androidResources { noCompress += listOf("bin", "hextm", "valid", "capitals") }
    sourceSets["main"].assets.srcDir(modelAssets)
    // The same plain-text Android policy ships in Settings and in the Play listing drafts.
    sourceSets["main"].assets.srcDir(rootProject.file("play/policy"))
    // The swipe parity fixture's reader, for the JVM test on the host core and the instrumented one on bionic's.
    sourceSets["testDebug"].java.srcDir("src/swipeParity/java")
    sourceSets["androidTest"].java.srcDir("src/swipeParity/java")
    sourceSets["androidTest"].assets.srcDir(rootProject.file("../crates/keyboard/core/tests/fixtures"))
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin.compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
tasks.matching { it.name in setOf("packageRelease", "packageReleaseBundle", "signReleaseBundle", "bundleRelease", "assembleRelease") }
    .configureEach { dependsOn(requireUploadKey) }
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

// The keyboard's Rust core (crates/keyboard): libhex_keyboard.so for every variant's APK and a
// host build for the JVM unit tests.
// rustup's cargo, found as iOS's build-keyboard-core.sh finds it (crates/keyboard/tools/
// rustup-env.sh: on PATH, in CARGO_HOME or ~/.cargo, or Homebrew's rustup), run from the crate
// so its pinned toolchain (rust-toolchain.toml) is used; Cargo tracks the sources, so the tasks
// always run. Cargo subcommands such as cargo-ndk come from CARGO_HOME/bin or PATH.
val keyboardCrate = rootProject.file("../crates/keyboard")
val cargoHomeBin = (System.getenv("CARGO_HOME")?.takeIf { it.isNotEmpty() }?.let(::File)
    ?: File(System.getProperty("user.home"), ".cargo")).resolve("bin")
/** The folder of rustup and its cargo (`find_rustup`), or null when there is none. */
fun findRustup(): String? {
    val lookup = ProcessBuilder(
        "sh", "-c", ". \"\$1\" && find_rustup \"\$2\" && printf %s \"\$rustup_bin\"", "sh",
        keyboardCrate.resolve("tools/rustup-env.sh").path, keyboardCrate.path,
    ).redirectError(ProcessBuilder.Redirect.INHERIT).start()
    val folder = lookup.inputStream.bufferedReader().readText()
    return folder.takeIf { lookup.waitFor() == 0 && it.isNotEmpty() }
}
abstract class CargoBuild : Exec() {
    @get:OutputDirectory abstract val outputDirectory: DirectoryProperty
}
/**
 * `cargo <arguments(output directory)>` for hex-keyboard-ffi with JNI, the output resolved as it
 * runs; it fails first, saying what to install, without rustup's cargo or one of `tools` (cargo
 * subcommands).
 */
fun CargoBuild.cargo(vararg tools: String, arguments: (File) -> List<String>) {
    workingDir(keyboardCrate)
    outputs.upToDateWhen { false }
    doFirst {
        val rustup = checkNotNull(findRustup()) {
            "The keyboard core (crates/keyboard) needs rustup, whose cargo runs its pinned toolchain " +
                "(on PATH, in $cargoHomeBin or Homebrew's): install rustup (https://rustup.rs, or brew install rustup)"
        }
        val path = listOf(rustup, cargoHomeBin.path, System.getenv("PATH").orEmpty()).joinToString(File.pathSeparator)
        val missing = tools.filterNot { tool -> path.split(File.pathSeparator).any { File(it, tool).canExecute() } }
        check(missing.isEmpty()) {
            "The keyboard core (crates/keyboard) needs ${missing.joinToString(" and ")}: `cargo install cargo-ndk`"
        }
        environment("PATH", path)
        executable(File(rustup, "cargo"))
        args(arguments(outputDirectory.get().asFile))
        args("--manifest-path", keyboardCrate.resolve("Cargo.toml").path, "-p", "hex-keyboard-ffi", "--features", "jni")
    }
}
val buildHostKeyboardCore by tasks.registering(CargoBuild::class) {
    outputDirectory.set(layout.buildDirectory.dir("rustHost"))
    cargo {
        environment("CARGO_TARGET_DIR", it.path)
        listOf("build", "--release")
    }
}
// One per variant, its output directory the variant's (addGeneratedSourceDirectory); always optimised.
androidComponents.onVariants(androidComponents.selector().all()) { variant ->
    val build = tasks.register<CargoBuild>("build${variant.name.replaceFirstChar(Char::uppercase)}KeyboardCore") {
        environment("ANDROID_NDK_HOME", android.ndkDirectory.path)
        cargo("cargo-ndk") { listOf("ndk", "-t", "arm64-v8a", "-P", "31", "-o", it.path, "build", "--release") }
    }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(build, CargoBuild::outputDirectory)
}
tasks.withType<Test>().matching { it.name == "testDebugUnitTest" }.configureEach {
    dependsOn(buildHostKeyboardCore)
    // KeyboardCore loads libhex_keyboard from here; KeyboardCoreTest opens a descriptor.
    systemProperty("java.library.path", buildHostKeyboardCore.get().outputDirectory.get().dir("release").asFile.path)
    jvmArgs("--add-opens", "java.base/java.io=ALL-UNNAMED")
}

dependencies {
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.compose.material3:material3")
    // The keyboard's emoji panel (Apache 2.0): categories, recents and skin tones, all kept on the device.
    implementation("androidx.emoji2:emoji2-emojipicker:1.7.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
