{
  description = "Stahl Capital — Tauri desktop GUI for portfolio_rs, runs on macOS, Linux, and NixOS";

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
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };
        craneLib = crane.mkLib pkgs;

        isDarwin = pkgs.stdenv.isDarwin;
        isLinux = pkgs.stdenv.isLinux;

        # --- Frontend (pnpm + Vite) ----------------------------------------
        # Produces frontend/dist which the Tauri Rust build embeds.
        # Uses nixpkgs' pnpm fetcher (fetcherVersion 4 = pnpm v11+ store
        # format, dumped to a reproducible tarball) plus pnpmConfigHook for
        # offline install in the build sandbox.
        #
        # nixpkgs' default nodejs (24.x) hits an ODR violation in V8's
        # hashing.h (https://github.com/NixOS/nixpkgs/issues/536039):
        # V8 injects its own `std::hash<int>` specialization, which the
        # linker mixes with libc++'s under LLVM 21, so a `std::unordered_set`
        # can insert under one hash and fail to erase under the other. Node
        # uses exactly that container to track open fds, so a large
        # `pnpm install` spams "File descriptor N ... not opened in
        # unmanaged mode" warnings and can get hard-killed mid-fetch in a
        # resource-constrained sandbox (reproduced locally). Fixed upstream
        # in V8 (Node 26+); we only need it for pnpm's own install steps, so
        # it's scoped here instead of swapping nixpkgs' default nodejs
        # everywhere.
        pnpmForFetch = pkgs.pnpm.override {"nodejs-slim" = pkgs."nodejs-slim_26";};

        # fetchPnpmDeps fetches every optionalDependency listed in the
        # lockfile (all os/cpu variants of native-binary packages like
        # esbuild/rolldown), not just the ones matching the host platform,
        # so this hash is identical across systems. Verified by comparing
        # the fetch output on aarch64-darwin and x86_64-linux.
        frontendDepsHash = "sha256-fBHNcPt1CqV9uCcEvaCGhAMi8Ia3eIJ+tZmtJC9ZGNc=";

        frontendDeps = pkgs.fetchPnpmDeps {
          pname = "stahl-frontend";
          version = "0.1.0";
          src = ./frontend;
          fetcherVersion = 4;
          hash = frontendDepsHash;
          pnpm = pnpmForFetch;
        };

        frontend = pkgs.stdenv.mkDerivation {
          pname = "stahl-frontend";
          version = "0.1.0";
          src = ./frontend;

          nativeBuildInputs = with pkgs; [
            nodejs
            pnpmForFetch
            pnpmConfigHook
            writableTmpDirAsHomeHook
          ];

          pnpmDeps = frontendDeps;

          # Build the Vite bundle into dist/.
          buildPhase = ''
            runHook preBuild
            pnpm run build
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            mkdir -p $out
            cp -r dist/* $out/
            runHook postInstall
          '';
        };

        # --- Rust + Tauri --------------------------------------------------
        # Tauri's build script reads frontendDist relative to src-tauri.
        # We stage the prebuilt frontend into the expected ../frontend/dist
        # location before crane invokes cargo build.
        commonArgs = {
          # The workspace root Cargo.toml has no [package], so crane would
          # otherwise emit placeholder-name warnings. Set these explicitly.
          pname = "stahl";
          version = "0.1.0";

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasSuffix "tauri.conf.json" path)
              # tauri::generate_context!() embeds src-tauri/capabilities/*.json
              # at compile time. Without this, the Nix-sandboxed build has no
              # capability files at all, so every plugin command (dialog,
              # shell, ...) is rejected with "not allowed by ACL" even though
              # our own #[tauri::command]s (which aren't ACL-gated by
              # default) work fine — the exact asymmetry that gave this away.
              || (pkgs.lib.hasInfix "/capabilities/" path)
              || (pkgs.lib.hasSuffix ".png" path)
              || (pkgs.lib.hasSuffix ".icns" path)
              || (pkgs.lib.hasSuffix ".ico" path)
              || (pkgs.lib.hasSuffix ".svg" path);
          };
          strictDeps = true;

          nativeBuildInputs = with pkgs;
            [
              pkg-config
            ]
            ++ pkgs.lib.optionals isLinux [
              wrapGAppsHook3
              gobject-introspection
            ];

          buildInputs =
            (with pkgs; [
              openssl
            ])
            ++ (pkgs.lib.optionals isLinux (
              with pkgs; [
                # Tauri 2 on Linux uses webkit2gtk-4.1 (WebKitGTK with libsoup3)
                webkitgtk_4_1
                gtk3
                libsoup_3
                glib
                glib-networking
                cairo
                pango
                gdk-pixbuf
                harfbuzz
                atk
                librsvg
                zlib
                gst_all_1.gstreamer
                gst_all_1.gst-plugins-base
                gst_all_1.gst-plugins-good
                gst_all_1.gst-plugins-bad
              ]
            ))
            ++ (pkgs.lib.optionals isDarwin (
              with pkgs; [
                # On macOS the Apple SDK (AppKit, WebKit, Security, Cocoa,
                # Foundation, CoreFoundation) is provided by stdenv's default
                # `apple-sdk`. The legacy `darwin.apple_sdk.frameworks.*` attrs
                # were removed in nixpkgs 25.11 — see
                # https://nixos.org/manual/nixpkgs/stable/#sec-darwin-legacy-frameworks
                # We only need to add libiconv explicitly here.
                libiconv
              ]
            ));

          # Stage the prebuilt frontend at the path Tauri's conf expects.
          preBuild = ''
            # Tauri.conf.json points at "../frontend/dist" relative to src-tauri.
            # Populate it from the prebuilt frontend derivation so the Rust
            # build embeds a known-good bundle instead of needing pnpm in the
            # cargo build sandbox.
            mkdir -p frontend/dist
            cp -r --dereference ${frontend}/* frontend/dist/

            # crane's buildDepsOnly compiles the `tauri` crate's build script
            # and writes permission files into target/release/build/tauri-<hash>/out/.
            # Those paths are baked into the cached build-script binary. When
            # buildPackage reuses the deps artifacts in a fresh build tree,
            # the path hashes don't match and the build script fails reading
            # `.../permissions/app/autogenerated/commands/app_hide.toml`.
            # Force the tauri crate's build script to rerun in this tree by
            # dropping its cached build artifacts.
            rm -rf target/release/build/tauri-* target/release/build/tauri-build-*
          '';
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        stahl = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            doCheck = false;

            # The workspace root Cargo.toml has no [package]; crane would
            # otherwise derive a placeholder name. Point it at the actual
            # crate and pin the member to build.
            cargoToml = ./src-tauri/Cargo.toml;
            # Enable `custom-protocol` so the Tauri webview loads the
            # embedded `dist/` at runtime instead of `devUrl`
            # (http://localhost:5173). `cargo tauri build` does this
            # automatically; with plain `cargo build --release` we must
            # opt in, otherwise the binary tries to connect to a Vite dev
            # server that isn't there ("Connection refused").
            cargoExtraArgs = "-p stahl --features custom-protocol";

            # WebKit2GTK's dmabuf renderer Wayland workaround
            # (WEBKIT_DISABLE_DMABUF_RENDERER) is set in main.rs itself so
            # it applies to any build (cargo build, cargo tauri dev/build,
            # or this flake), not just Nix-packaged binaries.

            # On Linux, the compiled binary needs GI_TYPELIBS at runtime;
            # wrapGAppsHook3 handles this in the installPhase.
            meta = with pkgs.lib; {
              description = "Stahl Capital — Tauri desktop GUI for portfolio_rs";
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
          packages =
            (with pkgs; [
              rust-analyzer
              rustfmt
              clippy
              cargo-watch
              cargo-tauri
              alejandra
              pkg-config
              openssl
              nodejs
              pnpm

              libiconv
              libidn2
            ])
            ++ (pkgs.lib.optionals isLinux (
              with pkgs; [
                # Allow `cargo tauri dev` to spawn a working webkit environment.
                webkitgtk_4_1
                gtk3
                libsoup_3
                glib
                gobject-introspection
                wrapGAppsHook3
              ]
            ));
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

          stahl-frontend-lint = pkgs.stdenv.mkDerivation {
            pname = "stahl-frontend-lint";
            version = "0.1.0";
            src = ./frontend;

            nativeBuildInputs = with pkgs; [
              nodejs
              pnpmForFetch
              pnpmConfigHook
            ];

            pnpmDeps = frontendDeps;

            buildPhase = ''
              runHook preBuild
              pnpm exec oxlint
              pnpm exec oxfmt --check
              pnpm exec tsc --noEmit
              runHook postBuild
            '';

            installPhase = "touch $out";
          };
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
