use std::path::PathBuf;
use std::process::Command;

fn main() {
    build_version();
    test_build_commit();
    // `hex_capture`: the `capture` experiment's code, in every macOS and Linux
    // build (`scrawl-capture` is their dependency); Windows has no host yet.
    println!("cargo::rustc-check-cfg=cfg(hex_capture)");
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("macos" | "linux")
    ) {
        println!("cargo::rustc-cfg=hex_capture");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            // transcribe-cpp's static manifest records OpenBLAS as an absolute
            // link argument, which Cargo does not propagate through the -sys
            // crate to this final binary. Keep it after the static archives so
            // GNU ld's --as-needed does not discard it before seeing cblas_*.
            println!("cargo:rustc-link-arg=-lopenblas");
        }
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            // transcribe-cpp's Vulkan backend links the vulkan-1 import
            // library by name without its search path. The DLL itself ships
            // with the GPU driver.
            println!("cargo:rerun-if-env-changed=VULKAN_SDK");
            if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
                && let Some(sdk) = std::env::var_os("VULKAN_SDK")
            {
                println!(
                    "cargo:rustc-link-search=native={}",
                    PathBuf::from(sdk).join("Lib").display()
                );
            }
            #[cfg(windows)]
            embed_windows_resources();
        }
        return;
    }
    let output = Command::new("xcode-select")
        .arg("-p")
        .output()
        .expect("xcode-select is required to link ScreenCaptureKit");
    assert!(output.status.success(), "xcode-select -p failed");
    let developer_dir = String::from_utf8(output.stdout).expect("Xcode path must be UTF-8");
    compile_swift_bridge("apple_speech", "HEXAppleSpeechBridge", &[]);
    println!("cargo:rustc-link-lib=framework=AVFAudio");
    println!("cargo:rustc-link-lib=framework=Speech");
    // FoundationModels exists only on macOS 26 and later. Weak-link it, and
    // drop the object's strong autolink entry, so Hex still launches on older
    // systems and reports Apple Intelligence as unavailable.
    compile_swift_bridge(
        "apple_intelligence",
        "HEXAppleIntelligenceBridge",
        &["FoundationModels"],
    );
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,FoundationModels");
    compile_swift_bridge("calendar", "HEXCalendarBridge", &[]);
    println!("cargo:rustc-link-lib=framework=EventKit");
    compile_permission_guide();
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for runtime in ["swift-5.5/macosx", "swift/macosx"] {
        let swift_runtime = format!(
            "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/{runtime}",
            developer_dir.trim()
        );
        println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime}");
    }
}

