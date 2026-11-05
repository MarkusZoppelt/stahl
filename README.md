# Stahl Capital

[![CI](https://github.com/MarkusZoppelt/stahl/actions/workflows/ci.yml/badge.svg)](https://github.com/MarkusZoppelt/stahl/actions/workflows/ci.yml)

A local-first, native desktop app for [`portfolio_rs`](https://github.com/MarkusZoppelt/portfolio_rs):
track investment portfolios, see how they got here, run policy-aware reviews, and simulate
rebalancing. Your data stays in plain files on your own machine. Built in Rust on
[GPUI](https://www.gpui.rs), with no web view.

![Overview](screenshots/overview.png)

> **Status:** early. Expect rough edges.

## Features

- **Overview:** total value, today's change, and value over time at a glance, with what needs attention
- **Holdings:** a sortable, searchable table with a detail pane for cost basis and purchases
- **Allocation:** the split today, compared with your policy's targets
- **Performance:** value over time with time-weighted return, separating market moves from money you added
- **Review** (workspace): drift, constraint checks, data quality, and suggested actions
- **Rebalance** (workspace): what-if scenarios. It never trades.
- **Policy** (workspace): your machine-readable investment policy, in an editor with a readable summary
- **Documents** (workspace): weekly reports and decision records
- **Briefing:** the summary an advisor would ask for, with Copy as Markdown to hand it to an AI assistant
- **Data Health:** file validation, data-quality findings, and workspace checks

And it behaves like a desktop app:

- **Command palette** (`⌘K`) and shortcuts for every page (`⌘1`–`⌘0`)
- **Hide amounts** (`⌘⇧H`) before you share your screen
- **Drop a file or folder** on the window to open it
- **Unsaved changes** show in the title bar, and quitting offers to save them
- **A broken positions file** shows the line and column at fault. It is never shown as an empty portfolio, and never overwritten.
- **Price freshness** in the toolbar: live, stale, partial, or offline
- Native menus and file panels, light and dark themes

### History

To chart your portfolio over time, Stahl records one valuation per day when it opens a portfolio or
refreshes prices, if every position has a price. It uses the same files as `portfolio_rs snapshot`,
so both add to one history:

- in a workspace, `portfolio/history/main/snapshots.jsonl`
- otherwise, `$XDG_DATA_HOME/portfolio_rs/main/snapshots.jsonl` (or under `~/.local/share`)
- anywhere you like, with `PORTFOLIO_RS_DATA_DIR`

Encrypted (`.gpg`) portfolios are never recorded, because that would put their values on disk in
plaintext.

## Run

```sh
nix run github:MarkusZoppelt/stahl
```

Pass a positions file to open it: `stahl positions.json`.

## Install

```sh
nix profile install github:MarkusZoppelt/stahl
```

Installers (`.dmg`, `.deb`, and a Windows `.zip`) are attached to
[GitHub Releases](https://github.com/MarkusZoppelt/stahl/releases).

## Building

### With Nix (recommended)

```sh
nix build .#stahl      # ./result/bin/stahl
nix run .#stahl        # build + run
nix develop            # the pinned Rust toolchain and everything else
cargo run
```

### Without Nix

Requires [rustup](https://rustup.rs), which picks up the toolchain pinned in `rust-toolchain.toml`.
On Linux you also need the development packages for Vulkan, Wayland, X11, xkbcommon, fontconfig, and
freetype; `.github/workflows/release.yml` has the list for Debian and Ubuntu. On macOS the Xcode
command line tools are enough.

```sh
cargo run
```

## Development

```sh
cargo test
STAHL_SCREENSHOTS=/tmp/shots cargo test --test e2e   # also renders every screen to PNG
```

`tests/e2e.rs` drives the real app in a headless window, with clicks and shortcuts, on real
`portfolio_rs` data and fixture quotes. To try the app without the network, set
`PORTFOLIO_RS_QUOTES=offline` or `fixture:<path>`.

`nix flake check` runs the release build, `cargo clippy --deny warnings`, `cargo fmt --check`, and
the unit tests. The end-to-end tests need a login session, so they run outside it. `nix fmt` formats
the Nix code. CI (`.github/workflows/ci.yml`) runs all of this for every push and pull request:
`nix flake check` on Linux and macOS, and the end-to-end tests on macOS.

## Releases

Pushing a tag matching `v*` (e.g. `v0.2.0`) triggers `.github/workflows/release.yml`, which builds
installers with each OS's native toolchain and attaches them to a draft GitHub Release. This is
separate from Nix: `nix build` produces binaries linked against `/nix/store` paths, which is fine for
machines with Nix but not for distribution. macOS builds are ad-hoc signed, as there is no Apple
Developer certificate configured, so users need to right-click → Open (or clear the quarantine
attribute) on first launch.

## Project structure

```
src/backend.rs   The bridge to portfolio_rs, which runs on Tokio beside GPUI
src/model.rs     State shared across the window
src/app.rs       Window, menus, shortcuts, command palette, sidebar
src/pages/       One module per screen
src/ui.rs        Layout pieces shared by the pages
assets/          Theme, app icons, embedded UI icons (Lucide)
tests/e2e.rs     End-to-end tests on the real UI
flake.nix        Reproducible build for macOS, Linux, and NixOS
```

Almost all domain logic (portfolio math, policy review, simulation, reports) lives in the
`portfolio_rs` library.

## License

MIT — see [LICENSE](LICENSE).
