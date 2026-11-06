#!/usr/bin/env bash
# Builds the ERC-8421 alternative mempool sidecar (forshtat/mempool-workshop-sidecar, the skandha
# fork for EIP-8141 frame transactions) with sidecar.patch, which lets it run the topi privacy-pool
# mempool: a configurable validation budget, untraced nodes, 8-field chains, standard relaying and a
# pluggable admission policy (mempool/src/policy.ts). Linux or WSL (its RocksDB has no Windows build).
#
#   bash mempool/sidecar/setup.sh       then   node mempool/sidecar/start.ts
#
# SIDECAR_HOME  where to build it (default: ~/.local/share/topi-altmempool)
# BUN           the bun binary (default: bun on PATH, else ~/.bun/bin/bun)
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
DIR=${SIDECAR_HOME:-$HOME/.local/share/topi-altmempool}
REPO=https://github.com/forshtat/mempool-workshop-sidecar.git
REV=2e22ff0a2898e69ea4bf5275bce1be8581a8582a
BUN=${BUN:-$(command -v bun || echo "$HOME/.bun/bin/bun")}
[ -x "$BUN" ] || { echo "bun not found: curl -fsSL https://bun.sh/install | bash" >&2; exit 1; }
export PATH="$(dirname "$BUN"):$PATH"   # its build scripts call bun by name

if [ ! -d "$DIR/.git" ]; then
  git clone --quiet "$REPO" "$DIR"
fi
cd "$DIR"
git fetch --quiet origin "$REV" 2>/dev/null || git fetch --quiet origin
git checkout --quiet --force "$REV"
git clean --quiet -fd packages
git apply --whitespace=nowarn "$HERE/sidecar.patch"
"$BUN" install --frozen-lockfile >/dev/null
"$BUN" run build >/dev/null
# The admission policy imports the SDK, which needs its npm dependencies (absent in the console's
# unpacked assets, which carry no node_modules).
ROOT=$(cd "$HERE/../.." && pwd)
if [ ! -d "$ROOT/node_modules/@noble/hashes" ] && [ ! -d "$ROOT/sdk/node_modules/@noble/hashes" ]; then
  (cd "$ROOT/sdk" && "$BUN" install --production >/dev/null)
fi
echo "sidecar ready in $DIR ($REV + sidecar.patch)"
