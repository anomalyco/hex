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
  # Every Git dependency Cargo.lock pins, with the revision its output hash was
  # computed from. scripts/check-nix-git-pins.py fails a pull request that bumps
  # one without the other, before Nix reports a hash mismatch.
  cargoGitDependencies = {
    "transcribe-rs-0.3.11" = {
      rev = "e37c66b6df43af51fde8dec34f98a07aaf09564c";
      hash = "sha256-MJJiug3LfbKsuHcL2+LJk95+j5kk/MWzb5ihdBlHLmE=";
    };
    "rune-0.0.1" = {
      rev = "1899e648a3a09bcca8a59b44deb9957302a73f04";
      hash = "sha256-6B+veHICwhZTWdwNwA48xSXZPfNHbh+DmRr03w6dPC0=";
    };
    "transcribe-cpp-sys-0.4.1" = {
      rev = "bcb639faa4c74a2300ba19e30360cb2355b1663a";
      hash = "sha256-1VztJ0VhPTjAuk4J0NT9KapDOqplegpHOqHsTpfHXWc=";
    };
  };
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
      # The Labs registry's published copy: a Rust test compares it to the registry.
      ../site/src/labs.json
      ../macos/HexMicrophoneDriver/Info.plist
      # Built in: a 2.3.1 install moves itself into this installer's layout.
      ../.github/release/install-linux.sh
    ];
  };

  # nixpkgs' vendor directory keeps every crate in one place, keyed by name and
  # version, so it cannot hold two copies from different sources. Rune's
  # `vendor/tiny-skia-path` is a path dependency inside Rune (a Git crate here)
  # at 0.11.4, the same name and version as resvg's from crates.io, so this
  # vendors crates.io's crates in `registry/` and every Git source's crates in
  # `git/`, and points each source at its own directory: Cargo tells them apart
  # by source. The script is importCargoLock's, rewritten; each fragment below
  # must still be in it (the guard says which is gone), so a nixpkgs change
  # fails loudly here instead of vendoring something wrong.
  cargoDeps =
    (rustPlatform.importCargoLock {
      lockFile = ../Cargo.lock;
      outputHashes = builtins.mapAttrs (_: dependency: dependency.hash) cargoGitDependencies;
    }).overrideAttrs
      (old: {
        buildCommand =
          let
            # importCargoLock's script, and what this override turns it into.
            fragments = [
              {
                what = "the single vendored-sources config";
                from = ''
                  [source.crates-io]
                  replace-with = "vendored-sources"

                  [source.vendored-sources]
                  directory = "cargo-vendor-dir"'';
                to = ''
                  [source.crates-io]
                  replace-with = "registry-vendor-dir"

                  [source.registry-vendor-dir]
                  directory = "cargo-vendor-dir/registry"

                  [source.git-vendor-dir]
                  directory = "cargo-vendor-dir/git"'';
              }
              {
                what = "the one-directory crate link";
                from = ''ln -s "$crate" $out/$(basename "$crate" | cut -c 34-)'';
                to = ''
                  dest=$(basename "$crate" | cut -c 34-)
                        if [ -e "$crate/.cargo-config" ]; then
                          mkdir -p $out/git
                          ln -s "$crate" $out/git/$dest
                        else
                          mkdir -p $out/registry
                          ln -s "$crate" $out/registry/$dest
                        fi'';
              }
              {
                what = "the Git source config append";
                from = ''cat "$crate/.cargo-config" >> $out/.cargo/config.toml'';
                to = ''sed 's/replace-with = "vendored-sources"/replace-with = "git-vendor-dir"/' "$crate/.cargo-config" >> $out/.cargo/config.toml'';
              }
            ];
            missing = builtins.filter (fragment: !(lib.hasInfix fragment.from old.buildCommand)) fragments;
          in
          if missing != [ ] then
            throw (
              "nix/package.nix: importCargoLock's vendor script changed; re-apply the split into "
              + "a registry and a Git vendor directory. Missing: "
              + lib.concatMapStringsSep ", " (fragment: fragment.what) missing
            )
          else
            lib.replaceStrings (map (fragment: fragment.from) fragments) (map (
              fragment: fragment.to
            ) fragments) old.buildCommand;
      });

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
