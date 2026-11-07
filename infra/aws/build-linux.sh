#!/usr/bin/env bash
# Build the node binary the AWS instances run (x86_64 Linux, glibc 2.39 = the Ubuntu 24.04 image).
# On Windows run it inside WSL. Output: data/aws/bin/topi-node.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
[ -f "$ROOT/.env" ] && { set -a; . "$ROOT/.env"; set +a; }
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

export CARGO_TARGET_DIR=${LINUX_TARGET_DIR:-$ROOT/target/linux}
# A portable CPU target: every t3 instance (Skylake-SP and later) has x86-64-v3, AES and CLMUL.
export RUSTFLAGS="-C target-cpu=x86-64-v3 -C target-feature=+aes,+pclmulqdq"
if grep -q "^\[workspace\]" "$ROOT/Cargo.toml" 2>/dev/null; then
  cargo build --release --locked --manifest-path "$ROOT/Cargo.toml" -p topi-mixnet --bin topi-node
else
  # No source here (the console's unpacked assets): build the node from the repository.
  cargo install --locked --git "${TOPI_GIT:-https://github.com/Naveen-6087/topi}" topi-mixnet --bin topi-node     --root "$CARGO_TARGET_DIR/git" --force
  mkdir -p "$CARGO_TARGET_DIR/release" && cp "$CARGO_TARGET_DIR/git/bin/topi-node" "$CARGO_TARGET_DIR/release/"
fi

OUT=$ROOT/data/aws/bin
mkdir -p "$OUT"
cp "$CARGO_TARGET_DIR/release/topi-node" "$OUT/"
strip "$OUT/topi-node"
ls -la "$OUT/topi-node"
