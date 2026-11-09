#!/usr/bin/env bash
# Moves Rune and rune-kit to one new revision everywhere they are pinned (the app's Cargo.toml and
# ios/rust's), and refreshes both lockfiles to match. With no argument it takes the tip of Rune's
# dev branch. Run from anywhere, in a shell with cargo:
#
#   nix develop -c scripts/update-rune.sh [rev]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
repo=https://github.com/anomalyco/rune
manifests=("$root/Cargo.toml" "$root/ios/rust/Cargo.toml")

new="${1:-$(git ls-remote "$repo" refs/heads/dev | cut -f1)}"
if ! [[ "$new" =~ ^[0-9a-f]{40}$ ]]; then
  echo "not a full commit hash: '$new'" >&2
  exit 2
fi

old="$(grep -oE "anomalyco/rune\", rev = \"[0-9a-f]{40}\"" "$root/Cargo.toml" | head -n1 | grep -oE '[0-9a-f]{40}')"
if [ "$old" = "$new" ]; then
  echo "Rune is already at $new"
  exit 0
fi

for manifest in "${manifests[@]}"; do
  perl -pi -e "s/\Q$old\E/$new/g" "$manifest"
  if grep -qE "anomalyco/rune\", rev = \"" "$manifest" && grep -E "anomalyco/rune\", rev = \"" "$manifest" | grep -vq "$new"; then
    echo "$manifest pins Rune at a revision other than $old; fix it by hand" >&2
    exit 1
  fi
done

# Resolving the workspace rewrites only what changed in the lockfile.
for manifest in "${manifests[@]}"; do
  cargo metadata --format-version 1 --manifest-path "$manifest" > /dev/null
done

echo "Rune: ${old:0:7} -> ${new:0:7}"
echo "$repo/compare/$old...$new"
