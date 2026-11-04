{
  description = "Marbble, a cross-platform DAW with VST3 hosting, modelled on Logic Pro";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        marbble = pkgs.callPackage ./nix/package.nix { };
        default = marbble;
      });

      apps = forAllSystems (pkgs: rec {
        marbble = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.marbble;
        };
        default = marbble;
      });

      devShells = forAllSystems (pkgs: {
        default = import ./nix/shell.nix {
          inherit pkgs;
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) marbble;
        };
      });

      # For NixOS / home-manager configurations: `nixpkgs.overlays = [ marbble.overlays.default ];`
      overlays.default = final: _prev: {
        marbble = final.callPackage ./nix/package.nix { };
      };

      checks = forAllSystems (pkgs: {
        # Builds the app and runs the engine's unit tests.
        marbble = self.packages.${pkgs.stdenv.hostPlatform.system}.marbble;
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
