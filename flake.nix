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
            filter = path: _type: builtins.baseNameOf path != "target";
          };
          cargoLock.lockFile = ./Cargo.lock;
          buildType = "release";
        };
      in {
        packages = {
          default = niri-tools;
          inherit niri-tools;
        };

        devShells.default = pkgs.mkShell {
          name = "niri-tools";

          buildInputs = [
            rust
            pkgs.dprint
          ];
        };
      }
    );
}
