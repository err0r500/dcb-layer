#!/usr/bin/env bash
# Bump the shared (lockstep) version across all manifests and create the release
# tag. One tag `vX.Y.Z` drives the whole pipeline in .github/workflows/publish.yml:
# publish crate -> build precompiled NIFs -> publish Elixir package.
#
# Usage: bin/release.sh 0.2.3
set -euo pipefail

VERSION="${1:?usage: bin/release.sh X.Y.Z}"
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be X.Y.Z, got '$VERSION'" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_TOML="$ROOT/dcb-layer/Cargo.toml"
NIF_TOML="$ROOT/dcb-layer-ex/native/dcb_layer_nif/Cargo.toml"
MIX="$ROOT/dcb-layer-ex/mix.exs"

# dcb-layer crate version (first `version = "..."` under [package])
perl -0pi -e 's/^(version = ")[^"]+(")/${1}'"$VERSION"'${2}/m' "$CRATE_TOML"
# NIF dependency on the crate: `dcb-layer = "X.Y.Z"`
perl -pi -e 's/^(dcb-layer = ")[^"]+(")/${1}'"$VERSION"'${2}/' "$NIF_TOML"
# Elixir @version
perl -pi -e 's/^(  \@version ")[^"]+(")/${1}'"$VERSION"'${2}/' "$MIX"

echo "Bumped to $VERSION:"
grep -m1 '^version' "$CRATE_TOML"
grep '^dcb-layer = ' "$NIF_TOML"
grep '@version' "$MIX"

echo
echo "Review the diff, commit, then:"
echo "  git tag v$VERSION && git push origin v$VERSION"
