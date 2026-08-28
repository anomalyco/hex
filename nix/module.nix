{
  config,
  lib,
  pkgs,
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
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
        message = "The HEX Nix package supports x86_64-linux only.";
      }
    ];
    environment.systemPackages = [ cfg.package ];
  };
}
