#!/usr/bin/env bash
# Exit side: two client proofs -> leanVM recursion -> grouped Keccak terminal proof -> calldata for the
# deployed terminal verifier (verifyGroupedLogupV1). Needs about 12 GB of memory and 12-20 minutes.
# Usage: prove-exit.sh A.tgz B.tgz OUT_DIR   (writes OUT_DIR/calldata.bin)
set -euo pipefail
. "$(dirname "$0")/env.sh"
A=$1
B=$2
OUT=$3
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/a" "$WORK/b" "$OUT"
tar -xzf "$A" -C "$WORK/a"
tar -xzf "$B" -C "$WORK/b"

(cd "$PROVER_HOME/leanVM" &&
  timed "recursion" python3 script/run_terminal_fixture.py "$WORK/a" "$WORK/b" "$WORK/terminal" \
    crates/spartan_whir_guest/profiles/terminal_grouped.json 2>&1 | tail -2)

LABEL="GroupedLogupKeccakPow28T6Exit$(date +%s)$$"
cd "$PROVER_HOME/sol-whir-p3"
python3 script/generate_leanvm_grouped_terminal_fixture.py "$WORK/terminal/native/root-fixture.json" "$LABEL" \
  --require-current-source >/dev/null
GEN=testdata/leanvm_terminal/$LABEL
cp "$GEN/calldata.bin" "$OUT/calldata.bin"

# The proof must be for the same guest programs as the deployed terminal verifier.
python3 - "$GEN/manifest.json" "$TERMINAL_MANIFEST" <<'PY'
import json, sys
new, deployed = (json.load(open(p)) for p in sys.argv[1:])
same = all(a["mle_sha256"] == b["mle_sha256"] for a, b in zip(new["programs"], deployed["programs"]))
print("programs match the deployed terminal:", same)
sys.exit(0 if same else 3)
PY

# The per-run Solidity export is not needed: the deployed verifier checks this proof.
SUFFIX=${LABEL#GroupedLogup}
rm -rf "$GEN" "src/leanvm/generated/LeanVmGroupedTerminal_$SUFFIX.sol" "test/LeanVmGroupedTerminal_$SUFFIX.t.sol" \
  "script/LeanVmGroupedTerminal_$SUFFIX.s.sol"
echo "calldata: $OUT/calldata.bin"
