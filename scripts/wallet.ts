// Pool wallet, chain side (the `pool-wallet` binary does keys, Merkle paths and witnesses).
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { PoolReader, packDigest } from "../sdk/src/index.ts";
import {
  bigintOption,
  cli,
  loadAccount,
  loadPool,
  lskSigner,
  main,
  readEcdsaKey,
  rel,
  rpc,
  settings,
  usageError,
} from "./lib/common.ts";
import { depositFromAccount, depositFromKey, summarizeSimulation } from "./lib/pool-ops.ts";

const USAGE = `
Usage: node scripts/wallet.ts <command> [options]

Commands:
  chain                   Rebuild both trees from the pool's Deposited/Settled logs and write the
                          chain view (leaves, current roots, pool and scope identity) that
                          \`pool-wallet witness\` reads. Read-only.
  deposit                 deposit(precommitment) from an ECDSA account (EIP-1559 transaction).
  deposit-pq              The same deposit from the leanSPHINCS PQ account (frame transaction).

Options:
  --deployment FILE       Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  --out FILE              chain: output file (default: data/wallet/chain.json)
  --from-block N          chain: first block to scan (default: 0)
  --note FILE             deposit*: note file from \`pool-wallet keygen\`; updated with the deposit
  --amount WEI            deposit*: amount in wei (default: 10000000000000000)
  --key FILE              deposit: ECDSA key file (default: DEPLOYER_KEY)
  --account FILE          deposit-pq: PQ account record (default: deployments/8141/account.json)
  --pq-key FILE           deposit-pq: leanSPHINCS key file (default: ACCOUNT_PQ_KEY)
  --send                  deposit*: broadcast. Without it, deposit-pq only simulates and deposit
                          only reports what it would send.
  -h, --help              Show this help
`;

main(async () => {
  const { values, positionals } = cli(USAGE, {
    deployment: { type: "string" },
    out: { type: "string" },
    "from-block": { type: "string" },
    note: { type: "string" },
    amount: { type: "string" },
    key: { type: "string" },
    account: { type: "string" },
    "pq-key": { type: "string" },
    send: { type: "boolean", default: false },
  });
  const [command] = positionals;
  const deployment = loadPool(values.deployment);
  const client = rpc();

  if (command === "chain") {
    const out = values.out ?? join(settings.walletDir(), "chain.json");
    const pool = new PoolReader(client, deployment.pool);
    const fromBlock = values["from-block"] === undefined ? "earliest" : bigintOption(values["from-block"], 0n, "--from-block");
    const view = await pool.chainView(deployment, fromBlock);
    const [poolKnown, associationKnown] = await Promise.all([
      pool.knownPoolRoot(packDigest(view.pool_root)),
      pool.knownAssociationRoot(packDigest(view.association_root)),
    ]);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, JSON.stringify(view, null, 1));
    console.log(
      `chain view: ${view.pool_leaves.length} pool leaves, ${view.association_leaves.length} association leaves -> ${rel(out)}`,
    );
    console.log(`roots: pool known ${poolKnown}, association known ${associationKnown}`);
    if (!poolKnown || !associationKnown) throw new Error("the pool does not recognise its current roots");
    return;
  }

  if (command === "deposit" || command === "deposit-pq") {
    if (!values.note) usageError(USAGE, `${command} needs --note FILE`);
    const amount = bigintOption(values.amount, 10_000_000_000_000_000n, "--amount");
    if (command === "deposit-pq") {
      const account = loadAccount(values.account);
      const result = await depositFromAccount({
        rpc: client,
        deployment,
        account,
        signer: lskSigner(values["pq-key"] ?? settings.accountPqKey()),
        noteFile: values.note,
        amount,
        send: values.send,
      });
      console.log(`simulate: ${summarizeSimulation(result.simulation!)}`);
      if (!values.send) return console.log("not sent (pass --send to broadcast)");
      const rc = result.receipt!;
      console.log(
        `PQ account deposited ${amount} wei: leaf ${result.note!.leaf_index}, block ${result.note!.deposit_block}, gas ${BigInt(rc.gasUsed)}, payer ${rc.payer ?? "-"}`,
      );
    } else {
      const key = readEcdsaKey(values.key ?? settings.deployerKey());
      const result = await depositFromKey({ rpc: client, deployment, key, noteFile: values.note, amount, send: values.send });
      if (!values.send) return console.log(`would deposit ${amount} wei from ${result.from} into ${deployment.pool} (pass --send to broadcast)`);
      console.log(`deposited ${amount} wei: leaf ${result.note!.leaf_index}, label index ${result.note!.label_index}, block ${result.note!.deposit_block}`);
    }
    return;
  }

  usageError(USAGE, command ? `unknown command: ${command}` : "missing command");
});
