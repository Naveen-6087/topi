// Differential test of the pool policy: the Lean model (`valid`, `poolPolicy`, `paymasterPolicy`)
// against the chain's validation, on random mutations of a valid settle transaction for a real
// deployment. Validation only: frame 2 carries a short settle call, so executions revert, but
// validity is decided before execution. Nothing is broadcast.
import { MODE, SCHEME, addArbitraryWitness, signTransaction, type Frame, type FrameTransaction } from "../../sdk/src/index.ts";
import { bigintOption, cli, loadPool, lskSigner, main, readEcdsaKey, rel, rpc, settings } from "../lib/common.ts";
import { askLean, chainVerdict, modelTx, randomKey, report, type Row } from "../lib/differential.ts";

const USAGE = `
Usage: node scripts/differential/policy.ts [options]

Options:
  --deployment FILE       Pool deployment record (default: POOL_DEPLOYMENT or deployments/8141/pool.json)
  --count N               Random multi-mutation cases on top of the single mutations (default: 20)
  --seed N                Seed for the random cases (default: 1)
  --key FILE              The operator's ECDSA key file (default: OPERATOR_KEY, then DEPLOYER_KEY)
  --exit-pq-key FILE      PQ paymaster: the exit's leanSPHINCS key (default: EXIT_PQ_KEY)
  --other-pq-key FILE     PQ paymaster: another leanSPHINCS key (default: ACCOUNT_PQ_KEY)
  --paymaster-state N     Paymaster frame state budget (default: 110000)
  -h, --help              Show this help

Needs the Lean model (LEAN_POLICY_BIN, default lean/.lake/build/bin/pool-policy) and, for a PQ
paymaster, LSK_BIN. Exits 1 when the model and the chain disagree.
`;

type Signer = "operator" | "other" | "none" | "pq" | "pq-corrupt" | "pq-otherkey" | "pq+ecdsa";
interface Case {
  name: string;
  sender: bigint;
  frames: Frame[];
  keys: number;
  seq: bigint;
  signer: Signer;
}

