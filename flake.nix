{
  description = "Stahl Capital — native GPUI desktop app for portfolio_rs, runs on macOS, Linux, and NixOS";

  nixConfig = {
    extra-substituters = [
      "https://nix-community.cachix.org"
    ];
    extra-trusted-public-keys = [
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        # GPUI needs a newer compiler than nixpkgs ships. rust-toolchain.toml
        # is the single pin, shared with rustup and the release workflow.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        isLinux = pkgs.stdenv.isLinux;

        # Loaded with dlopen at runtime, so linking alone does not find them.
        linuxRuntimeLibs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          vulkan-loader
          wayland
          libx11
          libxcb
        ];

        commonArgs = {
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              # The theme and icons are embedded at compile time.
              || (pkgs.lib.hasInfix "/assets/" path);
          };
          strictDeps = true;

          nativeBuildInputs = with pkgs;
            [
              pkg-config
              # aws-lc-sys, behind the TLS stack, builds C with it.
              cmake
            ]
            ++ pkgs.lib.optionals isLinux [
              makeWrapper
            ];

          buildInputs =
            pkgs.lib.optionals isLinux linuxRuntimeLibs
            # stdenv's default apple-sdk provides the frameworks on macOS.
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs; [
              libiconv
            ]);
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        stahl = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            # Tests run in the `stahl-test` check.
            doCheck = false;

            postFixup = pkgs.lib.optionalString isLinux ''
              wrapProgram $out/bin/stahl \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath linuxRuntimeLibs}
            '';

            meta = with pkgs.lib; {
              description = "Stahl Capital — native GPUI desktop app for portfolio_rs";
              homepage = "https://github.com/MarkusZoppelt/stahl";
              license = licenses.mit;
              mainProgram = "stahl";
              maintainers = [maintainers.MarkusZoppelt];
              platforms = platforms.linux ++ platforms.darwin;
            };
          }
        );
      in {
        packages = {
          default = stahl;
          stahl = stahl;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = stahl;
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [stahl];
          packages = with pkgs; [
            cargo-watch
            alejandra
          ];
          LD_LIBRARY_PATH = pkgs.lib.optionalString isLinux (pkgs.lib.makeLibraryPath linuxRuntimeLibs);
        };

        checks = {
          inherit stahl;

          stahl-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          stahl-fmt = craneLib.cargoFmt commonArgs;

          stahl-test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
              # tests/e2e.rs needs a login session, which the sandbox lacks
              # (no NSPasteboard on macOS). CI runs it outside the sandbox.
              cargoTestExtraArgs = "--lib --bins";
            }
          );
        };

        formatter = pkgs.alejandra;
      }
    )
    // {
      overlays.default = final: prev: {
        stahl = self.packages.${final.system}.stahl;
      };
    };
}
