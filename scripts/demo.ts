// Demo wallet: one private spend, step by step (the console's Wallet tab runs these steps).
// Notes live in data/wallet/demo; the depositor and recipient is the leanSPHINCS PQ account and
// the fee goes to the exit (the deployment's operator).
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { MixnetClient, PoolReader, Prover } from "../sdk/src/index.ts";
import {
  bigintOption,
  cli,
  loadAccount,
  loadPool,
  lskSigner,
  main,
  pipelineLog,
  readEcdsaKey,
  readJson,
  rel,
  rpc,
  settings,
  usageError,
} from "./lib/common.ts";
import { depositFromAccount, depositFromKey, summarizeSimulation, type NoteFile } from "./lib/pool-ops.ts";

const USAGE = `
Usage: node scripts/demo.ts <step> [NAME] [options]

Steps:
  status                  Every demo note's stage, as JSON
  new NAME                Fresh note and change-note secrets (pool-wallet keygen)
  deposit NAME            Deposit the note from the PQ account (frame tx, leanSPHINCS witness)
  witness NAME            Rebuild both trees from the pool's logs and build the withdrawal:
                          40% to the PQ account, 10% fee to the exit, 50% change
  prove NAME              Full-ZK client proof (prover/prove-client.sh) -> send/NAME.tgz
  send NAME               Submit the proof through the mixnet into the exit's spend queue

Options:
  --amount WEI            deposit: amount (default: 10000000000000000)
  --dry-run               deposit: simulate only, do not broadcast
  --topology FILE         send: network topology (default: data/aws/topology.json)
  --network aws|local     send: shorthand for data/<network>/topology.json
  --deployment FILE       Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  -h, --help              Show this help

Files (data/wallet/demo): note-NAME.json, change-NAME.json, request-NAME.json, input-NAME.json,
public-NAME.json, send/NAME.tgz, sent-NAME.json, chain.json. Progress goes to logs/pipeline.log.
`;

const dir = settings.demoDir();
const file = (kind: string, name: string) => join(dir, `${kind}-${name}.json`);
const proofFile = (name: string) => join(dir, "send", `${name}.tgz`);
const log = (message: string) => pipelineLog("wallet", message);