main(async () => {
  const { values } = cli(USAGE, {
    deployment: { type: "string" },
    count: { type: "string", default: "20" },
    seed: { type: "string", default: "1" },
    key: { type: "string" },
    "exit-pq-key": { type: "string" },
    "other-pq-key": { type: "string" },
    "paymaster-state": { type: "string" },
  });
  const deploymentFile = values.deployment ?? settings.poolDeployment();
  const d = loadPool(deploymentFile);
  const opKey = readEcdsaKey(values.key ?? settings.operatorKey());
  const otherKey = new Uint8Array(32).fill(7);
  const exitPq = lskSigner(values["exit-pq-key"] ?? settings.exitPqKey());
  const otherPq = lskSigner(values["other-pq-key"] ?? settings.accountPqKey());
  const pool = BigInt(d.pool);
  const pm = BigInt(d.paymaster);
  const random = 0x1234567890abcdef1234567890abcdef12345678n;
  const pqDeploy = d.pqPaymaster === true;
  const pmState = bigintOption(values["paymaster-state"], 110_000n, "--paymaster-state");

  let seed = Number(values.seed) >>> 0;
  const rnd = (n: number) => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed % n;
  };
  const settleData = (len: number, sel = "f4b589c7") => {
    const b = new Uint8Array(len);
    b.set(Buffer.from(sel, "hex").subarray(0, Math.min(4, len)));
    return b;
  };
  const base = (): Case => ({
    name: "baseline",
    sender: pool,
    keys: 1,
    seq: 0n,
    signer: pqDeploy ? "pq" : "operator",
    frames: [
      { target: pool, mode: MODE.VERIFY, flags: 2n, value: 0n, data: new Uint8Array(4), gasLimit: 80_000n, stateLimit: 0n },
      { target: pm, mode: MODE.VERIFY, flags: 1n, value: 0n, data: new Uint8Array(4), gasLimit: pqDeploy ? 200_000n : 60_000n, stateLimit: pmState },
      { target: pool, mode: MODE.SENDER, flags: 0n, value: 0n, data: settleData(68), gasLimit: 200_000n, stateLimit: 200_000n },
    ],
  });
  const mutations: [string, (c: Case) => void][] = [
    ["sender=paymaster", (c) => { c.sender = pm; }],
    ["sender=random", (c) => { c.sender = random; }],
    ["drop frame 2", (c) => { c.frames.pop(); }],
    ["extra DEFAULT frame", (c) => { c.frames.push({ ...base().frames[2], mode: MODE.DEFAULT, data: new Uint8Array(0) }); }],
    ["extra SENDER frame", (c) => { c.frames.push({ ...base().frames[2] }); }],
    ["f0.flags=1", (c) => { c.frames[0].flags = 1n; }],
    ["f0.flags=3", (c) => { c.frames[0].flags = 3n; }],
    ["f0.target=paymaster", (c) => { c.frames[0].target = pm; }],
    ["f0.target=random", (c) => { c.frames[0].target = random; }],
    ["f1.target=pool", (c) => { c.frames[1].target = pool; }],
    ["f1.target=random", (c) => { c.frames[1].target = random; }],
    ["f1.flags=2", (c) => { c.frames[1].flags = 2n; }],
    ["f1.flags=3", (c) => { c.frames[1].flags = 3n; }],
    ["f2.target=paymaster", (c) => { if (c.frames[2]) c.frames[2].target = pm; }],
    ["f2.mode=DEFAULT", (c) => { if (c.frames[2]) c.frames[2].mode = MODE.DEFAULT; }],
    ["f2.flags=atomic", (c) => { if (c.frames[2]) c.frames[2].flags = 4n; }],
    ["f2.value=1", (c) => { if (c.frames[2]) c.frames[2].value = 1n; }],
    ["f2.selector=deposit", (c) => { if (c.frames[2]) c.frames[2].data = settleData(68, "09d009b8"); }],
    ["f2.data=3 bytes", (c) => { if (c.frames[2]) c.frames[2].data = settleData(3); }],
    ["f2.data empty", (c) => { if (c.frames[2]) c.frames[2].data = new Uint8Array(0); }],
    ["2 nonce keys", (c) => { c.keys = 2; }],
    ["2 nonce keys + paymaster state 220k", (c) => { c.keys = 2; c.frames[1].stateLimit = 220_000n; }],
    ["paymaster state 90k", (c) => { c.frames[1].stateLimit = 90_000n; }],
    ["signer=other", (c) => { c.signer = "other"; }],
    ["no signature", (c) => { c.signer = "none"; }],
    ...(pqDeploy
      ? ([
          ["pq: corrupted witness", (c) => { c.signer = "pq-corrupt"; }],
          ["pq: witness by another leanSPHINCS key", (c) => { c.signer = "pq-otherkey"; }],
          ["pq: ECDSA operator signature instead", (c) => { c.signer = "operator"; }],
          ["pq: witness plus an ECDSA signature", (c) => { c.signer = "pq+ecdsa"; }],
        ] as [string, (c: Case) => void][])
      : []),
  ];

  const cases: Case[] = [base()];
  for (const [name, m] of mutations) {
    const c = base();
    m(c);
    c.name = name;
    cases.push(c);
  }
  for (let i = 0; i < Number(values.count); i++) {
    const c = base();
    const names: string[] = [];
    for (let k = 0, n = 1 + rnd(3); k < n; k++) {
      const [name, m] = mutations[rnd(mutations.length)];
      m(c);
      names.push(name);
    }
    c.name = names.join(" + ");
    cases.push(c);
  }

  const client = rpc();
  const chainId = await client.chainId();
  const fees = await client.feeCaps();
  console.log(`deployment ${rel(deploymentFile)} (${pqDeploy ? "leanSPHINCS" : "ECDSA"} paymaster), ${cases.length} cases`);
  const rows: Row[] = [];
  for (const c of cases) {
    let tx: FrameTransaction = {
      chainId,
      nonceKeys: Array.from({ length: c.keys }, (_, i) => randomKey(15) * 4n + BigInt(i)).sort((x, y) => (x < y ? -1 : 1)),
      nonceSeq: c.seq,
      sender: c.sender,
      frames: c.frames,
      signatures: [],
      ...fees,
      maxBlobFee: 0n,
      blobHashes: [],
    };
    let verifies = false;
    if (c.signer.startsWith("pq")) {
      tx.signatures = [{ scheme: SCHEME.ARBITRARY, signer: 0n, msg: new Uint8Array(0), signature: new Uint8Array(0) }];
      if (c.signer === "pq+ecdsa") tx = signTransaction(tx, opKey, ...tx.signatures);
      await addArbitraryWitness(tx, c.signer === "pq-otherkey" ? otherPq : exitPq);
      if (c.signer === "pq-corrupt") tx.signatures[tx.signatures.length - 1].signature[300] ^= 1;
      verifies = c.signer === "pq" || c.signer === "pq+ecdsa";
    } else if (c.signer !== "none") {
      tx = signTransaction(tx, c.signer === "operator" ? opKey : otherKey);
    }
    const { chain, violation } = await chainVerdict(client, tx);
    rows.push({
      name: c.name,
      chain,
      violation,
      query: {
        deployment: { pool: d.pool, paymaster: d.paymaster, operator: d.operator, pqPaymaster: pqDeploy },
        tx: modelTx(tx, verifies, "0x00"),
      },
    });
  }
  const answers = await askLean(rows);
  const disagreements = report("pool policy differential test", rows, answers, (a) => `pool ${a.pool}, paymaster ${a.paymaster}`);
  if (disagreements > 0) process.exit(1);
});
