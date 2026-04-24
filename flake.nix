{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crate2nix.url = "github:bengsparks/crate2nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      crate2nix,
      fenix,
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
          inherit system;
        };
        fenix' = fenix.packages.${system};
        toolchain = fenix'.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-eJQEH3Nhkib52jvDqq2wdEmewxXbdl1oN6OGTGf7WH4=";
        };

        cargoNix = crate2nix.tools.${system}.generatedCargoNix {
          name = "snow";
          cargo = toolchain;
          src = ./.;
        };

        cargoNixPackage = pkgs.callPackage cargoNix {
          release = true;
          buildRustCrateForPkgs =
            pkgs:
            pkgs.buildRustCrate.override {
              rustc = toolchain;
              cargo = toolchain;
            };
          defaultCrateOverrides = pkgs.defaultCrateOverrides // {
            kdam = _: {
              CARGO_CRATE_NAME = "kdam";
            };
          };
        };
      in
      {
        packages.default = cargoNixPackage.rootCrate.build;

        formatter = pkgs.treefmt;

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            pkgsCross.musl64.buildPackages.gcc
            toolchain

            # formatters
            deadnix
            nixfmt
            taplo
            clang-tools
          ];

          shellHook =
            #bash
            ''
              set_if_unset() {
                if [ -z "$(eval \$$1)" ]; then
                  export "$1"="$2"
                fi
              }

              export CC_x86_64_unknown_linux_musl=${pkgs.pkgsCross.musl64.buildPackages.gcc}/bin/x86_64-unknown-linux-musl-gcc
              export AR_x86_64_unknown_linux_musl=${pkgs.pkgsCross.musl64.buildPackages.gcc}/bin/x86_64-unknown-linux-musl-ar
            '';
        };
      }
    );
}
