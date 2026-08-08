{
  description = "md-preview: native Markdown previewer built on wry/tao and webkit2gtk";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      overlays.default = final: _prev: {
        md-preview = final.callPackage ./nix/package.nix { };
      };

      packages = forAllSystems (pkgs: rec {
        md-preview = pkgs.callPackage ./nix/package.nix { };
        default = md-preview;
      });
    };
}