main(async () => {
  const { values, positionals } = cli(USAGE, {
    amount: { type: "string" },
    "dry-run": { type: "boolean", default: false },
    topology: { type: "string" },
    network: { type: "string" },
    deployment: { type: "string" },
  });
  const [step, name] = positionals;
  mkdirSync(dir, { recursive: true });
  const prover = () => new Prover({ proverDir: settings.proverDir(), wslDistro: settings.wslDistro(), echo: true });

  if (step === "status") {
    const names = [...new Set(readdirSync(dir).filter((f) => f.startsWith("note-") && f.endsWith(".json")).map((f) => f.slice(5, -5)))].sort();
    const rows = names.map((n) => {
      const note = readJson<NoteFile>(file("note", n));
      return {
        name: n,
        amount: note.amount ?? null,
        leaf: note.leaf_index ?? null,
        deposit_block: note.deposit_block ?? null,
        witness: existsSync(file("input", n)),
        proof: existsSync(proofFile(n)),
        sent: existsSync(file("sent", n)) ? readJson(file("sent", n)) : null,
      };
    });
    console.log(JSON.stringify(rows));
    return;
  }

  if (!step) usageError(USAGE, "missing step");
  if (!name || !/^[A-Za-z0-9_-]+$/.test(name)) usageError(USAGE, `${step} needs a NAME (letters, digits, - and _)`);

  if (step === "new") {
    if (existsSync(file("note", name))) throw new Error(`note ${name} already exists`);
    await prover().keygen(file("note", name));
    await prover().keygen(file("change", name));
    console.log(`note ${name}: fresh note and change-note secrets (Poseidon2 precommitments)`);
    log(`new note ${name}`);
  } else if (step === "deposit") {
    const deployment = loadPool(values.deployment);
    const send = !values["dry-run"];
    const amount = bigintOption(values.amount, 10_000_000_000_000_000n, "--amount");
    const pqKey = settings.accountPqKey();
    let note: NoteFile | undefined;
    if (existsSync(pqKey)) {
      try {
        const result = await depositFromAccount({
          rpc: rpc(),
          deployment,
          account: loadAccount(),
          signer: lskSigner(pqKey),
          noteFile: file("note", name),
          amount,
          send,
        });
        console.log(`simulate: ${summarizeSimulation(result.simulation!)}`);
        note = result.note;
      } catch (err) {
        console.warn(`PQ account deposit failed (${(err as Error).message}), attempting fallback to deployer key...`);
      }
    }
    if (!note) {
      const keyFile = settings.deployerKey();
      if (!existsSync(keyFile)) {
        throw new Error(`no valid signing key found: neither ${pqKey} nor ${keyFile} exist`);
      }
      const key = readEcdsaKey(keyFile);
      const result = await depositFromKey({
        rpc: rpc(),
        deployment,
        key,
        noteFile: file("note", name),
        amount,
        send,
      });
      note = result.note;
    }
    if (!send) return;
    if (!note) throw new Error("deposit was not recorded");
    console.log(`note ${name}: deposited ${note.amount} wei, leaf ${note.leaf_index}, block ${note.deposit_block}`);
    log(`deposited note ${name}: leaf ${note.leaf_index}, block ${note.deposit_block}`);
  } else if (step === "witness") {
    const deployment = loadPool(values.deployment);
    const account = loadAccount();
    const note = readJson<NoteFile>(file("note", name));
    if (!note.amount) throw new Error(`note ${name} has no deposit yet`);
    const chainFile = join(dir, "chain.json");
    const view = await new PoolReader(rpc(), deployment.pool).chainView(deployment);
    writeFileSync(chainFile, JSON.stringify(view, null, 1));
    const amount = BigInt(note.amount);
    const withdrawn = (amount * 4n) / 10n;
    const fee = amount / 10n;
    writeFileSync(
      file("request", name),
      JSON.stringify(
        {
          recipient: account.account,
          withdrawn: withdrawn.toString(),
          fee: fee.toString(),
          fee_recipient: deployment.operator,
          change_note: readJson(file("change", name)),
        },
        null,
        1,
      ),
    );
    await prover().witness(file("note", name), chainFile, file("request", name), file("input", name));
    writeFileSync(file("public", name), JSON.stringify({ fields: readJson(file("input", name)).public.fields }));
    console.log(`withdrawal ${name}: ${withdrawn} wei to the PQ account, fee ${fee} to the exit, change ${amount - withdrawn - fee}`);
    log(`withdrawal ${name} witnessed against the pool's current roots`);
  } else if (step === "prove") {
    if (!existsSync(file("input", name))) throw new Error(`note ${name} has no witness yet (run: witness ${name})`);
    mkdirSync(join(dir, "send"), { recursive: true });
    await prover().proveClient(file("input", name), proofFile(name));
    log(`wallet proof ready for note ${name} (full-ZK Spartan-WHIR)`);
  } else if (step === "send") {
    if (!existsSync(proofFile(name))) throw new Error(`note ${name} has no proof yet (run: prove ${name})`);
    if (values.network && !["aws", "local"].includes(values.network)) usageError(USAGE, "--network is aws or local");
    const topology = values.topology ?? settings.topology(values.network ?? "aws");
    const mixnet = new MixnetClient({ client: settings.mixnetClient(), topology, identity: settings.memberIdentity() });
    const reply = await mixnet.submitSpend(proofFile(name), file("public", name));
    process.stderr.write(reply.stderr);
    if (!reply.ok) throw new Error(`the exit did not queue the spend: ${reply.reason ?? "no reason given"}`);
    const { stderr: _, ...stored } = reply;
    writeFileSync(file("sent", name), JSON.stringify({ ...stored, topology: rel(topology), at: new Date().toISOString() }));
    console.log(`spend ${name} queued at the exit as ${reply.queued}`);
    log(`spend ${name}: ${Math.round((reply.bytes ?? 0) / 1024)} KB through the mixnet, queued at the exit as ${reply.queued}`);
  } else {
    usageError(USAGE, `unknown step: ${step}`);
  }
});
