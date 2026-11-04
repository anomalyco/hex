{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  python3,
  shaderc,
  vulkan-headers,
  vulkan-loader,
  spirv-headers,
  openblas,
  alsa-lib,
  alsa-plugins,
  pipewire,
  libva,
  ffmpeg-headless,
  openssl,
  libxkbcommon,
  wayland,
  fontconfig,
  freetype,
  curl,
  util-linux,
  wl-clipboard,
  wtype,
  makeWrapper,
  autoAddDriverRunpath,
  symlinkJoin,
  libx11,
  libxcb,
  libxcursor,
  libxrandr,
  libxi,
  mkShell,
  rust-analyzer,
  rustfmt,
  clippy,
}:

let
  buildEnv = {
    OPENSSL_NO_VENDOR = "1";
    BLA_VENDOR = "OpenBLAS";
  };
  runtimeEnv = {
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      vulkan-loader
      openblas
      alsa-lib
      wayland
      libxkbcommon
      fontconfig
      # Linux Annotate dlopens libpipewire-0.3, libva and libva-drm. libva finds VA-API
      # drivers through its built-in /run/opengl-driver path or LIBVA_DRIVERS_PATH.
      pipewire
      libva
    ];
    ALSA_PLUGIN_DIR = symlinkJoin {
      name = "hex-alsa-plugins";
      paths = [
        "${pipewire}/lib/alsa-lib"
        "${alsa-plugins}/lib/alsa-lib"
      ];
    };
  };
  runtimePackages = [
    curl
    wl-clipboard
    wtype
    # Annotate runs ffmpeg and ffprobe; the headless build includes libx264.
    ffmpeg-headless
  ];
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "hex";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../.cargo
      ../build.rs
      ../crates
      ../src
      ../tests
      ../native
      ../resources
      ../assets
      ../packaging
      ../macos/HexMicrophoneDriver/Info.plist
      # Built in: a 2.3.1 install moves itself into this installer's layout.
      ../.github/release/install-linux.sh
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "transcribe-rs-0.3.11" = "sha256-MJJiug3LfbKsuHcL2+LJk95+j5kk/MWzb5ihdBlHLmE=";
      "rune-0.0.1" = "sha256-mUrn+AJ4M2dHGZseybw/0mAO+mgIVNSMXMwyj/51En0=";
      "transcribe-cpp-sys-0.4.1" = "sha256-BAc2qKUMBZ5D6+Bq12NaZ1+ntWxHv95TYcAMTt3a5X8=";
    };
  };

  nativeBuildInputs = [
    pkg-config
    cmake
    python3
    shaderc
    makeWrapper
    autoAddDriverRunpath
  ];
  buildInputs = [
    vulkan-headers
    vulkan-loader
    spirv-headers
    openblas
    alsa-lib
    openssl
    libxkbcommon
    wayland
    fontconfig
    freetype
    libx11
    libxcb
    libxcursor
    libxrandr
    libxi
  ];

  # OpenCode HTTP requests run through curl, including in the tests; the
  # 2.3.1 adoption test runs the Linux installer, which needs flock.
  nativeCheckInputs = [
    curl
    util-linux
  ];

  dontUseCmakeConfigure = true;
  env = buildEnv;
  # Desktop and installed-model tests are #[ignore]; run the remaining suite.
  doCheck = true;
  preCheck = ''
    export HOME="$TMPDIR/hex-test-home"
    mkdir -p "$HOME"
  '';

  postInstall = ''
    install -Dm644 ${../packaging/anomaly.hex.desktop} $out/share/applications/anomaly.hex.desktop
    mkdir -p $out/share/icons
    cp -R ${../packaging/linux/icons/hicolor} $out/share/icons/hicolor
    substituteInPlace $out/share/applications/anomaly.hex.desktop \
      --replace-fail '@HEX_BIN@' "$out/bin/hex"
  '';

  postFixup = ''
    wrapProgram $out/bin/hex \
      --prefix PATH : ${lib.makeBinPath runtimePackages} \
      --prefix LD_LIBRARY_PATH : ${runtimeEnv.LD_LIBRARY_PATH} \
      --set ALSA_PLUGIN_DIR ${runtimeEnv.ALSA_PLUGIN_DIR}
  '';

  passthru.devShell = mkShell {
    inputsFrom = [ finalAttrs.finalPackage ];
    env = buildEnv // runtimeEnv;
    packages = runtimePackages ++ [
      rust-analyzer
      rustfmt
      clippy
    ];
  };

  meta = {
    description = "Local-first voice dictation (Linux beta)";
    homepage = "https://anoma.ly/hex/";
    license = lib.licenses.mit;
    mainProgram = "hex";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
})
