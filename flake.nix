{
  description = "HEX local-first voice dictation";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
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
        programs.hex.package = nixpkgs.lib.mkDefault self.packages.${system}.hex;
      };
      nixosModules.default = self.nixosModules.hex;

      homeManagerModules.hex = {
        imports = [ ./nix/hm-module.nix ];
        programs.hex.package = nixpkgs.lib.mkDefault self.packages.${system}.hex;
      };
      homeManagerModules.default = self.homeManagerModules.hex;

      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ hex ];
        packages = with pkgs; [
          rustc
          cargo
          rust-analyzer
          rustfmt
          clippy
        ];
      };

      formatter.${system} = pkgs.nixfmt;
    };
}
