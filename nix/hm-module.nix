{
  config,
  lib,
  ...
}:

let
  cfg = config.programs.hex;
in
{
  options.programs.hex = {
    enable = lib.mkEnableOption "HEX local voice dictation";

    package = lib.mkOption {
      type = lib.types.package;
      description = "HEX package to install.";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Start HEX with the graphical session.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.hex = lib.mkIf cfg.autostart {
      Unit = {
        Description = "HEX local voice dictation";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${lib.getExe cfg.package} app --hidden";
        Restart = "on-failure";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
