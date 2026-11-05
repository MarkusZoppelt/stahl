# Agent notes

Guidance for AI agents (and humans) working on this repo.

## Rust style

- Prefer top-level `use` imports over inline fully-qualified paths. Write:

  ```rust
  use std::fs;
  ...
  fs::write(&path, data)?;
  ```

  not:

  ```rust
  std::fs::write(&path, data)?;
  ```

  This applies even to single call sites and to imports inside a function body
  (`use std::os::unix::fs::PermissionsExt;` inside a `fn` should be hoisted to
  the top of the file instead).

- If an import is only needed under a `#[cfg(...)]`-gated function or block,
  gate the `use` with the same `#[cfg(...)]` rather than importing it
  unconditionally (which would produce an unused-import warning on other
  platforms). Example: `main.rs`'s Linux-only display check uses
  `#[cfg(target_os = "linux")] use std::process;`.

- Run `cargo fmt` after adding/moving imports — rustfmt reorders and groups
  `use` statements (including `#[cfg(...)]`-gated ones) and CI enforces this
  via `cargo fmt --check` (`nix flake check` → `stahl-fmt`).

- `cargo clippy --all-targets -- --deny warnings` must be clean (also
  enforced by `nix flake check` → `stahl-clippy`).

## GPUI

GPUI comes through [`gpui-kit`](https://github.com/longbridge/gpui-kit), which
pins a `gpui-pre` snapshot of Zed's crates and adds the widgets (inputs, editor,
charts, dialogs, command palette). Import everything from `gpui_kit`. Updating
it can move GPUI's API, and may raise the Rust version it needs.

Things that are easy to get wrong:

- `KeyBinding::new` panics on `secondary-*`. Use the `key!` macro in `app.rs`.
- Key and menu dispatch is already updating the window, so an action handler
  cannot call `window.update`. App-wide actions go through `with_shell`, which
  defers. Code inside `with_shell` is also inside an update of the shell, so it
  must not `shell.read(cx)`.
- `portfolio_rs` needs Tokio and GPUI has its own executors. Reach the backend
  through `ui::run_backend` or `ui::run_backend_in`.

## Tests

`tests/e2e.rs` drives the real app headlessly. It has its own `main` because
GPUI's macOS platform must own the main thread. It needs a login session, so it
does not run in the Nix sandbox: `nix flake check` skips it, and CI runs it
separately. Unit tests cannot catch the problems above, so run it, and look at
the result of UI changes:

```sh
STAHL_SCREENSHOTS=/tmp/shots cargo test --test e2e
```

Tests must not touch the user's config. `AppState::load_workspace` and
`load_simple_file` save it to the default location whatever `Backend` was given,
so tests open portfolios through `Backend::startup` instead.

When running the app by hand, isolate it from real data: set `HOME` to a temp
directory and `PORTFOLIO_RS_QUOTES=offline`. Opening a portfolio records a
valuation in its history.

## Nix flake maintenance

`rust-toolchain.toml` is the single Rust version pin, shared by rustup, the
release workflow, and the flake (through `rust-overlay`, because GPUI needs a
newer compiler than nixpkgs ships). Do not add a second pin.

Dependabot owns Cargo and GitHub Actions. A weekly workflow updates `flake.lock`.

## CI

`.github/workflows/ci.yml` uses `cachix/install-nix-action` (not
DeterminateSystems' installer/FlakeHub — keep this on plain, non-proprietary
Nix tooling) and runs `nix flake check` + `nix build .#stahl` on Linux and
macOS, plus the end-to-end tests on macOS. It reads from the public
`cache.nixos.org` and `nix-community.cachix.org` substituters already declared in `flake.nix`'s
`nixConfig`; there is no project-specific push cache configured.
