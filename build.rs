use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            // transcribe-cpp's static manifest records OpenBLAS as an absolute
            // link argument, which Cargo does not propagate through the -sys
            // crate to this final binary.
            println!("cargo:rustc-link-lib=dylib=openblas");
        }
        return;
    }
    let output = Command::new("xcode-select")
        .arg("-p")
        .output()
        .expect("xcode-select is required to link ScreenCaptureKit");
    assert!(output.status.success(), "xcode-select -p failed");
    let developer_dir = String::from_utf8(output.stdout).expect("Xcode path must be UTF-8");
    compile_indicator_shader();
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for runtime in ["swift-5.5/macosx", "swift/macosx"] {
        let swift_runtime = format!(
            "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/{runtime}",
            developer_dir.trim()
        );
        println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime}");
    }
}

fn compile_indicator_shader() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let source = manifest_dir.join("src/dictation_indicator.metal");
    let air = output_dir.join("dictation_indicator.air");
    let library = output_dir.join("dictation_indicator.metallib");
    println!("cargo:rerun-if-changed={}", source.display());

    let metal = Command::new("xcrun")
        .args(["--sdk", "macosx", "metal", "-c"])
        .arg(&source)
        .arg("-o")
        .arg(&air)
        .status()
        .expect("xcrun metal is required to build the dictation indicator");
    assert!(metal.success(), "Metal shader compilation failed");

    let metallib = Command::new("xcrun")
        .args(["--sdk", "macosx", "metallib"])
        .arg(&air)
        .arg("-o")
        .arg(&library)
        .status()
        .expect("xcrun metallib is required to build the dictation indicator");
    assert!(metallib.success(), "Metal library creation failed");
}
