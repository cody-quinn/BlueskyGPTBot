{
  description = "bluesky-gptbot";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/master";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustNightly = pkgs.rust-bin.nightly."2023-02-10".default;
      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            (rustNightly.override {
              extensions = [ "rust-src" "rust-analyzer" ];
            })

            cargo-deny
            cargo-release

            pkg-config
            openssl
          ];
        };
      }
    );
}
