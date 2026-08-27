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
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
