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
    enable = lib.mkEnableOption "Hex local voice dictation";
    package = lib.mkOption {
      type = lib.types.package;
      description = "Hex package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
        message = "The Hex Nix package supports x86_64-linux only.";
      }
    ];
    environment.systemPackages = [ cfg.package ];
  };
}
