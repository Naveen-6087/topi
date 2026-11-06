#!/usr/bin/env bash
# Clone the upstream provers at their pinned commits, install this repository's wallet circuit into
# spartan-whir, and build the wallet and proving binaries. Safe to re-run.
# Needs: git, a Rust toolchain, python3, Foundry (for the exit proof's Solidity export).
set -euo pipefail
. "$(dirname "$0")/env.sh"

fetch() {
  local dir=$1 url=$2 rev=$3
  if [ ! -d "$PROVER_HOME/$dir/.git" ]; then
    git clone --quiet "$url" "$PROVER_HOME/$dir"
  fi
  git -C "$PROVER_HOME/$dir" fetch --quiet origin "$rev" 2>/dev/null || true
  git -C "$PROVER_HOME/$dir" checkout --quiet "$rev"
  git -C "$PROVER_HOME/$dir" submodule update --init --recursive --quiet
}

mkdir -p "$PROVER_HOME"
fetch spartan-whir "$SPARTAN_WHIR_URL" "$SPARTAN_WHIR_REV"
fetch leanVM "$LEANVM_URL" "$LEANVM_REV"
fetch sol-whir-p3 "$SOL_WHIR_URL" "$SOL_WHIR_REV"

cp -r "$ROOT/prover/patches/spartan-whir/." "$PROVER_HOME/spartan-whir/"
# sol-whir-p3's terminal fixture codec imports terminal_hashes from this fixed place under PROVER_HOME.
mkdir -p "$PROVER_HOME/research/2026-09-09-withdrawal-chain/terminal"
cp "$ROOT/prover/support/terminal_hashes.py" "$PROVER_HOME/research/2026-09-09-withdrawal-chain/terminal/"
grep -q "^pub mod pool_wallet;" "$PROVER_HOME/spartan-whir/src/lib.rs" || echo "pub mod pool_wallet;" >>"$PROVER_HOME/spartan-whir/src/lib.rs"

(cd "$PROVER_HOME/spartan-whir" && cargo build --release --quiet --features parallel,poseidon1 --bin pool-wallet --bin leanvm-full-zk-spark-fixture)
echo "prover ready in $PROVER_HOME"
