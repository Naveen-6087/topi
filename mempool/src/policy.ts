// The topi privacy-pool mempool's rules as an ERC-8421 sidecar admission policy.
//
// The alt mempool sidecar (forshtat/mempool-workshop-sidecar, with altmempool/sidecar.patch) loads
// this module through its `admissionPolicy` setting and calls `check` after its own structural,
// signature and simulation checks, at admission and again on every block. The rules are the ones
// RULES.md publishes (this mempool's id is that document's IPFS hash): the settle's exact shape,
// unchanged pool and paymaster code, unspent and not-already-pending nullifiers, known roots, and a
// leanSPHINCS witness that verifies under the exit key.
//
// Settings (environment, or .env through the SDK's config): POOL_DEPLOYMENT, MEMPOOL_PAYMASTERS,
// MEMPOOL_MAX_VALIDATION_GAS.
import { join } from "node:path";

import { RpcClient, fromHex, loadConfig, loadPoolDeployment } from "../../sdk/src/index.ts";

import { RpcChain } from "./chain.ts";
import { Mempool } from "./mempool.ts";
import { policyFor } from "./rules.ts";

export interface PolicyContext {
  rpcEndpoint: string;
  chainId: number;
  log: (line: string) => void;
}

/** How often to look for a new block and drop settles it made invalid. */
const REVALIDATE_MS = 4_000;

export default async function createPolicy(context: PolicyContext) {
  const root = join(import.meta.dirname, "..", "..");
  const config = loadConfig(root);
  const deployment = loadPoolDeployment(config.path("POOL_DEPLOYMENT", "deployments/8141/pool.json")!);
  if (BigInt(deployment.chainId) !== BigInt(context.chainId)) {
    throw new Error(`deployment is for chain ${deployment.chainId}, this mempool serves chain ${context.chainId}`);
  }
  const paymasters = (config.get("MEMPOOL_PAYMASTERS") ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((a) => BigInt(a));
  const maxValidationGas = BigInt(config.get("MEMPOOL_MAX_VALIDATION_GAS", "200000")!);
  const rules = policyFor(deployment, paymasters, maxValidationGas);
  const chain = new RpcChain(new RpcClient(context.rpcEndpoint));
  // The sidecar has just simulated the transaction; this keeps the pool-specific state (pending
  // nullifiers) and its caps.
  const mempool = new Mempool({ policy: rules, chain, simulate: false });
  await mempool.start();
  mempool.on("dropped", (e, reason) => context.log(`released ${e.hash}: ${reason}`));

  let lastBlock = -1n;
  const timer = setInterval(async () => {
    try {
      const block = await chain.blockNumber();
      if (block !== lastBlock) {
        lastBlock = block;
        await mempool.revalidate();
      }
    } catch (e) {
      context.log(`revalidation: ${e instanceof Error ? e.message : e}`);
    }
  }, REVALIDATE_MS);
  timer.unref?.();

  const r = mempool.rules();
  context.log(`pool ${r.pool}, paymasters ${r.paymasters.join(", ")}, validation budget ${r.maxValidationGas}`);
  return {
    name: "topi-privacy-pool",
    /** Throws `<rule>: <reason>` (a Rejection) when the settle breaks a rule; a re-check passes. */
    async check(_tx: unknown, rawTx: string): Promise<void> {
      await mempool.admit(fromHex(rawTx), "sidecar");
    },
  };
}
