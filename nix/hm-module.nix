{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.hex;
  json = pkgs.formats.json { };
  supportDir = "${config.xdg.dataHome}/hex";
  settings = lib.filterAttrs (_: value: value != null) {
    dictation_hotkey = cfg.shortcut;
    "transcription.model" = cfg.model;
    "transcription.language" = cfg.language;
  } // cfg.settings;
  # User managers have no network-online.target of their own, so this asks
  # the system's, then tries anyway; failed downloads retry.
  installModel = pkgs.writeShellApplication {
    name = "hex-install-model";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.systemd
    ];
    text = ''
      for _ in $(seq 60); do
        systemctl --system is-active --quiet network-online.target && break
        sleep 2
      done
      exec ${lib.getExe cfg.package} model install
    '';
  };
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
    enable = lib.mkEnableOption "Hex local voice dictation";
    package = lib.mkOption {
      type = lib.types.package;
      description = "Hex package to install.";
    };
    autostart = lib.mkEnableOption "Hex startup with the graphical session";
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
    settings = lib.mkOption {
      inherit (json) type;
      default = { };
      example = lib.literalExpression ''
        {
          sound_effect_volume = 0.4;
          shortcut_mode = "toggle";
          "meetings.enabled" = true;
        }
      '';
      description = ''
        Settings applied on every activation, keyed as `hex settings list`
        names them. Declared keys overwrite Hex's values; settings changed in
        the window that aren't declared here keep their values. A running Hex
        adopts them at once; otherwise they are written to settings.json.
      '';
    };
    shortcut = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "control+option+space";
      description = "Dictation shortcut, the `dictation_hotkey` setting.";
    };
    model = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "parakeet_v3";
      description = ''
        Transcription model id from `hex model list`. Declaring it only
        selects it; set `installModel` to download it.
      '';
    };
    language = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "de";
      description = "Dictation language, a code the selected model supports.";
    };
    installModel = lib.mkEnableOption ''
      downloading the selected model when it is missing, from a user service
      that runs once the network is online. Activation itself never downloads
    '';
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
        message = "The Hex Nix package supports x86_64-linux only.";
      }
    ];
    home.packages = [ cfg.package ];
    # After reloadSystemd, so a restarted Hex is the one that answers.
    home.activation.hexSettings = lib.mkIf (settings != { }) (
      lib.hm.dag.entryAfter [ "reloadSystemd" ] ''
        if ! run ${pkgs.coreutils}/bin/env HEX_APPLICATION_SUPPORT_DIR=${lib.escapeShellArg supportDir} \
          ${lib.getExe cfg.package} settings apply ${json.generate "hex-settings.json" settings}; then
          warnEcho "Hex didn't apply programs.hex.settings; see the message above."
        fi
      ''
    );
    systemd.user.services.hex = {
      Unit = {
        Description = "Hex local voice dictation";
        After = [ cfg.systemdTarget ];
        PartOf = [ cfg.systemdTarget ];
      };
      Service = {
        Type = "notify";
        NotifyAccess = "main";
        ExecStartPre = lib.getExe waitForSession;
        ExecStart = "${lib.getExe cfg.package} service";
        Environment = [ "HEX_APPLICATION_SUPPORT_DIR=${supportDir}" ];
        EnvironmentFile = "-${supportDir}/session.env";
        TimeoutStartSec = 35;
        Restart = "on-failure";
        RestartSec = 5;
        TimeoutStopSec = 30;
        UMask = "0077";
      };
      Install.WantedBy = lib.optionals cfg.autostart [ cfg.systemdTarget ];
    };
    # Type=exec, so a switch never waits on the download.
    systemd.user.services.hex-model = lib.mkIf cfg.installModel {
      Unit.Description = "Download Hex's selected transcription model";
      Service = {
        Type = "exec";
        ExecStart = lib.getExe installModel;
        Environment = [ "HEX_APPLICATION_SUPPORT_DIR=${supportDir}" ];
        Restart = "on-failure";
        RestartSec = 60;
        UMask = "0077";
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
