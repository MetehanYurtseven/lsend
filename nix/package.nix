{ craneLib, lib }:
let
  commonArgs = {
    src = craneLib.cleanCargoSource ../.;
    strictDeps = true;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (
  commonArgs
  // {
    inherit cargoArtifacts;
    # Reused by the clippy and fmt checks in flake.nix.
    passthru = { inherit commonArgs cargoArtifacts; };
    meta = {
      mainProgram = "lsendctl";
      license = lib.licenses.mit;
    };
  }
)
