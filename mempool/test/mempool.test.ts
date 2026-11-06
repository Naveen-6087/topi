import assert from "node:assert/strict";
import { test } from "node:test";
import type { AddressInfo } from "node:net";

import {
  APPROVE,
  MODE,
  SPEND_FIELDS,
  encodeRaw,
  fromFrameTxRequest,
  packFields,
  randomBytes,
  toFrameTxRequest,
  toHex,
} from "../../sdk/src/index.ts";

import { Mempool } from "../src/mempool.ts";
import { Releaser } from "../src/releaser.ts";
import { Rejection, checkSettle, decodeSettleFields, policyFor } from "../src/rules.ts";
import { cidV0, mempoolId } from "../src/mempool-id.ts";
import { Gossip, serve } from "../src/server.ts";
import { FakeChain, deployment, hashOf, policy, rawSettle, settleTx, spendFields } from "./fixtures.ts";

const code = async (p: Promise<unknown>) => {
  try {
    await p;
  } catch (e) {
    if (e instanceof Rejection) return e.code;
    throw e;
  }
  return "accepted";
};
const codeOf = (f: () => unknown) => code(Promise.resolve().then(f));

async function fresh(options: Partial<ConstructorParameters<typeof Mempool>[0]> = {}) {
  const chain = new FakeChain();
  const mempool = new Mempool({ policy, chain, ...options });
  await mempool.start();
  return { chain, mempool };
}

// ---------- rules ----------

test("a well-formed settle passes the offline rules and its spends are read back", () => {
  const s = checkSettle(settleTx(1), policy);
  assert.equal(s.sender, BigInt(deployment.pool));
  assert.equal(s.paymaster, BigInt(deployment.paymaster));
  assert.deepEqual(s.nullifiers, [packFields(spendFields(2), SPEND_FIELDS.nullifier), packFields(spendFields(3), SPEND_FIELDS.nullifier)]);
  assert.equal(s.validationGas, 160_000n);
});

test("the offline rules refuse every shape the pool or paymaster would refuse", async () => {
  const cases: [string, Parameters<typeof settleTx>[1]][] = [
    ["not-pool", (tx) => (tx.sender = 1n)],
    ["wrong-chain", (tx) => (tx.chainId = 1n)],
    ["shape", (tx) => tx.frames.push({ ...tx.frames[2] })],
    ["shape", (tx) => (tx.frames[0].flags = APPROVE.EXECUTION_AND_PAYMENT)],
    ["shape", (tx) => (tx.frames[1].mode = MODE.SENDER)],
    ["shape", (tx) => (tx.frames[1].target = BigInt(deployment.pool))],
    ["paymaster", (tx) => (tx.frames[1].target = 0x1234n)],
    ["shape", (tx) => (tx.frames[2].value = 1n)],
    ["not-settle", (tx) => (tx.frames[2].data = new Uint8Array([1, 2, 3, 4, ...tx.frames[2].data.subarray(4)]))],
    ["nonce", (tx) => (tx.nonceSeq = 1n)],
    ["nonce", (tx) => (tx.nonceKeys = [])],
    ["shape", (tx) => (tx.blobHashes = [randomBytes(32)])],
    ["signature", (tx) => (tx.signatures = [])],
    ["signature", (tx) => (tx.signatures[0].msg = new Uint8Array([1]))],
    ["signature", (tx) => (tx.signatures[0].signature = randomBytes(100))],
    ["validation-gas", (tx) => (tx.frames[1].gasLimit = 500_000n)],
  ];
  for (const [want, edit] of cases) assert.equal(await codeOf(() => checkSettle(settleTx(1, edit), policy)), want, String(edit));
});

test("spend fields are checked: this pool, this scope, two different nullifiers", async () => {
  const otherPool = settleTx(1);
  const bad = spendFields(9);
  bad[SPEND_FIELDS.poolId] += 1;
  const { buildSettleTransaction } = await import("../../sdk/src/index.ts");
  const make = (a: number[], b: number[]) => {
    const tx = buildSettleTransaction({
      deployment,
      chainId: 8141n,
      fees: { maxPriorityFee: 1n, maxFee: 2n },
      proofCall: randomBytes(64),
      fields: [a, b],
    });
    tx.signatures = otherPool.signatures;
    return tx;
  };
  assert.equal(await codeOf(() => checkSettle(make(bad, spendFields(10)), policy)), "fields");
  assert.equal(await codeOf(() => checkSettle(make(spendFields(5), spendFields(5)), policy)), "fields");
  assert.equal(decodeSettleFields(settleTx(4).frames[2].data)[1][SPEND_FIELDS.nullifier], 9);
});

test("extra paymasters can be accepted alongside the deployment's", () => {
  const other = 0x98f4e0c284fbb848adccb4470f74d08054569ea4n;
  const tx = settleTx(1, (t) => (t.frames[1].target = other));
  assert.throws(() => checkSettle(tx, policy), Rejection);
  assert.equal(checkSettle(tx, policyFor(deployment, [other])).paymaster, other);
});

