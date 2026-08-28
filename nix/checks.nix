{ self, pkgs }:
let
  inherit (pkgs) lib;
  hex = self.packages.${pkgs.stdenv.hostPlatform.system}.hex;
  shell = self.devShells.${pkgs.stdenv.hostPlatform.system}.default;
  home =
    settings:
    (self.inputs.home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules = [
        self.homeManagerModules.hex
        {
          home.username = "hex-test";
          home.homeDirectory = "/home/hex-test";
          home.stateVersion = "26.05";
          programs.hex = settings;
        }
      ];
    }).config;
  enabled = home {
    enable = true;
    autostart = true;
  };
  manual = home { enable = true; };
  disabled = home { enable = false; };
  custom = home {
    enable = true;
    autostart = true;
    package = pkgs.hello;
    systemdTarget = "sway-session.target";
  };
  system =
    (self.inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        self.nixosModules.hex
        {
          programs.hex.enable = true;
          system.stateVersion = "26.05";
        }
      ];
    }).config;
in
{
  package = pkgs.runCommand "hex-package-check" { } ''
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
    ${lib.getExe hex} --version
    ${lib.getExe hex} model status | grep '^missing[[:space:]]'
    touch "$out"
  '';

  shell-environment =
    assert lib.all (name: shell.${name} == hex.${name}) [
      "BLA_VENDOR"
      "OPENSSL_NO_VENDOR"
    ];
    # OpenBLAS ordering belongs to build.rs, not an injected Nix-only workaround.
    assert !(hex ? RUSTFLAGS) && !(shell ? RUSTFLAGS);
    assert lib.all
      (package: builtins.elem "${lib.getLib package}/lib" (lib.splitString ":" shell.LD_LIBRARY_PATH))
      [
        pkgs.vulkan-loader
        pkgs.libayatana-appindicator
        pkgs.gtk-layer-shell
        pkgs.wayland
        pkgs.libxkbcommon
      ];
    assert lib.all (package: builtins.elem (lib.getDev package) shell.nativeBuildInputs) [
      pkgs.curl
      pkgs.wl-clipboard
      pkgs.wtype
    ];
    pkgs.runCommand "hex-shell-environment-check" { } ''
      test -d ${shell.ALSA_PLUGIN_DIR}
      touch "$out"
    '';

  modules =
    assert builtins.elem hex system.environment.systemPackages;
    assert builtins.elem hex enabled.home.packages;
    assert !(builtins.elem hex disabled.home.packages);
    assert !(manual.systemd.user.services ? hex);
    assert !(disabled.systemd.user.services ? hex);
    assert enabled.systemd.user.services.hex.Service.ExecStart == [ "${lib.getExe hex} app --hidden" ];
    assert enabled.systemd.user.services.hex.Service.Restart == "on-failure";
    assert
      custom.systemd.user.services.hex.Service.ExecStart == [ "${lib.getExe pkgs.hello} app --hidden" ];
    assert lib.all (name: custom.systemd.user.services.hex.Unit.${name} == [ "sway-session.target" ]) [
      "After"
      "Requisite"
      "PartOf"
    ];
    assert custom.systemd.user.services.hex.Install.WantedBy == [ "sway-session.target" ];
    pkgs.runCommand "hex-module-check" { } ''
      bash -n ${enabled.systemd.user.services.hex.Service.ExecStartPre}
      touch "$out"
    '';

  session-readiness =
    let
      xprop = pkgs.writeShellScriptBin "xprop" ''
        printf '%s\n' "''${HEX_TEST_XPROP:-}"
      '';
      sleep = pkgs.writeShellScriptBin "sleep" "exit 0";
    in
    pkgs.runCommand "hex-session-readiness-check" { } ''
      run() {
        env -i PATH=${
          lib.makeBinPath [
            xprop
            sleep
            pkgs.gnugrep
          ]
        } "$@" \
          ${pkgs.bash}/bin/bash -euo pipefail ${./wait-for-session.sh}
      }

      if run; then exit 1; fi
      if run DISPLAY=:1 HEX_TEST_XPROP='_NET_SUPPORTING_WM_CHECK: not found.'; then exit 1; fi
      if run DISPLAY=:1 HEX_TEST_XPROP='_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x0'; then exit 1; fi
      run DISPLAY=:1 HEX_TEST_XPROP='_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0xa00001'

      if run WAYLAND_DISPLAY=wayland-test XDG_RUNTIME_DIR="$TMPDIR"; then exit 1; fi
      ${pkgs.python3}/bin/python -c \
        'import socket, sys; sock = socket.socket(socket.AF_UNIX); sock.bind(sys.argv[1])' \
        "$TMPDIR/wayland-test"
      run WAYLAND_DISPLAY=wayland-test XDG_RUNTIME_DIR="$TMPDIR"
      run WAYLAND_DISPLAY="$TMPDIR/wayland-test"
      touch "$out"
    '';
}