/// The version Hex reports (`HEX_BUILD_VERSION`): the crate version, which
/// the Release workflow stamps (scripts/set-crate-version.sh), else, for any
/// other build of the unstamped `0.0.0`, `0.0.0-dev-local-<commit>` with
/// `-dirty` for uncommitted changes (`0.0.0-dev-local` without git, as in Nix).
/// It counts as a dev build, so it follows the dev channel's current release.
fn build_version() {
    println!("cargo:rerun-if-env-changed=HEX_VERSION");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    // CI builds name their version in HEX_VERSION and check `hex --version`
    // against it, test builds included (their crate version stays 0.0.0).
    if let Some(named) = std::env::var("HEX_VERSION")
        .ok()
        .filter(|named| !named.is_empty())
    {
        println!("cargo:rustc-env=HEX_BUILD_VERSION={named}");
        return;
    }
    if version != "0.0.0" {
        println!("cargo:rustc-env=HEX_BUILD_VERSION={version}");
        return;
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let mut version = "0.0.0-dev-local".to_owned();
    if let Some(commit) =
        git(&["rev-parse", "--short=7", "HEAD"]).filter(|commit| !commit.is_empty())
    {
        version = format!("{version}-{commit}");
        if git(&["status", "--porcelain", "--untracked-files=no"])
            .is_some_and(|status| !status.is_empty())
        {
            version.push_str("-dirty");
        }
        let reference = git(&["symbolic-ref", "-q", "HEAD"]);
        for name in ["HEAD", "packed-refs", "index"]
            .into_iter()
            .chain(reference.as_deref())
        {
            if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", name])
                .filter(|path| std::path::Path::new(path).exists())
            {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    println!("cargo:rustc-env=HEX_BUILD_VERSION={version}");
}

/// A test or local build carries its commit (`HEX_COMMIT`), which the sidebar shows
/// beside the version.
fn test_build_commit() {
    println!("cargo:rerun-if-env-changed=HEX_TEST_BUILD");
    println!("cargo:rerun-if-env-changed=HEX_LOCAL_BUILD");
    if std::env::var("HEX_TEST_BUILD").as_deref() != Ok("1")
        && std::env::var("HEX_LOCAL_BUILD").as_deref() != Ok("1")
    {
        return;
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let Some(commit) = git(&["rev-parse", "--short=7", "HEAD"]) else {
        return;
    };
    println!("cargo:rustc-env=HEX_COMMIT={commit}");
    let reference = git(&["symbolic-ref", "-q", "HEAD"]);
    for name in ["HEAD", "packed-refs"]
        .into_iter()
        .chain(reference.as_deref())
    {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", name])
            .filter(|path| std::path::Path::new(path).exists())
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// Gives the executable its icon and the "Hex" name Explorer and Task
/// Manager show, whatever the file is called.
#[cfg(windows)]
fn embed_windows_resources() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let icon = manifest_dir.join("resources/windows/hex.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = version
        .split(['.', '-'])
        .take(3)
        .map(|part| part.parse::<u16>().unwrap_or(0).to_string())
        .chain(std::iter::repeat("0".to_string()))
        .take(4)
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        r#"#include <winver.h>
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Hex"
      VALUE "ProductName", "Hex"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "hex.exe"
      VALUE "InternalName", "hex"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let resource = output_dir.join("hex.rc");
    std::fs::write(&resource, script).expect("write the Windows resource script");
    embed_resource::compile(&resource, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the Windows resources");
}

fn compile_permission_guide() {
    println!("cargo:rerun-if-changed=native/permission_guide.m");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=ApplicationServices");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=CoreImage");
    println!("cargo:rustc-link-lib=framework=IOKit");
    println!("cargo:rustc-link-lib=framework=QuartzCore");

    cc::Build::new()
        .file("native/permission_guide.m")
        .flag("-fobjc-arc")
        .compile("hex_permission_guide");
}

fn compile_swift_bridge(name: &str, module: &str, without_autolink: &[&str]) {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let source = manifest_dir.join(format!("src/{name}.swift"));
    let object = output_dir.join(format!("{name}.o"));
    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun is required to compile Swift bridges");
    assert!(sdk.status.success(), "could not locate the macOS SDK");
    let target = match std::env::var("TARGET").as_deref() {
        Ok("aarch64-apple-darwin") => "arm64-apple-macosx15.0",
        Ok("x86_64-apple-darwin") => "x86_64-apple-macosx15.0",
        Ok(target) => panic!("unsupported macOS target for {module}: {target}"),
        Err(error) => panic!("TARGET is unavailable: {error}"),
    };
    println!("cargo:rerun-if-changed={}", source.display());
    let mut swift = Command::new("xcrun");
    swift.args([
        "swiftc",
        "-parse-as-library",
        "-emit-object",
        "-module-name",
        module,
        "-sdk",
    ]);
    swift
        .arg(String::from_utf8_lossy(&sdk.stdout).trim())
        .args(["-target", target]);
    for framework in without_autolink {
        swift.args([
            "-Xfrontend",
            "-disable-autolink-framework",
            "-Xfrontend",
            framework,
        ]);
    }
    let status = swift
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("xcrun swiftc is required to compile Swift bridges");
    assert!(status.success(), "{module} compilation failed");
    println!("cargo:rustc-link-arg={}", object.display());
}
