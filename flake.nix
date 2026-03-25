{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        rustPlatform = pkgs.makeRustPlatform {
          rustc = rust;
          cargo = rust;
        };

        niri-tools = rustPlatform.buildRustPackage {
          pname = "niri-tools";
          version = "0.1.0";
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: _type: baseNameOf path != "target";
          };
          cargoLock.lockFile = ./Cargo.lock;
          buildType = "release";

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.wrapGAppsHook4
          ];
          buildInputs = [
            pkgs.gtk4
            pkgs.gtk4-layer-shell
            pkgs.wayland
            pkgs.pango
            pkgs.glib
            pkgs.cairo
            pkgs.graphene
            pkgs.gdk-pixbuf
            pkgs.harfbuzz
          ];
        };
      in {
        packages = {
          default = niri-tools;
          inherit niri-tools;
        };

        devShells.default = pkgs.mkShell {
          name = "niri-tools";

          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [
            rust
            pkgs.dprint
            pkgs.gtk4
            pkgs.gtk4-layer-shell
            pkgs.wayland
            pkgs.pango
            pkgs.glib
            pkgs.cairo
            pkgs.graphene
            pkgs.gdk-pixbuf
            pkgs.harfbuzz
          ];
        };
      }
    );
}
