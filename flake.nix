{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
      in
      {
        defaultPackage = naersk-lib.buildPackage {
          src = ./.;
        };

        devShell = with pkgs; mkShell {
          buildInputs = [
            cargo
            rustc
            rustfmt
            pre-commit
            rustPackages.clippy
            protobuf
          ];
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
          
          shellHook = ''
            echo "Development shell activated"
            echo "Note: protobuf is available for regenerating bindings"
            echo "To regenerate protobuf files, run: cargo build -p lla_plugin_interface --features regenerate-protobuf"
          '';
        };

        packages.withProtobuf = naersk-lib.buildPackage {
          src = ./.;
          buildInputs = with pkgs; [ protobuf ];
          cargoBuildOptions = [ "-p" "lla_plugin_interface" ];
          cargoBuildFeatures = [ "regenerate-protobuf" ];
        };
      }
    );
}
