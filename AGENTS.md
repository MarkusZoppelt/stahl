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
  platforms). Example: `main.rs`'s Linux-only `WEBKIT_DISABLE_DMABUF_RENDERER`
  workaround uses `#[cfg(target_os = "linux")] use std::env;`.

- Run `cargo fmt` after adding/moving imports — rustfmt reorders and groups
  `use` statements (including `#[cfg(...)]`-gated ones) and CI enforces this
  via `cargo fmt --check` (`nix flake check` → `stahl-fmt`).

- `cargo clippy --all-targets -- --deny warnings` must be clean (also
  enforced by `nix flake check` → `stahl-clippy`).

## Nix flake maintenance

`flake.nix` builds the frontend hermetically via `pkgs.fetchPnpmDeps`, which
is a fixed-output derivation pinned to `frontendDepsHash`. Any change to
`frontend/package.json` or `frontend/pnpm-lock.yaml` invalidates that hash.
To regenerate it:

1. Set `frontendDepsHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";`
   (nix's `lib.fakeHash`) in `flake.nix`.
2. Run `nix build .#stahl -L` and copy the `got: sha256-...` value from the
   hash-mismatch error back into `frontendDepsHash`.
3. Rebuild to confirm it succeeds.

This hash is the same across `x86_64-linux`, `aarch64-linux`, `x86_64-darwin`,
and `aarch64-darwin` — `fetchPnpmDeps` fetches every optional dependency
listed in the lockfile (all os/cpu native-binary variants), not just the ones
matching the host, so there's no need for a per-system hash.

Do not remove the `pnpmForFetch` (`nodejs-slim_26`) override in `flake.nix`.
It works around a real, reproducible V8/libc++ ODR-violation bug in
nixpkgs' default Node 24
([NixOS/nixpkgs#536039](https://github.com/NixOS/nixpkgs/issues/536039))
that can hard-kill `pnpm install` mid-fetch under load. Confirmed locally:
removing the override lets the fd-tracking warnings spam through, and the
fetch gets `Killed: 9` under load; restoring it fixes it.

## Frontend tooling

Linting/formatting use [Oxlint](https://oxc.rs/docs/guide/usage/linter.html)
and [Oxfmt](https://oxc.rs/docs/guide/usage/formatter.html), not
ESLint/Prettier:

```sh
pnpm --dir frontend lint        # oxlint
pnpm --dir frontend fmt         # oxfmt (writes)
pnpm --dir frontend fmt:check   # oxfmt --check
```

## CI

`.github/workflows/ci.yml` uses `cachix/install-nix-action` (not
DeterminateSystems' installer/FlakeHub — keep this on plain, non-proprietary
Nix tooling) and runs `nix flake check` + `nix build .#stahl` on Linux and
macOS. It reads from the public `cache.nixos.org` and
`nix-community.cachix.org` substituters already declared in `flake.nix`'s
`nixConfig`; there is no project-specific push cache configured.
