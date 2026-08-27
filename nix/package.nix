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
  gtk3,
  gtk-layer-shell,
  libayatana-appindicator,
  alsa-lib,
  alsa-plugins,
  pipewire,
  openssl,
  libxkbcommon,
  wayland,
  fontconfig,
  freetype,
  curl,
  wl-clipboard,
  wtype,
  wrapGAppsHook3,
  autoAddDriverRunpath,
  makeWrapper,
  symlinkJoin,
  libx11,
  libxcb,
  libxcursor,
  libxrandr,
  libxi,
}:

let
  alsaPluginDir = symlinkJoin {
    name = "hex-alsa-plugins";
    paths = [
      "${pipewire}/lib/alsa-lib"
      "${alsa-plugins}/lib/alsa-lib"
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "hex";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
      ../src
      ../native
      ../resources
      ../assets
      ../packaging
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "transcribe-rs-0.3.11" = "sha256-MJJiug3LfbKsuHcL2+LJk95+j5kk/MWzb5ihdBlHLmE=";
    };
  };

  nativeBuildInputs = [
    pkg-config
    cmake
    python3
    shaderc
    wrapGAppsHook3
    autoAddDriverRunpath
    makeWrapper
  ];

  buildInputs = [
    vulkan-headers
    vulkan-loader
    spirv-headers
    openblas
    gtk3
    gtk-layer-shell
    libayatana-appindicator
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

  dontUseCmakeConfigure = true;
  doCheck = false;

  env = {
    OPENSSL_NO_VENDOR = "1";
    BLA_VENDOR = "OpenBLAS";
    # build.rs emits -lopenblas before transcribe-cpp-sys, so GNU ld drops
    # cblas_sgemm. Append the library after the static archives.
    RUSTFLAGS = "-C link-arg=-lopenblas";
  };

  postInstall = ''
    mv $out/bin/voice-control $out/bin/hex
    install -Dm644 ${../packaging/hex.desktop} $out/share/applications/hex.desktop
    substituteInPlace $out/share/applications/hex.desktop \
      --replace-fail '@HEX_BIN@' "$out/bin/hex"
  '';

  preFixup = ''
    gappsWrapperArgs+=(
      --prefix PATH : ${
        lib.makeBinPath [
          curl
          wl-clipboard
          wtype
        ]
      }
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          vulkan-loader
          libayatana-appindicator
          gtk-layer-shell
          openblas
          alsa-lib
        ]
      }
      --set ALSA_PLUGIN_DIR ${alsaPluginDir}
    )
  '';

  meta = {
    description = "Local-first voice dictation (Linux beta)";
    longDescription = ''
      HEX transcribes speech locally and pastes at the current focus.
      This package is not the user-local managed layout, so HEX does not
      apply its signed in-app updater.
    '';
    homepage = "https://github.com/anomalyco/hex";
    license = lib.licenses.mit;
    mainProgram = "hex";
    platforms = [ "x86_64-linux" ];
  };
}
