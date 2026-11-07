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

## Rune

The UI is [Rune](https://github.com/anomalyco/rune) and `rune-kit`, pinned to one git revision in
`Cargo.toml`. Rune is pre-release and moves fast, so read the README at the pinned revision rather
than trusting memory, and expect an update to move its API. Both crates must stay on the same `rev`.

The shape of the app: one `State`, and views that are pure functions of it. Rune re-runs them when
state changes, so a view must not do work or mutate anything.

Things that are easy to get wrong:

- `portfolio_rs` needs Tokio. Reach the backend through `State::run`: the work runs on Tokio, and its
  result lands in `poll` on the UI thread. Headless code waits for it with `State::settle`.
- A row stretches its children by default. Use `row().center_y()` unless you want that.
- `TextInput` is 280pt wide unless told otherwise, and `TextInput::label` only names it for assistive
  technology. Use `.width(None)` to fill a column and `ui::field` for a visible label.
- Write shortcuts with `ui::chord` or `ui::chord_keys`: Rune's command chords are Ctrl off macOS.
- A window can't refuse to close. `on_close` reopens it to ask about unsaved changes.
- Native menus, context menus, the Dock menu and `drag_out` are macOS only. Everything they do must
  also be reachable through a shortcut or the command palette.

## Tests

`tests/e2e.rs` drives the real views and handlers through pointer and key input in Rune's headless
runtime. It needs no display or GPU, so `nix flake check` runs it. Tests can't judge how a screen
looks, so render the ones you changed and look at them:

```sh
cargo run --example demo -- --snap /tmp/x.png --scene holdings --at 2 --size 1320x860
```

Every screen has a scene in `examples/demo.rs`; add one with each new screen. Regenerate
`screenshots/` the same way when the UI changes.

Tests must not touch the user's config. `AppState::load_workspace` and
`load_simple_file` save it to the default location whatever `Backend` was given,
so tests open portfolios through `Backend::startup`, on a backend from `demo::backend`.

When running the app by hand, use the demo, or isolate it from real data: set `HOME` to a temp
directory and `PORTFOLIO_RS_QUOTES=offline`. Opening a portfolio records a valuation in its history.

## Landing page

`site/` is the landing page: plain HTML, CSS and JavaScript with no build step, deployed by
`.github/workflows/pages.yml`. Paths stay relative, because it's served from `/stahl/`.

- Its images and video come from the real app. When the UI changes, run
  `nix develop -c site/render-media.sh`. The tour's input is in `site/media/tour.json`, and its
  chapter times are repeated in `CHAPTERS` in `site/app.js`, so change them together.
- The sketches in `site/app.js` use the prices and targets in `src/demo.rs`, so its totals match the
  footage. Change them together.
- The page makes specific claims about what the app does, including everything that leaves the
  machine (the letter in `site/index.html`, and `site/llms.txt`). A change that makes one false, like
  a new network request, has to update them.
- Its content security policy forbids contacting any other origin, and the letter says so. Keep it
  that way: no analytics, and no fonts or scripts from anywhere else.

To preview it, serve `site/` on loopback with `python3 -m http.server --bind 127.0.0.1 -d site`.

## Nix flake maintenance

`rust-toolchain.toml` is the single Rust version pin, shared by rustup, the
release workflow, and the flake (through `rust-overlay`, because Rune needs a
newer compiler than nixpkgs ships). Do not add a second pin.

Dependabot owns Cargo and GitHub Actions. A weekly workflow updates `flake.lock`.

## CI

`.github/workflows/ci.yml` uses `cachix/install-nix-action` (not
DeterminateSystems' installer/FlakeHub — keep this on plain, non-proprietary
Nix tooling) and runs `nix flake check` + `nix build .#stahl` on Linux and
macOS. A second job covers Linux, macOS and Windows with plain cargo, because Nix can't build for
Windows: clippy, tests, renders of the demo scenes (every one, or four on Windows), and a real window. It reads from the public
`cache.nixos.org` and `nix-community.cachix.org` substituters already declared in `flake.nix`'s
`nixConfig`; there is no project-specific push cache configured.
