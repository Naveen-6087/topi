#!/usr/bin/env bash
# Wallet side: prove one withdrawal (full zero knowledge, lean statement) and pack the client proof.
# Usage: prove-client.sh INPUT.json OUT.tgz   (INPUT from `pool-wallet witness`)
set -euo pipefail
. "$(dirname "$0")/env.sh"
INPUT=$1
OUT=$2
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

timed "wallet proof" "$FIXTURE_BIN" "$WORK/proof" --circuit privacy-pools-lean --input "$INPUT" 2>&1 | tail -2
mkdir -p "$(dirname "$OUT")"
tar -czf "$OUT" -C "$WORK/proof" .
echo "client proof: $(stat -c %s "$OUT") bytes -> $OUT"
