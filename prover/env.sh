# shellcheck shell=bash disable=SC2034 # variables here are used by the scripts that source this file
# Shared settings for the prover scripts (Linux or WSL). Source it; do not run it.
#   PROVER_HOME  where the upstream provers are cloned and built (default: ~/.local/share/pq-prover)
#   PROVER_THREADS  worker threads for proving (default: all cores)

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [ -f "$ROOT/.env" ]; then
  set -a
  . "$ROOT/.env"
  set +a
fi
PROVER_HOME=${PROVER_HOME:-$HOME/.local/share/pq-prover}

# Upstream provers, pinned to the commits the deployed terminal verifier was generated from.
SPARTAN_WHIR_URL=https://github.com/ethereum/spartan-whir
SPARTAN_WHIR_REV=14d6da4211eda59e65cfd0af5e2dcd89d523edc3
LEANVM_URL=https://github.com/Client-Side-Proving/leanVM
LEANVM_REV=fb0909d0dc3ab4cffd1910c065998effd44a5ee3
SOL_WHIR_URL=https://github.com/ethereum/sol-whir-p3
SOL_WHIR_REV=b82359ec98e1e3c9f6f39ebf984e7aefb0eeb9be

WALLET_BIN=$PROVER_HOME/spartan-whir/target/release/pool-wallet
FIXTURE_BIN=$PROVER_HOME/spartan-whir/target/release/leanvm-full-zk-spark-fixture
# Program hashes of the terminal verifier deployed on chain; every exit proof must match them.
TERMINAL_MANIFEST=$ROOT/contracts/test/fixtures/terminal/manifest.json

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export RAYON_NUM_THREADS=${PROVER_THREADS:-$(nproc)}
export SPARK_FRESH_MASK_BATCHING=same_height SPARK_MASK_PACKING=all_free_basis
export RUSTFLAGS=${RUSTFLAGS:-"-C target-cpu=native -C debuginfo=0"}

timed() {
  local label=$1
  shift
  if [ -x /usr/bin/time ]; then
    /usr/bin/time -f "$label: %e s wall, %M KB peak" "$@"
  else
    "$@"
  fi
}
