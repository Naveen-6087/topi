import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  DEPOSITED_TOPIC,
  SETTLED_TOPIC,
  SPEND_FIELDS,
  buildSettleTransaction,
  buildTrees,
  checkSpendFields,
  contextDigest,
  decodePoolLog,
  depositCalldata,
  deriveIdentity,
  packDigest,
  packFields,
  parsePoolDeployment,
  selector,
  settleCalldata,
  toHex,
  unpackDigest,
  word,
  concat,
  type Hex,
  type PoolDeployment,
} from "../src/index.ts";

const deployment = parsePoolDeployment(
  JSON.parse(readFileSync(new URL("../../deployments/8141/pool.json", import.meta.url), "utf8")),
);

const hexWords = (...values: bigint[]): Hex => toHex(concat(...values.map((v) => word(v))));

test("selectors and event topics (cross-checked with cast)", () => {
  assert.equal(toHex(selector("settle(bytes,uint256[],uint256[])")), "0xf4b589c7");
  assert.equal(toHex(selector("deposit(uint256[8])")), "0x09d009b8");
  assert.equal(DEPOSITED_TOPIC, "0x0a944b8f49444f906e0a3f95a3c09508d3697509680d81709d25c327ab8cf651");
  assert.equal(SETTLED_TOPIC, "0xd6bc57af9bdeff672c39eb7ac8725e83883c78f01ea44f6ce017a99e3ef5bb3c");
});

test("digest packing is little-endian 32-bit words and round-trips", () => {
  const digest = [1, 2, 3, 4, 5, 6, 7, 0xffffffff];
  const packed = packDigest(digest);
  assert.equal(packed & 0xffffffffn, 1n);
  assert.equal(packed >> 224n, 0xffffffffn);
  assert.deepEqual(unpackDigest(packed), digest);
  const fields = Array.from({ length: 82 }, (_, i) => i);
  assert.deepEqual(unpackDigest(packFields(fields, 50)), [50, 51, 52, 53, 54, 55, 56, 57]);
  assert.throws(() => packDigest([1, 2, 3]));
});

test("context digest: six 28-bit limbs of the address, low first, then two zeros", () => {
  const ctx = contextDigest("0x28Ca5A65213834f97e17707467337fBcf856E08d");
  assert.equal(ctx.length, 8);
  assert.deepEqual(ctx.slice(6), [0, 0]);
  let v = 0n;
  for (let i = 5; i >= 0; i--) v = (v << 28n) | BigInt(ctx[i]);
  assert.equal(v, 0x28ca5a65213834f97e17707467337fbcf856e08dn);
});

test("identities are eight field elements", () => {
  const id = deriveIdentity("pq-pool:8141:example:pool");
  assert.equal(id.length, 8);
  assert.ok(id.every((x) => x >= 0 && x < 2_130_706_433));
  assert.deepEqual(deriveIdentity("pq-pool:8141:example:pool"), id);
});

test("spend field checks against the deployment", () => {
  const fields = Array(SPEND_FIELDS.count).fill(0);
  fields.splice(SPEND_FIELDS.poolId, 8, ...deployment.poolId);
  fields.splice(SPEND_FIELDS.scope, 8, ...deployment.scope);
  fields.splice(SPEND_FIELDS.context, 8, ...contextDigest(deployment.operator));
  assert.equal(checkSpendFields(fields, deployment), null);
  const other = [...fields];
  other[3] ^= 1;
  assert.equal(checkSpendFields(other, deployment), "not this pool");
  assert.equal(checkSpendFields(fields, deployment, "0x000000000000000000000000000000000000dead"), "fee is not committed to this exit");
  assert.match(checkSpendFields(fields.slice(1), deployment) ?? "", /expected 82 fields/);
});

test("event decoding and tree rebuild", () => {
  const note = packDigest([1, 1, 1, 1, 1, 1, 1, 1]);
  const label = packDigest([2, 2, 2, 2, 2, 2, 2, 2]);
  const meta = { blockNumber: "0x10" as Hex, transactionHash: "0xabc" as Hex };
  const deposit = decodePoolLog({
    topics: [DEPOSITED_TOPIC, hexWords(0n)],
    data: hexWords(0n, note, label, 10n ** 16n, 111n, 222n),
    ...meta,
  });
  assert.equal(deposit?.kind, "deposited");
  assert.equal(deposit?.kind === "deposited" && deposit.amount, 10n ** 16n);
  const changeA = packDigest([3, 3, 3, 3, 3, 3, 3, 3]);
  const changeB = packDigest([4, 4, 4, 4, 4, 4, 4, 4]);
  const settled = decodePoolLog({ topics: [SETTLED_TOPIC], data: hexWords(2n, 5n, 6n, changeA, changeB, 333n), ...meta });
  assert.equal(settled?.kind === "settled" && settled.nullifierB, 6n);
  assert.equal(decodePoolLog({ topics: ["0x00"], data: "0x", ...meta }), null);

  const trees = buildTrees([deposit!, settled!]);
  assert.deepEqual(trees.poolLeaves, [unpackDigest(note), Array(8).fill(0), unpackDigest(changeA), unpackDigest(changeB)]);
  assert.deepEqual(trees.associationLeaves, [unpackDigest(label)]);
});

test("deposit calldata", () => {
  assert.equal(
    toHex(depositCalldata([1, 2, 3, 4, 5, 6, 7, 8])),
    "0x09d009b8" + Array.from({ length: 8 }, (_, i) => (i + 1).toString(16).padStart(64, "0")).join(""),
  );
});

test("settle calldata matches the ABI encoding (cast calldata)", () => {
  const expected =
    "0xf4b589c7" +
    [0x60, 0xa0, 0x120, 5].map((n) => n.toString(16).padStart(64, "0")).join("") +
    "0102030405".padEnd(64, "0") +
    [3, 1, 2, 3, 2, 4, 5].map((n) => n.toString(16).padStart(64, "0")).join("");
  assert.equal(toHex(settleCalldata(Uint8Array.of(1, 2, 3, 4, 5), [1, 2, 3], [4, 5])), expected);
});

test("settle transaction shape", () => {
  const d: PoolDeployment = { ...deployment, pqPaymaster: true };
  const fields: [number[], number[]] = [Array(82).fill(1), Array(82).fill(2)];
  const tx = buildSettleTransaction({
    deployment: d,
    chainId: 8141n,
    fees: { maxPriorityFee: 1n, maxFee: 2n },
    proofCall: new Uint8Array(100),
    fields,
    nonceKey: 9n,
  });
  assert.equal(tx.sender, BigInt(d.pool));
  assert.deepEqual(tx.frames.map((f) => [f.mode, f.flags, f.target]), [
    [1n, 2n, BigInt(d.pool)],
    [1n, 1n, BigInt(d.paymaster)],
    [2n, 0n, BigInt(d.pool)],
  ]);
  assert.equal(tx.frames[1].gasLimit, 150_000n);
  assert.equal(tx.frames[2].gasLimit, 14_600_000n);
  assert.deepEqual(tx.frames[2].data, settleCalldata(new Uint8Array(100), fields[0], fields[1]));
  assert.deepEqual(tx.nonceKeys, [9n]);
});
