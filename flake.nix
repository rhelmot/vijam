{

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        inherit (pkgs) lib;
        libs = with pkgs; [
          alsa-lib
          libGL
          libxkbcommon
          wayland
        ];
      in
      {
        devShells.default =
          with pkgs;
          mkShell {
            packages =
              with pkgs;
              [
                pkg-config
                rustPackages.cargo
                rustPackages.rustc
                rustPackages.rustfmt
                rustPackages.clippy
              ]
              ++ libs;

            LD_LIBRARY_PATH = "${lib.makeLibraryPath libs}";
            RUST_SRC_PATH = "${rustPackages.rustPlatform.rustLibSrc}";
          };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
