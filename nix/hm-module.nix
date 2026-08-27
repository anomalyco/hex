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
      description = "Start HEX hidden in the tray when the graphical session starts.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."autostart/HEX.desktop" = lib.mkIf cfg.autostart {
      text = ''
        [Desktop Entry]
        Type=Application
        Name=HEX
        Comment=Local voice dictation
        Exec=${lib.getExe cfg.package} app --hidden
        Icon=audio-input-microphone
        Terminal=false
        X-GNOME-Autostart-enabled=true
      '';
    };
  };
}
