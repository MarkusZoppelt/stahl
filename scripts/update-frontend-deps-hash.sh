#!/usr/bin/env bash
# Regenerate flake.nix frontendDepsHash from frontend/pnpm-lock.yaml.
set -euo pipefail
cd "$(dirname "$0")/.."

target=".#frontend-deps"
if ! nix eval "$target.name" --raw >/dev/null 2>&1; then
  sys=$(nix eval --impure --raw --expr 'builtins.currentSystem')
  target=".#checks.${sys}.stahl-frontend-lint"
fi

sed -i -E 's|(frontendDepsHash = ")sha256-[^"]+(")|\1sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\2|' flake.nix

log=$(mktemp)
trap 'rm -f "$log"' EXIT
nix build "$target" --no-link 2>&1 | tee "$log" || true

got=$(sed -nE 's/.*got:[[:space:]]+(sha256-[A-Za-z0-9+/=]+).*/\1/p' "$log" | tail -1)
[[ -n "$got" ]] || { echo "error: could not parse hash mismatch output" >&2; exit 1; }

sed -i -E "s|(frontendDepsHash = \")sha256-[^\"]+(\")|\1${got}\2|" flake.nix
echo "frontendDepsHash = \"$got\";"
