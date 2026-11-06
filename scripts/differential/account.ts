// Differential test of the leanSPHINCS PQ account: the Lean model (`validFromAccount`,
// `pqAccountPolicy`) against the chain's validation, on mutations of a valid account transaction
// [VERIFY account verifySelector flags 3, SENDER 1 wei -> 0x..dEaD]. Nothing is broadcast.
import { MODE, SCHEME, addArbitraryWitness, fromHex, signTransaction, type FrameTransaction } from "../../sdk/src/index.ts";
import { cli, loadAccount, lskSigner, main, readEcdsaKey, rel, rpc, settings } from "../lib/common.ts";
import { askLean, chainVerdict, modelTx, randomKey, report, type Row } from "../lib/differential.ts";

const USAGE = `
Usage: node scripts/differential/account.ts [options]

Options:
  --account FILE          PQ account record (default: ACCOUNT_DEPLOYMENT or deployments/8141/account.json)
  --pq-key FILE           The account's leanSPHINCS key (default: ACCOUNT_PQ_KEY)
  --other-pq-key FILE     Another leanSPHINCS key (default: EXIT_PQ_KEY)
  --ecdsa-key FILE        An ECDSA key for the mixed-signature cases (default: OPERATOR_KEY, then DEPLOYER_KEY)
  -h, --help              Show this help

Needs the Lean model (LEAN_POLICY_BIN, default lean/.lake/build/bin/pool-policy) and LSK_BIN.
Exits 1 when the model and the chain disagree.
`;

type Sig = "user" | "corrupt" | "otherkey" | "none" | "ecdsa" | "ecdsa+user";
interface Case {
  name: string;
  sender: bigint;
  target0: bigint;
  flags: bigint;
  state: bigint;
  sig: Sig;
}

main(async () => {
  const { values } = cli(USAGE, {
    account: { type: "string" },
    "pq-key": { type: "string" },
    "other-pq-key": { type: "string" },
    "ecdsa-key": { type: "string" },
  });
  const accountFile = values.account ?? settings.accountDeployment();
  const a = loadAccount(accountFile);
  const account = BigInt(a.account);
  const ecdsa = readEcdsaKey(values["ecdsa-key"] ?? settings.operatorKey());
  const user = lskSigner(values["pq-key"] ?? settings.accountPqKey());
  const other = lskSigner(values["other-pq-key"] ?? settings.exitPqKey());
  const eoa = 0x28ca5a65213834f97e17707467337fbcf856e08dn;

  const base = (): Case => ({ name: "baseline", sender: account, target0: account, flags: 3n, state: 110_000n, sig: "user" });
  const mutations: [string, (c: Case) => void][] = [
    ["flags 2 (execution only)", (c) => { c.flags = 2n; }],
    ["flags 1 (payment only)", (c) => { c.flags = 1n; }],
    ["flags 0", (c) => { c.flags = 0n; }],
    ["corrupted witness", (c) => { c.sig = "corrupt"; }],
    ["witness by another key", (c) => { c.sig = "otherkey"; }],
    ["no signature", (c) => { c.sig = "none"; }],
    ["ECDSA signature only", (c) => { c.sig = "ecdsa"; }],
    ["ECDSA signature, then the witness", (c) => { c.sig = "ecdsa+user"; }],
    ["no state budget for the nonce key", (c) => { c.state = 0n; }],
    ["sender = an ECDSA account", (c) => { c.sender = eoa; }],
    ["frame 0 targets another account", (c) => { c.target0 = eoa; }],
  ];
  const cases: Case[] = [base()];
  for (const [n, m] of mutations) {
    const c = base();
    m(c);
    c.name = n;
    cases.push(c);
  }
  for (const [n1, m1] of mutations) {
    for (const [n2, m2] of mutations.slice(3, 6)) {
      if (n1 === n2) continue;
      const c = base();
      m1(c);
      m2(c);
      c.name = `${n1} + ${n2}`;
      cases.push(c);
    }
  }

  const client = rpc();
  const chainId = await client.chainId();
  const fees = await client.feeCaps();
  console.log(`account ${a.account} (${rel(accountFile)}), ${cases.length} cases`);
  const rows: Row[] = [];
  for (const c of cases) {
    let tx: FrameTransaction = {
      chainId,
      nonceKeys: [randomKey(16)],
      nonceSeq: 0n,
      sender: c.sender,
      frames: [
        { mode: MODE.VERIFY, flags: c.flags, target: c.target0, gasLimit: 150_000n, stateLimit: c.state, value: 0n, data: fromHex(a.verifySelector) },
        { mode: MODE.SENDER, flags: 0n, target: 0xdeadn, gasLimit: 50_000n, stateLimit: 0n, value: 1n, data: new Uint8Array(0) },
      ],
      signatures: [],
      ...fees,
      maxBlobFee: 0n,
      blobHashes: [],
    };
    let verifies = false;
    if (c.sig === "ecdsa") tx = signTransaction(tx, ecdsa);
    else if (c.sig !== "none") {
      tx.signatures = [{ scheme: SCHEME.ARBITRARY, signer: 0n, msg: new Uint8Array(0), signature: new Uint8Array(0) }];
      if (c.sig === "ecdsa+user") tx = signTransaction(tx, ecdsa, ...tx.signatures);
      await addArbitraryWitness(tx, c.sig === "otherkey" ? other : user);
      if (c.sig === "corrupt") tx.signatures[tx.signatures.length - 1].signature[300] ^= 1;
      verifies = c.sig === "user" || c.sig === "ecdsa+user";
    }
    const { chain, violation } = await chainVerdict(client, tx);
    rows.push({ name: c.name, chain, violation, query: { account: a.account, tx: modelTx(tx, verifies, "0x0") } });
  }
  const answers = await askLean(rows);
  const disagreements = report("PQ account differential test", rows, answers, (r) => JSON.stringify(r.account));
  if (disagreements > 0) process.exit(1);
});
