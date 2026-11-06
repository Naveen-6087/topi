// topi alternative mempool for settle transactions.
//
//   node mempool/src/main.ts [--port 47500] [--rpc URL] [--deployment FILE] [--peers URL,URL]
//                            [--paymasters ADDR,ADDR] [--no-release] [--no-simulate]
import { parseArgs } from "node:util";
import { join } from "node:path";

import { RpcClient, loadConfig, loadPoolDeployment } from "../../sdk/src/index.ts";

import { RpcChain } from "./chain.ts";
import { Mempool } from "./mempool.ts";
import { Releaser } from "./releaser.ts";
import { policyFor } from "./rules.ts";
import { Gossip, serve } from "./server.ts";

const USAGE = `
Usage: node mempool/src/main.ts [options]

An alternative mempool for the pool's settle transactions: admits settles by the pool's own rules,
holds many at once from the shared pool sender, gossips them to peers and releases them to the chain
one at a time.

Options:
  --port N               JSON-RPC port (default: MEMPOOL_PORT or 47500)
  --host ADDR            Address to listen on (default: 127.0.0.1)
  --id URL               This node's URL as peers reach it (default: http://<host>:<port>)
  --rpc URL              The chain's RPC (default: RPC_URL or the chain 8141 testnet)
  --deployment FILE      Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  --peers URL,URL        Peer mempools to gossip with (default: MEMPOOL_PEERS)
  --paymasters ADDR,...  Paymasters accepted besides the deployment's (default: MEMPOOL_PAYMASTERS)
  --max-validation-gas N Largest frame 0 + frame 1 budget admitted (default: 200000)
  --no-release           Hold and gossip only; let a peer hand settles to the chain
  --no-simulate          Skip ethrex_simulateFrameTransaction on admission
  -h, --help             Show this help
`;

const ROOT = join(import.meta.dirname, "..", "..");
const config = loadConfig(ROOT);
const { values } = parseArgs({
  options: {
    port: { type: "string" },
    host: { type: "string", default: "127.0.0.1" },
    id: { type: "string" },
    rpc: { type: "string" },
    deployment: { type: "string" },
    peers: { type: "string" },
    paymasters: { type: "string" },
    "max-validation-gas": { type: "string" },
    "no-release": { type: "boolean", default: false },
    "no-simulate": { type: "boolean", default: false },
    help: { type: "boolean", short: "h", default: false },
  },
});
if (values.help) {
  console.log(USAGE.trim());
  process.exit(0);
}

const list = (v: string | undefined) => (v ?? "").split(",").map((s) => s.trim()).filter(Boolean);
const port = Number(values.port ?? config.get("MEMPOOL_PORT", "47500"));
const host = values.host!;
const id = values.id ?? `http://${host}:${port}`;
const rpcUrl = values.rpc ?? config.get("RPC_URL", "https://rpc1.privacy.ethrex.xyz")!;
const deployment = loadPoolDeployment(values.deployment ?? config.path("POOL_DEPLOYMENT", "deployments/8141/pool.json")!);
const peers = list(values.peers ?? config.get("MEMPOOL_PEERS")).filter((p) => p !== id);
const paymasters = list(values.paymasters ?? config.get("MEMPOOL_PAYMASTERS")).map((a) => BigInt(a));
const maxValidationGas = BigInt(values["max-validation-gas"] ?? "200000");

const log = (line: string) => console.log(`[mempool] ${line}`);
const chain = new RpcChain(new RpcClient(rpcUrl));
const mempool = new Mempool({ policy: policyFor(deployment, paymasters, maxValidationGas), chain, simulate: !values["no-simulate"] });
await mempool.start();
mempool.on("admitted", (e) => log(`admitted ${e.hash} from ${e.source}`));
mempool.on("dropped", (e, reason) => log(`dropped ${e.hash}: ${reason}`));
const gossip = new Gossip(mempool, id, peers, log);
await serve(mempool, gossip, port, host);
new Releaser(mempool, { log, release: !values["no-release"] }).start();

const rules = mempool.rules();
log(`listening on ${id} (chain ${rpcUrl}); pool ${rules.pool}, paymasters ${rules.paymasters.join(", ")}`);
log(`peers: ${peers.length ? peers.join(", ") : "none"}; ${values["no-release"] ? "holding only" : "releasing to the chain"}`);