// ---------- admission ----------

test("admits a settle once, then reports it as known", async () => {
  const { mempool } = await fresh();
  const raw = rawSettle(1);
  assert.deepEqual(await mempool.admit(raw), { hash: hashOf(raw), result: "admitted" });
  assert.deepEqual(await mempool.admit(raw), { hash: hashOf(raw), result: "known" });
  assert.equal(mempool.size, 1);
});

test("holds many settles from the one shared pool sender at once", async () => {
  const { mempool } = await fresh();
  for (let i = 1; i <= 10; i++) await mempool.admit(rawSettle(i));
  assert.equal(mempool.size, 10);
  assert.equal(mempool.senders().length, 1);
});

test("refuses a second settle that spends a nullifier already pending", async () => {
  const { mempool } = await fresh();
  await mempool.admit(rawSettle(1));
  // same spends, different nonce key: a different transaction racing for the same notes
  assert.equal(await code(mempool.admit(rawSettle(1))), "nullifier-pending");
});

test("checks the chain: spent nullifiers, unknown roots, changed code, witness, simulation", async () => {
  {
    const { chain, mempool } = await fresh();
    chain.spentSet.add(packFields(spendFields(3), SPEND_FIELDS.nullifier));
    assert.equal(await code(mempool.admit(rawSettle(1))), "nullifier-spent");
  }
  {
    const { chain, mempool } = await fresh();
    chain.unknownRoots.add(packFields(spendFields(2), SPEND_FIELDS.poolRoot));
    assert.equal(await code(mempool.admit(rawSettle(1))), "unknown-root");
  }
  {
    const { chain, mempool } = await fresh();
    chain.codeHashes.set(BigInt(deployment.paymaster), toHex(randomBytes(32)));
    assert.equal(await code(mempool.admit(rawSettle(1))), "code-changed");
  }
  {
    const { chain, mempool } = await fresh();
    chain.witnessOk = false;
    assert.equal(await code(mempool.admit(rawSettle(1))), "witness");
  }
  {
    const { chain, mempool } = await fresh();
    chain.simulation = { valid: false, violation: "Nonce mismatch" };
    assert.equal(await code(mempool.admit(rawSettle(1))), "simulation");
    chain.simulation = { valid: true, violation: null, executionStatus: "Revert", executionError: "out of gas" };
    assert.equal(await code(mempool.admit(rawSettle(2))), "simulation");
  }
});

test("enforces its caps and bans a peer that keeps sending invalid settles", async () => {
  const { mempool } = await fresh({ maxPending: 2 });
  await mempool.admit(rawSettle(1));
  await mempool.admit(rawSettle(2));
  assert.equal(await code(mempool.admit(rawSettle(3))), "full");
  const { mempool: m2 } = await fresh({ banAfter: 3 });
  for (let i = 0; i < 3; i++) await code(m2.admit(rawSettle(1, (tx) => (tx.sender = 5n)), "peer:x"));
  assert.ok(m2.isBanned("peer:x"));
  assert.equal(await code(m2.admit(rawSettle(1), "peer:x")), "banned");
  assert.equal(await code(m2.admit(rawSettle(1), "local")), "accepted");
});

test("drops pending settles that a new block made invalid, and expired ones", async () => {
  let now = 0;
  const { chain, mempool } = await fresh({ now: () => now, ttlMs: 1_000 });
  await mempool.admit(rawSettle(1));
  await mempool.admit(rawSettle(2));
  chain.spentSet.add(packFields(spendFields(2), SPEND_FIELDS.nullifier)); // spend 1 settled elsewhere
  await mempool.revalidate();
  assert.equal(mempool.size, 1);
  now = 5_000;
  await mempool.revalidate();
  assert.equal(mempool.size, 0);
  assert.deepEqual(mempool.counters.dropped, { "nullifier-spent": 1, expired: 1 });
});

// ---------- release ----------

test("releases one settle at a time per sender, oldest first, and moves on when it is mined", async () => {
  let now = 0;
  const { chain, mempool } = await fresh({ now: () => now++ });
  const raws = [rawSettle(1), rawSettle(2), rawSettle(3)];
  for (const r of raws) await mempool.admit(r);
  const releaser = new Releaser(mempool);
  await releaser.tick();
  assert.deepEqual(chain.sent, [hashOf(raws[0])]);
  await releaser.tick();
  assert.deepEqual(chain.sent, [hashOf(raws[0])], "nothing more while one is in flight");
  chain.mine(hashOf(raws[0]));
  await releaser.tick();
  assert.deepEqual(chain.sent, [hashOf(raws[0]), hashOf(raws[1])]);
  chain.mine(hashOf(raws[1]), "0x0");
  await releaser.tick();
  assert.equal(mempool.counters.mined, 1);
  assert.equal(mempool.counters.reverted, 1);
  assert.equal(chain.sent.length, 3);
});

