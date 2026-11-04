{
  description = "HEX local-first voice dictation";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      package = pkgs: self.packages.${pkgs.stdenv.hostPlatform.system}.hex;
    in
    {
      packages = forSystems (
        pkgs:
        let
          hex = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit hex;
          default = hex;
        }
      );

      overlays.default = final: _prev: {
        hex = final.callPackage ./nix/package.nix { };
      };

      nixosModules.hex =
        { pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          programs.hex.package = nixpkgs.lib.mkDefault (package pkgs);
        };
      nixosModules.default = self.nixosModules.hex;

      homeManagerModules.hex =
        { pkgs, ... }:
        {
          imports = [ ./nix/hm-module.nix ];
          programs.hex.package = nixpkgs.lib.mkDefault (package pkgs);
        };
      homeManagerModules.default = self.homeManagerModules.hex;

      devShells = forSystems (pkgs: {
        default = (package pkgs).devShell;
      });
      checks = forSystems (pkgs: import ./nix/checks.nix { inherit self pkgs; });
      formatter = forSystems (pkgs: pkgs.nixfmt);
    };
}
