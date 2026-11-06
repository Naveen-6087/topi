// Exit prover: turns spends queued by the mixnet exit into settled pool transactions. For each
// pair of queued spends it
//   1. checks them cheaply: pool and scope are this pool's, the fee goes to this exit, both roots
//      are known, the nullifier is unspent, and the 82 fields hash to the proof's statement
//      (`pool-wallet check`), so the expensive recursion only runs on spends that can settle;
//   2. runs the leanVM recursion (prover/prove-exit.sh) to terminal proof calldata;
//   3. settles both on chain, signed as the paymaster's operator.
// The client proofs are zero-knowledge, so the exit learns nothing secret; the mixnet hid who sent them.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { PoolReader, Prover, ProverError, checkSpendFields, readSpendQueue } from "../sdk/src/index.ts";
import { cli, loadPool, lskSigner, main, mempoolClient, pipelineLog, readEcdsaKey, rel, rpc, settings } from "./lib/common.ts";
import { settle } from "./lib/pool-ops.ts";

const USAGE = `
Usage: node scripts/exit-prover.ts [options]

Checks the queued spends, proves the first ready pair (about 15 minutes, 12 GB) and settles it.

Options:
  --queue DIR             Spend queue (default: data/aws/spend-queue; the local network's is
                          data/local/spend-queue)
  --deployment FILE       Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  --key FILE              Operator key: leanSPHINCS key file for a PQ paymaster (default: EXIT_PQ_KEY),
                          ECDSA key file otherwise (default: OPERATOR_KEY, then DEPLOYER_KEY)
  --check-only            Check and sort the queue, then stop (no proving, no transaction)
  --no-send               Prove and simulate the settle, but do not broadcast it
  --mempool URL           Submit the settle through this alternative mempool (default: MEMPOOL_URL)
  --prover local|aws      Where the recursion runs: here, or the AWS prover machine (infra/aws/prover.sh up,
                          setup); default EXIT_PROVER or local
  -h, --help              Show this help

Rejected spends move to QUEUE/rejected, settled ones to QUEUE/done; each pair's proof and
settle log go to QUEUE/settle-A-B. Progress is appended to logs/pipeline.log.
`;

const log = (message: string) => {
  console.log(`[exit-prover] ${message}`);
  pipelineLog("exit-prover", message);
};

main(async () => {
  const { values } = cli(USAGE, {
    queue: { type: "string" },
    deployment: { type: "string" },
    key: { type: "string" },
    "check-only": { type: "boolean", default: false },
    "no-send": { type: "boolean", default: false },
    mempool: { type: "string" },
    prover: { type: "string" },
  });
  const queueDir = values.queue ?? settings.spendQueue("aws");
  const deployment = loadPool(values.deployment);
  const client = rpc();
  const pool = new PoolReader(client, deployment.pool);
  const prover = new Prover({ proverDir: settings.proverDir(), wslDistro: settings.wslDistro() });

  const pending = readSpendQueue(queueDir);
  if (pending.length > 0) log(`${pending.length} new spend(s) in ${rel(queueDir)}`);
  const ready: typeof pending = [];
  mkdirSync(join(queueDir, "rejected"), { recursive: true });
  for (const spend of pending) {
    const why =
      checkSpendFields(spend.fields, deployment) ??
      (await pool.checkSpendState(spend.fields)) ??
      ((await prover.checkSpend(spend.fieldsFile, spend.proofFile)) ? null : "fields do not match the proof");
    if (why) {
      log(`rejected ${spend.id}: ${why}`);
      renameSync(spend.fieldsFile, join(queueDir, "rejected", `${spend.id}.json`));
      continue;
    }
    log(`checked ${spend.id}: pool, fee, roots, nullifier and statement ok`);
    ready.push(spend);
  }
  if (ready.length < 2) {
    log(`${ready.length} spend(s) ready; waiting for a pair (the terminal proves two statements)`);
    return;
  }
  if (values["check-only"]) return;

  const proverOn = values.prover ?? process.env.EXIT_PROVER ?? "local";
  const [a, b] = ready;
  const out = join(queueDir, `settle-${a.id}-${b.id}`);
  mkdirSync(out, { recursive: true });
  log(`proving pair ${a.id} + ${b.id} (leanVM recursion)`);
  const started = Date.now();
  let calldata: string;
  try {
    calldata =
      proverOn === "aws"
        ? proveOnAws(a.proofFile, b.proofFile, out)
        : await new Prover({ proverDir: settings.proverDir(), wslDistro: settings.wslDistro(), echo: true }).proveExit(
            a.proofFile,
            b.proofFile,
            out,
          );
  } catch (error) {
    const detail = error instanceof ProverError ? error.result.stderr.slice(-500) : String(error);
    log(`recursion failed: ${detail}`);
    process.exit(1);
  }
  log(`terminal proof ready in ${Math.round((Date.now() - started) / 1000)} s`);
  writeFileSync(join(out, "vectors.json"), JSON.stringify({ fields: [a.fields, b.fields] }));

  const pq = deployment.pqPaymaster === true;
  const signer = pq
    ? { pqSigner: lskSigner(values.key ?? settings.exitPqKey()) }
    : { ecdsaKey: readEcdsaKey(values.key ?? settings.operatorKey()) };
  const lines: string[] = [];
  const result = await settle({
    rpc: client,
    deployment,
    signer,
    proofCall: new Uint8Array(readFileSync(calldata)),
    fields: [a.fields, b.fields],
    send: !values["no-send"],
    mempool: mempoolClient(values.mempool),
    log: (line) => {
      console.log(line);
      lines.push(line);
    },
  });
  writeFileSync(join(out, "settle.log"), lines.join("\n") + "\n");
  if (values["no-send"]) {
    log(`settle simulated (not sent): valid ${result.simulation.valid}`);
    return;
  }
  const receipt = result.receipt;
  if (!receipt || receipt.status !== "0x1") {
    log("settle failed");
    process.exit(1);
  }
  mkdirSync(join(queueDir, "done"), { recursive: true });
  for (const s of [a, b]) renameSync(s.fieldsFile, join(queueDir, "done", `${s.id}.json`));
  log(`settled ${a.id} + ${b.id} in block ${BigInt(receipt.blockNumber)}`);
});

/** The recursion on the AWS prover machine (infra/aws/prover.sh: up and setup first); returns the calldata path. */
function proveOnAws(a: string, b: string, out: string): string {
  const gitBash = "C:\\Program Files\\Git\\bin\\bash.exe";
  const bash = process.platform === "win32" && existsSync(gitBash) ? gitBash : "bash";
  execFileSync(bash, [join(import.meta.dirname, "..", "infra", "aws", "prover.sh"), "prove", a, b, out], { stdio: "inherit" });
  return join(out, "calldata.bin");
}
