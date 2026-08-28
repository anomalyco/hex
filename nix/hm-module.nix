{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.hex;
  waitForSession = pkgs.writeShellApplication {
    name = "hex-wait-for-session";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.gnugrep
      pkgs.xprop
    ];
    text = builtins.readFile ./wait-for-session.sh;
  };
in
{
  options.programs.hex = {
    enable = lib.mkEnableOption "HEX local voice dictation";
    package = lib.mkOption {
      type = lib.types.package;
      description = "HEX package to install.";
    };
    autostart = lib.mkEnableOption "HEX startup with the graphical session";
    systemdTarget = lib.mkOption {
      type = lib.types.str;
      default = "graphical-session.target";
      example = "sway-session.target";
      description = ''
        User target that owns the graphical session. The compositor or session
        manager must import its display environment before starting this target,
        and stop the target at logout. This module does not start a session.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
        message = "The HEX Nix package supports x86_64-linux only.";
      }
    ];
    home.packages = [ cfg.package ];
    systemd.user.services.hex = lib.mkIf cfg.autostart {
      Unit = {
        Description = "HEX local voice dictation";
        After = [ cfg.systemdTarget ];
        Requisite = [ cfg.systemdTarget ];
        PartOf = [ cfg.systemdTarget ];
      };
      Service = {
        ExecStartPre = lib.getExe waitForSession;
        ExecStart = "${lib.getExe cfg.package} app --hidden";
        TimeoutStartSec = 35;
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ cfg.systemdTarget ];
    };
  };
}
