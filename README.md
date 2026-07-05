# Stahl Capital

[![CI](https://github.com/MarkusZoppelt/stahl/actions/workflows/ci.yml/badge.svg)](https://github.com/MarkusZoppelt/stahl/actions/workflows/ci.yml)

A local-first desktop GUI for [`portfolio_rs`](https://github.com/MarkusZoppelt/portfolio_rs) — track
investment portfolios, run policy-aware reviews, simulate rebalancing, and manage a durable finance
workspace, all from a native app. Built with [Tauri 2](https://tauri.app), React, and TypeScript.

> **Status:** early prototype. Expect rough edges.

## Features

- **Dashboard** — total value, day change, and PnL at a glance
- **Positions** — add, edit, and remove holdings with live quotes
- **Allocation** — asset-class breakdown with drift visualization
- **Performance** — historical and daily returns, top movers
- **Context** — agent-friendly portfolio briefing
- **Validate** — sanity-check a portfolio file
- **Workspace mode** — for full finance workspaces (`portfolio_rs init-workspace`):
  - **Policy** — view and edit your machine-readable investment policy
  - **Review** — policy-aware drift/violation checks
  - **Simulate** — rebalancing what-if scenarios (never trades)
  - **Documents** — generate weekly reports and decision records
- **Settings** — currency, theme, and AI provider configuration


## Building

### With Nix (recommended)

Builds reproducibly on macOS, Linux, and NixOS — no manual Rust/Node/GTK setup required.

```sh
nix build .#stahl      # produces ./result/bin/stahl
./result/bin/stahl
```

```sh
nix run .#stahl        # build + run in one step
```

```sh
nix develop             # dev shell: cargo, pnpm, cargo-tauri, rust-analyzer, clippy, etc.
cargo tauri dev          # hot-reloading dev build
```

> If `cargo tauri dev` fails with a `dyld`/`libiconv` error, you likely have a stale
> `cargo-tauri` binary in `~/.cargo/bin` (installed via `cargo install tauri-cli`) that shadows
> the one provided by the dev shell. Either remove it, or invoke `cargo-tauri dev` directly
> (skipping cargo's subcommand resolution, which always prefers `$CARGO_HOME/bin`).

### Without Nix

Requires Rust (stable), Node.js, [pnpm](https://pnpm.io), and the platform prerequisites for
[Tauri 2](https://v2.tauri.app/start/prerequisites/) (WebKitGTK + GTK3 on Linux; Xcode command line
tools on macOS).

```sh
pnpm --dir frontend install
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri dev     # development
cargo tauri build   # release bundle
```

## Development

Frontend linting and formatting use [Oxlint](https://oxc.rs/docs/guide/usage/linter.html) and
[Oxfmt](https://oxc.rs/docs/guide/usage/formatter.html) — fast, Rust-based drop-ins for ESLint and
Prettier:

```sh
pnpm --dir frontend lint        # oxlint
pnpm --dir frontend fmt         # oxfmt (writes)
pnpm --dir frontend fmt:check   # oxfmt --check
```

`nix flake check` runs the full suite in one shot: the release build, `cargo clippy --deny warnings`,
`cargo fmt --check`, and the frontend lint/format/typecheck. `nix fmt` formats the Nix code itself
(via [Alejandra](https://github.com/kamadorueda/alejandra)). CI (`.github/workflows/ci.yml`) runs
`nix flake check` on Linux and macOS for every push and pull request.

## Releases

Pushing a tag matching `v*` (e.g. `v0.1.0`) triggers `.github/workflows/release.yml`, which builds
native installers with [`tauri-action`](https://github.com/tauri-apps/tauri-action) — macOS (Apple
Silicon) `.dmg`/`.app`, Linux `.deb`/`.AppImage`, and Windows `.msi`/`.exe` — and attaches them to a
draft GitHub Release. This is a separate, non-Nix pipeline: `nix build` produces binaries dynamically
linked against `/nix/store` paths (fine for machines with Nix, not for end-user distribution), so
releases are built with each OS's native toolchain instead, the same way as the "Without Nix" section
above. macOS builds are ad-hoc signed (`signingIdentity: "-"` in `tauri.conf.json`) since there's no
Apple Developer certificate configured; users will still need to right-click → Open (or clear the
quarantine attribute) on first launch.

## Project structure

```
src-tauri/    Rust backend (Tauri commands, AppState wiring, config)
frontend/     React + TypeScript + Tailwind UI
flake.nix     Reproducible build for macOS, Linux, and NixOS
```

The Rust side is a thin layer: `src-tauri/src/commands.rs` exposes `portfolio_rs::state::AppState`
methods as Tauri commands; almost all domain logic (portfolio math, policy review, simulation,
reports) lives in the `portfolio_rs` library.

## License

MIT — see [LICENSE](LICENSE).
