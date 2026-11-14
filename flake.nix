{
  description = "Stahl Capital — native desktop app for portfolio_rs, built on Rune, runs on macOS, Linux, and NixOS";

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

        # Rune needs a newer compiler than nixpkgs ships. rust-toolchain.toml
        # is the single pin, shared with rustup and the release workflow.
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        isLinux = pkgs.stdenv.isLinux;

        # Rune links xkbcommon and wayland. winit and wgpu dlopen the rest at
        # runtime, so linking alone does not find them.
        linuxRuntimeLibs = with pkgs; [
          libxkbcommon
          vulkan-loader
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
          libxcb
        ];

        # The app embeds its fonts and its mark.
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          name = "source";
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (pkgs.lib.hasInfix "/assets/" path
              && (type == "directory" || builtins.match ".*\\.(ttf|png)" path != null));
        };

        # rune-fx reads Rune's shaders from its sibling crate (`../../rune/src/*.wgsl`), but crane
        # vendors each crate of a git checkout on its own. Put Rune's `src` where rune-fx looks.
        cargoVendorDir = let
          vendored = craneLib.vendorCargoDeps {inherit src;};
        in
          pkgs.runCommand "vendor-cargo-deps-rune-shaders" {} ''
            cp -rL ${vendored} $out
            chmod -R u+w $out
            substituteInPlace $out/config.toml --replace-quiet ${vendored} $out
            for fx in $out/*/rune-fx-*; do
              checkout=$(dirname "$fx")
              rune=$(find "$checkout" -maxdepth 1 -type d -name 'rune-[0-9]*' | head -n 1)
              mkdir -p "$checkout/rune"
              cp -r "$rune/src" "$checkout/rune/src"
            done
          '';

        commonArgs = {
          inherit src cargoVendorDir;
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
              description = "Stahl Capital — native desktop app for portfolio_rs";
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

        devShells =
          {
            default = craneLib.devShell {
              inputsFrom = [stahl];
              packages = with pkgs; [
                cargo-watch
                alejandra
              ];
              LD_LIBRARY_PATH = pkgs.lib.optionalString isLinux (pkgs.lib.makeLibraryPath linuxRuntimeLibs);
            };
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
            # The iOS app (ios/build-sim.sh): the pinned toolchain with the iOS targets. Nix's
            # clang only knows macOS, so iOS code compiles and links with Xcode's.
            ios = pkgs.mkShellNoCC {
              packages = [
                (rustToolchain.override {
                  targets = ["aarch64-apple-ios" "aarch64-apple-ios-sim"];
                })
                pkgs.cmake
              ];
              CC_aarch64_apple_ios = "/usr/bin/clang";
              CC_aarch64_apple_ios_sim = "/usr/bin/clang";
              CARGO_TARGET_AARCH64_APPLE_IOS_LINKER = "/usr/bin/clang";
              CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER = "/usr/bin/clang";
            };
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

          # Includes tests/e2e.rs: Rune's headless runtime needs no display or GPU.
          stahl-test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
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
