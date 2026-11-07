#!/usr/bin/env bash
# Deploy a shielded pool: the ShieldedPool logic contract, the pool account (PoolAccount) and a
# paymaster (PQPaymaster with a leanSPHINCS operator key, or ClassicPaymaster with the ECDSA
# operator), then fund the paymaster. Writes a deployment record under deployments/<chain>/.
# This sends transactions; it needs forge, cast and node on PATH.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/deploy.sh NAME [options]

Options:
  --pq-key FILE      leanSPHINCS operator key file: deploy PQPaymaster (default: ClassicPaymaster)
  --testbed          Deploy a testbed pool (roots can be seeded; never for real funds)
  --terminal ADDR    Terminal proof verifier (default: "terminal" of deployments/<chain>/pool.json)
  --fund ETH         Paymaster funding (default: 0.1)
  --out FILE         Deployment record (default: deployments/<chain>/pool-NAME.json)
  -h, --help         Show this help

Settings (.env): RPC_URL, DEPLOYER_KEY (ECDSA key file; its address becomes the operator),
LSK_BIN (for --pq-key); optional FOUNDRY_BIN (directory) and NODE_BIN to pick the tools.
Pool and scope identities: eight 32-bit words of keccak256("pq-pool:<chain>:<NAME>:pool|scope"),
each reduced mod the KoalaBear prime.
EOF
}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$ROOT/.env"
  set +a
fi
if [ -n "${FOUNDRY_BIN:-}" ]; then export PATH="$FOUNDRY_BIN:$PATH"; fi
if [ -n "${NODE_BIN:-}" ]; then PATH="$(dirname "$NODE_BIN"):$PATH"; export PATH; fi

NAME=""
PQKEY=""
TESTBED=false
TERMINAL=""
FUND=0.1
OUT=""
while [ $# -gt 0 ]; do
  case $1 in
    -h | --help) usage; exit 0 ;;
    --pq-key) PQKEY=$2; shift 2 ;;
    --testbed) TESTBED=true; shift ;;
    --terminal) TERMINAL=$2; shift 2 ;;
    --fund) FUND=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    -*) echo "unknown option $1" >&2; usage >&2; exit 2 ;;
    *) [ -z "$NAME" ] || { usage >&2; exit 2; }; NAME=$1; shift ;;
  esac
done
[ -n "$NAME" ] || { usage >&2; exit 2; }
[[ $NAME =~ ^[A-Za-z0-9_-]+$ ]] || { echo "NAME: letters, digits, - and _ only" >&2; exit 2; }

