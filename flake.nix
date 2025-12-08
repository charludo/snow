{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      fenix,
      nixpkgs,
      utils,
      ...
    }:
    {
      nixosModules.snow = ./modules/snow.nix;
      nixosModules.default = self.nixosModules.snow;
    }
    // utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          system = system;
        };
        toolchain = fenix.packages.${system}.latest;
      in
      {
        packages.default =
          (pkgs.makeRustPlatform {
            cargo = toolchain.toolchain;
            rustc = toolchain.toolchain;
          }).buildRustPackage
            {
              pname = "snow";
              version = "0.2.2";
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
            };

        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            clang
            llvm
            llvmPackages.libclang
            lld
            pkg-config

            (toolchain.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
              "rust-analyzer"
            ])
          ];
          buildInputs = with pkgs; [
            udev
          ];
          LD_LIBRARY_PATH = nixpkgs.lib.makeLibraryPath buildInputs;
        };
      }
    );
}
