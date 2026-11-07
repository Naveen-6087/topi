#!/usr/bin/env bash
# Runs the topi privacy-pool mempool on the ERC-8421 sidecar built by setup.sh. Linux or WSL.
#
#   bash mempool/sidecar/start.sh [--port N] [--p2p] [--p2p-port N] [--join API_PORT | --bootnode ENR]
#
# Serves eth_sendFrameTransaction, skandha_frameTransactionStatus and skandha_explorer_* on
# http://127.0.0.1:PORT/rpc (default MEMPOOL_PORT or 47500), admits settles by mempool/RULES.md
# (mempool id: its IPFS hash) and relays them to RPC_URL with eth_sendRawTransaction. With --p2p it
# gossips with other sidecars of the same mempool over libp2p and saves its ENR (data-PORT/node/enr);
# --join API_PORT starts a second node that dials the local node serving API_PORT.
#
# Settings (.env): RPC_URL, POOL_DEPLOYMENT, MEMPOOL_PAYMASTERS, MEMPOOL_PORT, MEMPOOL_MAX_VALIDATION_GAS;
# SIDECAR_HOME and BUN as for setup.sh.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
DIR=${SIDECAR_HOME:-$HOME/.local/share/topi-altmempool}
BUN=${BUN:-$(command -v bun || echo "$HOME/.bun/bin/bun")}
[ -f "$DIR/packages/cli/bin/skandha.js" ] || { echo "sidecar not built: bash mempool/sidecar/setup.sh" >&2; exit 1; }
if [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1090,SC1091
  . <(tr -d '\r' <"$ROOT/.env")
  set +a
fi

PORT=${MEMPOOL_PORT:-47500}
P2P=false
BOOTNODE=""
P2P_PORT=4337
while [ $# -gt 0 ]; do
  case $1 in
    --port) PORT=$2; shift 2 ;;
    --p2p) P2P=true; shift ;;
    --bootnode) BOOTNODE=$2; P2P=true; shift 2 ;;
    --join)
      ENR_FILE=$DIR/data-$2/node/enr
      for _ in $(seq 1 60); do [ -s "$ENR_FILE" ] && break; sleep 1; done
      [ -s "$ENR_FILE" ] || { echo "no node on API port $2 to join (no $ENR_FILE)" >&2; exit 1; }
      BOOTNODE=$(tr -d '\n' <"$ENR_FILE"); P2P=true; shift 2 ;;
    --p2p-port) P2P_PORT=$2; shift 2 ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

RPC=${RPC_URL:-https://rpc1.privacy.ethrex.xyz}
DEPLOYMENT=${POOL_DEPLOYMENT:-deployments/8141/pool.json}
case $DEPLOYMENT in /*) ;; *) DEPLOYMENT=$ROOT/$DEPLOYMENT ;; esac
export POOL_DEPLOYMENT=$DEPLOYMENT
MEMPOOL_ID=$("$BUN" "$ROOT/mempool/src/mempool-id.ts")
DATA=$DIR/data-$PORT
mkdir -p "$DATA"
CONFIG=$DATA/config.json

# The pool account and the paymasters are this mempool's whitelisted entities (RULES.md, Entities).
"$BUN" -e '
  const [deploymentFile, extra, rpc, id, policy, out] = process.argv.slice(1);
  const fs = require("fs");
  const d = JSON.parse(fs.readFileSync(deploymentFile, "utf8"));
  // The ERC-8421 staking registry of the chain, when one is recorded next to the deployment.
  const stakingFile = require("path").join(require("path").dirname(deploymentFile), "staking.json");
  const registry = fs.existsSync(stakingFile) ? JSON.parse(fs.readFileSync(stakingFile, "utf8")).stakingRegistry : undefined;
  const paymasters = [d.paymaster, ...extra.split(",").map((s) => s.trim()).filter(Boolean)];
  const config = {
    entryPoints: ["0x00000000000000000000000000000000000000aa"],
    canonicalEntryPoint: "0x00000000000000000000000000000000000000aa",
    relayers: [],
    rpcEndpoint: rpc,
    rpcEndpointSubmitMethod: "eth_sendRawTransaction",
    standardMempoolId: id,
    supportedMempools: [{ mempoolId: id, entryPoint: "0x00000000000000000000000000000000000000aa" }],
    eip8272: d.chainId === 1337,
    traceSimulation: false,
    maxVerifyGas: Number(process.env.MEMPOOL_MAX_VALIDATION_GAS || 200000),
    admissionPolicy: policy,
    whitelistedEntities: { account: [d.pool], paymaster: paymasters, factory: [], external: [] },
    userOpGasLimit: 30000000,
    ...(registry ? { stakingRegistryAddress: registry } : {}),
  };
  require("fs").writeFileSync(out, JSON.stringify(config, null, 2));
' "$DEPLOYMENT" "${MEMPOOL_PAYMASTERS:-}" "$RPC" "$MEMPOOL_ID" "$ROOT/mempool/src/policy.ts" "$CONFIG"

echo "topi privacy-pool mempool $MEMPOOL_ID on http://127.0.0.1:$PORT/rpc (chain $RPC)"
cd "$DIR"
# --api.wsPort does not follow --api.port; sharing the port keeps two nodes on one machine apart.
ARGS=(--configFile "$CONFIG" --api.port "$PORT" --api.wsPort "$PORT" --api.address 127.0.0.1 --redirectRpc
  --executor.bundlingMode auto --explorer.enable)
if $P2P; then
  ARGS+=(--dataDir "$DATA/node" --p2p.host 127.0.0.1 --p2p.enrHost 127.0.0.1 --p2p.port "$P2P_PORT"
    --p2p.enrPort "$P2P_PORT" --p2p.retainPeerId)
  [ -n "$BOOTNODE" ] && ARGS+=(--p2p.bootEnrs "$BOOTNODE")
  echo "gossip on 127.0.0.1:$P2P_PORT${BOOTNODE:+, joining ${BOOTNODE:0:24}...}"
  exec "$BUN" --bun packages/cli/bin/skandha.js node "${ARGS[@]}"
fi
exec "$BUN" --bun packages/cli/bin/skandha.js standalone --dataDir "$DATA/db" "${ARGS[@]}"