RPC=${RPC_URL:-https://rpc1.privacy.ethrex.xyz}
json_field() { node -p "JSON.parse(require('fs').readFileSync(0, 'utf8')).$1"; }
abs() { case $1 in /* | [A-Za-z]:*) echo "$1" ;; *) echo "$ROOT/$1" ;; esac; }
KEYFILE=$(abs "${DEPLOYER_KEY:-.secrets/deployer.key}")
[ -f "$KEYFILE" ] || { echo "deployer key not found: DEPLOYER_KEY" >&2; exit 1; }
CHAIN=$(cast chain-id --rpc-url "$RPC")
DEPLOYMENTS=$ROOT/deployments/$CHAIN
OUT=${OUT:-$DEPLOYMENTS/pool-$NAME.json}
[ -e "$OUT" ] && { echo "refusing to overwrite $OUT" >&2; exit 1; }
if [ -z "$TERMINAL" ]; then
  TERMINAL=$(json_field terminal <"$DEPLOYMENTS/pool.json")
fi
YUL=$ROOT/contracts/yul/build
for f in PoolAccount.bin PQPaymaster.bin ClassicPaymaster.bin; do
  [ -f "$YUL/$f" ] || { echo "missing $YUL/$f (build the Yul accounts first)" >&2; exit 1; }
done

key() { cat "$KEYFILE"; }
OPERATOR=$(cast wallet address --private-key "$(key)")
pad() { printf "%064s" "${1#0x}" | tr ' ' 0; }
created() { grep -oE '"contractAddress":"0x[0-9a-fA-F]+"' | cut -d'"' -f4; }
identity() {
  node -e 'const h = process.argv[1].slice(2); const p = 2130706433n; const out = [];
    for (let i = 0; i < 8; i++) out.push((BigInt("0x" + h.slice(8 * i, 8 * i + 8)) % p).toString());
    console.log("[" + out.join(",") + "]")' "$(cast keccak "$1")"
}
POOLID=$(identity "pq-pool:$CHAIN:$NAME:pool")
SCOPE=$(identity "pq-pool:$CHAIN:$NAME:scope")

echo "deploying pool $NAME on chain $CHAIN (operator $OPERATOR, terminal $TERMINAL, testbed $TESTBED)"
cd "$ROOT/contracts"
LOGIC=$(forge create src/ShieldedPool.sol:ShieldedPool --rpc-url "$RPC" --private-key "$(key)" --broadcast --gas-limit 120000000 \
  --constructor-args "$TERMINAL" "$OPERATOR" "$TESTBED" "$POOLID" "$SCOPE" 2>&1 | grep -oE "Deployed to: 0x[0-9a-fA-F]+" | cut -d' ' -f3)
[ -n "$LOGIC" ] || { echo "logic deployment failed" >&2; exit 1; }
POOL=$(cast send --rpc-url "$RPC" --private-key "$(key)" --gas-limit 3000000 --json \
  --create "0x$(tr -d '\r\n' <"$YUL/PoolAccount.bin")$(pad "$LOGIC")" | created)

if [ -n "$PQKEY" ]; then
  VERIFIER=$(json_field verifier <"$DEPLOYMENTS/account.json")
  PK=$("${LSK_BIN:-lsk}" info "$PQKEY" | node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
    const k = JSON.parse(s); console.log(k.pkSeed.slice(2) + " " + k.pkRoot.slice(2)); })')
  PKSEED=${PK% *}
  PKROOT=${PK#* }
  INIT="0x$(tr -d '\r\n' <"$YUL/PQPaymaster.bin")$(pad "$POOL")$(pad "$OPERATOR")$(pad "$VERIFIER")$PKSEED$PKROOT"
else
  INIT="0x$(tr -d '\r\n' <"$YUL/ClassicPaymaster.bin")$(pad "$POOL")$(pad "$OPERATOR")"
fi
PAYMASTER=$(cast send --rpc-url "$RPC" --private-key "$(key)" --gas-limit 3000000 --json --create "$INIT" | created)
cast send --rpc-url "$RPC" --private-key "$(key)" "$PAYMASTER" --value "${FUND}ether" --json >/dev/null

mkdir -p "$(dirname "$OUT")"
if [ -n "$PQKEY" ]; then
  EXTRA=$(printf ',"pqPaymaster":true,"exitPkSeed":"0x%s","exitPkRoot":"0x%s","verifier":"%s"' "$PKSEED" "$PKROOT" "$VERIFIER")
else
  EXTRA=""
fi
printf '{"chainId":%s,"name":"%s","terminal":"%s","logic":"%s","pool":"%s","paymaster":"%s","operator":"%s","testbed":%s,"poolId":%s,"scope":%s%s}\n' \
  "$CHAIN" "$NAME" "$TERMINAL" "$LOGIC" "$POOL" "$PAYMASTER" "$OPERATOR" "$TESTBED" "$POOLID" "$SCOPE" "$EXTRA" >"$OUT"
cat "$OUT"
echo "initialized=$(cast call "$POOL" 'initialized()(bool)' --rpc-url "$RPC") testbed=$(cast call "$POOL" 'testbed()(bool)' --rpc-url "$RPC")" \
  "paymaster balance=$(cast balance "$PAYMASTER" --rpc-url "$RPC" --ether)"
