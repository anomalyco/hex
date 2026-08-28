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
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      hex = pkgs.callPackage ./nix/package.nix { };
    in
    {
      packages.${system} = {
        inherit hex;
        default = hex;
      };

      overlays.default = final: _prev: {
        hex = final.callPackage ./nix/package.nix { };
      };

      nixosModules.hex = {
        imports = [ ./nix/module.nix ];
        programs.hex.package = nixpkgs.lib.mkDefault hex;
      };
      nixosModules.default = self.nixosModules.hex;

      homeManagerModules.hex = {
        imports = [ ./nix/hm-module.nix ];
        programs.hex.package = nixpkgs.lib.mkDefault hex;
      };
      homeManagerModules.default = self.homeManagerModules.hex;

      devShells.${system}.default = hex.devShell;
      checks.${system} = import ./nix/checks.nix { inherit self pkgs; };
      formatter.${system} = pkgs.nixfmt;
    };
}
