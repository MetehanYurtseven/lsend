{
  description = "lsend — scriptable LocalSend (daemon + CLI)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

      rustToolchain = pkgs.rust-bin.stable."1.97.1".default.override {
        extensions = [
          "rust-analyzer"
          "rustfmt"
          "clippy"
          "rust-src"
        ];
      };

      craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);
      src = craneLib.cleanCargoSource ./.;

      commonArgs = {
        inherit src;
        strictDeps = true;
        pname = "lsend";
        version = "0.1.0";
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      lsend = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
        }
      );
    in
    {
      packages.${system}.default = lsend;

      devShells.${system}.default = craneLib.devShell {
        packages = [
          rustToolchain
          pkgs.nil
          pkgs.nixd
        ];
      };
    };
}