test("a holding node never releases, but drops a settle once a peer gets it mined", async () => {
  const { chain, mempool } = await fresh();
  await mempool.admit(rawSettle(1));
  const holder = new Releaser(mempool, { release: false });
  await holder.tick();
  assert.deepEqual(chain.sent, []);
  assert.equal(mempool.size, 1);
  chain.block++;
  chain.spentSet.add(packFields(spendFields(2), SPEND_FIELDS.nullifier));
  await holder.tick();
  assert.equal(mempool.size, 0);
  assert.deepEqual(mempool.counters.dropped, { "nullifier-spent": 1 });
});

test("drops a settle the node refuses, and keeps one the node already has", async () => {
  const { chain, mempool } = await fresh();
  await mempool.admit(rawSettle(1));
  chain.refuse = "already known";
  await new Releaser(mempool).tick();
  assert.equal(mempool.list()[0].status, "inflight");
  const { chain: c2, mempool: m2 } = await fresh();
  await m2.admit(rawSettle(1));
  c2.refuse = "insufficient funds for gas";
  await new Releaser(m2).tick();
  assert.equal(m2.size, 0);
  assert.equal(m2.counters.dropped["node-refused"], 1);
});

// ---------- the RPC endpoint and gossip ----------

async function node(peers: string[] = []) {
  const { chain, mempool } = await fresh();
  const gossip = new Gossip(mempool, "", peers);
  const server = await serve(mempool, gossip, 0);
  const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  (gossip as { id: string }).id = url;
  return { chain, mempool, gossip, server, url };
}

const call = async (url: string, method: string, params: unknown[] = []) =>
  (await (await fetch(url, { method: "POST", body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) })).json()) as {
    result?: any;
    error?: { message: string; data?: { reason: string } };
  };

const until = async (ok: () => boolean) => {
  for (let i = 0; i < 100 && !ok(); i++) await new Promise((r) => setTimeout(r, 20));
  assert.ok(ok());
};

test("speaks JSON-RPC: eth_sendRawTransaction, status, pending, rules, metrics", async () => {
  const a = await node();
  try {
    const raw = rawSettle(1);
    assert.equal((await call(a.url, "eth_sendRawTransaction", [toHex(raw)])).result, hashOf(raw));
    const refused = await call(a.url, "eth_sendRawTransaction", [toHex(rawSettle(2, (tx) => (tx.sender = 9n)))]);
    assert.equal(refused.error?.data?.reason, "not-pool");
    assert.equal((await call(a.url, "altmempool_status")).result.pending, 1);
    assert.equal((await call(a.url, "altmempool_pending")).result[0].hash, hashOf(raw));
    assert.equal((await call(a.url, "altmempool_rules")).result.pool, deployment.pool.toLowerCase());
    assert.equal((await call(a.url, "eth_chainId")).result, "0x1fcd");
    const metrics = await (await fetch(`${a.url}/metrics`)).text();
    assert.match(metrics, /altmempool_admitted_total 1/);
    assert.match(metrics, /altmempool_rejected_total\{reason="not-pool"\} 1/);
  } finally {
    a.server.close();
  }
});

test("speaks eth_sendFrameTransaction: the JSON form re-encodes to the same raw bytes and hash", async () => {
  const tx = settleTx(3);
  const raw = encodeRaw(tx);
  const req = toFrameTxRequest(tx);
  assert.deepEqual(encodeRaw(fromFrameTxRequest(JSON.parse(JSON.stringify(req)))), raw, "lossless round trip");
  assert.equal(req.signatures[0].signer, "0x");
  assert.equal(req.signatures[0].msg, "0x");
  assert.equal(req.frames[1].flags, 1);
  const a = await node();
  try {
    assert.equal((await call(a.url, "eth_sendFrameTransaction", [req])).result, hashOf(raw));
    const bad = await call(a.url, "eth_sendFrameTransaction", ["0x00"]);
    assert.equal(bad.error?.data?.reason, "params");
  } finally {
    a.server.close();
  }
});

test("the mempool id is the IPFS CIDv0 of RULES.md", () => {
  assert.equal(cidV0(new TextEncoder().encode("hello world\n")), "QmT78zSuBmuS4z925WZfrqQ1qHaJ56DQaTfyMUF7F8ff5o");
  assert.match(mempoolId(), /^Qm[1-9A-HJ-NP-Za-km-z]{44}$/);
});

test("gossips an admitted settle to peers, which hold it once and do not send it back", async () => {
  const b = await node();
  const c = await node();
  const a = await node([b.url, c.url]);
  b.gossip.peers.push(a.url, c.url);
  c.gossip.peers.push(a.url, b.url);
  try {
    const raw = rawSettle(1);
    await a.mempool.admit(raw);
    await until(() => b.mempool.size === 1 && c.mempool.size === 1);
    assert.equal(b.mempool.get(hashOf(raw))?.source, `peer:${a.url}`);
    await new Promise((r) => setTimeout(r, 100));
    assert.equal(a.mempool.size, 1);
    assert.equal(a.mempool.counters.admitted, 1, "the origin never re-admits its own settle");
  } finally {
    for (const n of [a, b, c]) n.server.close();
  }
});
