// Settle two spends on chain: one frame transaction whose sender is the pool,
//   frame 0  VERIFY  pool       policy check, APPROVE(EXECUTION)
//   frame 1  VERIFY  paymaster  operator authorization, APPROVE(PAYMENT)
//   frame 2  SENDER  pool       settle(proof, A, B)
// signed by the paymaster's operator (leanSPHINCS witness or ECDSA, per the deployment).
import { readFileSync } from "node:fs";

import type { SettleLimits } from "../sdk/src/index.ts";
import { bigintOption, cli, loadPool, lskSigner, main, mempoolClient, readEcdsaKey, readJson, rpc, settings, usageError } from "./lib/common.ts";
import { SETTLE_VARIANTS, settle, type SettleVariant } from "./lib/pool-ops.ts";

const USAGE = `
Usage: node scripts/settle.ts --calldata FILE --fields FILE [options]

Builds the settle frame transaction for two spends, signs it as the paymaster's operator,
simulates it (ethrex_simulateFrameTransaction) and, with --send, broadcasts it.

Options:
  --calldata FILE         Terminal proof calldata (calldata.bin from prove-exit.sh)
  --fields FILE           JSON {"fields": [A, B]}: both spends' 82 public settle fields
  --deployment FILE       Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  --key FILE              Operator key: leanSPHINCS key file for a PQ paymaster (default: EXIT_PQ_KEY),
                          ECDSA key file otherwise (default: OPERATOR_KEY, then DEPLOYER_KEY)
  --variant NAME          ${SETTLE_VARIANTS.join(" | ")} (default: valid);
                          the others are negative cases for policy checks
  --pool-gas N            Frame 0 execution limit (default: 10000)
  --paymaster-gas N       Frame 1 execution limit (default: 150000 PQ, 60000 ECDSA)
  --paymaster-state N     Frame 1 state budget (default: 110000)
  --settle-gas N          Frame 2 execution limit (default: 14600000)
  --send                  Broadcast after simulating (otherwise simulate only)
  --mempool URL           Submit through this alternative mempool (default: MEMPOOL_URL; unset: the node)
  -h, --help              Show this help
`;

main(async () => {
  const { values } = cli(USAGE, {
    calldata: { type: "string" },
    fields: { type: "string" },
    deployment: { type: "string" },
    key: { type: "string" },
    variant: { type: "string", default: "valid" },
    "pool-gas": { type: "string" },
    "paymaster-gas": { type: "string" },
    "paymaster-state": { type: "string" },
    "settle-gas": { type: "string" },
    send: { type: "boolean", default: false },
    mempool: { type: "string" },
  });
  if (!values.calldata || !values.fields) usageError(USAGE, "--calldata and --fields are required");
  if (!(SETTLE_VARIANTS as readonly string[]).includes(values.variant)) usageError(USAGE, `unknown variant ${values.variant}`);
  const deployment = loadPool(values.deployment);
  const pq = deployment.pqPaymaster === true;
  const signer = pq
    ? { pqSigner: lskSigner(values.key ?? settings.exitPqKey()) }
    : { ecdsaKey: readEcdsaKey(values.key ?? settings.operatorKey()) };
  const { fields } = readJson<{ fields: [number[], number[]] }>(values.fields);
  if (!Array.isArray(fields) || fields.length !== 2) throw new Error(`${values.fields}: expected {"fields": [A, B]}`);
  const limits: Partial<SettleLimits> = {};
  if (values["pool-gas"]) limits.poolGas = bigintOption(values["pool-gas"], 0n, "--pool-gas");
  if (values["paymaster-gas"]) limits.paymasterGas = bigintOption(values["paymaster-gas"], 0n, "--paymaster-gas");
  if (values["paymaster-state"]) limits.paymasterState = bigintOption(values["paymaster-state"], 0n, "--paymaster-state");
  if (values["settle-gas"]) limits.settleGas = bigintOption(values["settle-gas"], 0n, "--settle-gas");
  const result = await settle({
    rpc: rpc(),
    deployment,
    signer,
    proofCall: new Uint8Array(readFileSync(values.calldata)),
    fields,
    limits,
    variant: values.variant as SettleVariant,
    send: values.send,
    mempool: mempoolClient(values.mempool),
  });
  if (result.receipt && result.receipt.status !== "0x1") process.exit(1);
});
