import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  SCHEME,
  addressOf,
  decodeRaw,
  encodeRaw,
  executionCapUsage,
  fromHex,
  keccak256,
  recoverSigner,
  signTransaction,
  signatureHash,
  toHex,
  totalGasLimit,
  type FrameTransaction,
} from "../src/index.ts";
import { TEST_KEY, cases } from "./frame-cases.ts";

// Recorded from the reference implementation this module was ported from (byte-identical at
// the time of the port); see frame-cases.ts for the transactions.
const vectors = JSON.parse(readFileSync(new URL("./frame-vectors.json", import.meta.url), "utf8")) as Record<
  string,
  { rawLength: number; rawKeccak: string; raw?: string; signatureHash: string; executionCapUsage: string; totalGasLimit: string }
>;
const built = cases<FrameTransaction>({ signTransaction, sigHash: signatureHash });

test("every recorded case has a builder", () => {
  assert.deepEqual(Object.keys(built).sort(), Object.keys(vectors).sort());
});

for (const [name, build] of Object.entries(built)) {
  test(`encoding matches the recorded vector: ${name}`, () => {
    const tx = build();
    const raw = encodeRaw(tx);
    const v = vectors[name];
    assert.equal(raw.length, v.rawLength);
    assert.equal(toHex(keccak256(raw)), v.rawKeccak);
    if (v.raw) assert.equal(toHex(raw), v.raw);
    assert.equal(toHex(signatureHash(tx)), v.signatureHash);
    assert.equal(executionCapUsage(tx).toString(), v.executionCapUsage);
    assert.equal(totalGasLimit(tx).toString(), v.totalGasLimit);
  });

  test(`decode then re-encode is the identity: ${name}`, () => {
    const tx = build();
    const raw = encodeRaw(tx);
    const decoded = decodeRaw(raw);
    assert.deepEqual(encodeRaw(decoded), raw);
    assert.deepEqual(signatureHash(decoded), signatureHash(tx));
  });
}

test("the signature hash ignores the bytes of empty-msg signatures", () => {
  const tx = built["ARBITRARY witness (PQ account)"]();
  const before = signatureHash(tx);
  tx.signatures[0].signature = new Uint8Array(0);
  assert.deepEqual(signatureHash(tx), before);
  tx.signatures[0].signature = new Uint8Array(5_684).fill(0xff);
  assert.deepEqual(signatureHash(tx), before);
  // but the hash covers everything else
  tx.frames[1].value += 1n;
  assert.notDeepEqual(signatureHash(tx), before);
});

test("ARBITRARY entries encode an empty signer whatever the signer field says", () => {
  const tx = built["ARBITRARY witness (PQ account)"]();
  const raw = encodeRaw(tx);
  tx.signatures[0].signer = 0x1234n;
  assert.deepEqual(encodeRaw(tx), raw);
  assert.equal(decodeRaw(raw).signatures[0].scheme, SCHEME.ARBITRARY);
  assert.equal(decodeRaw(raw).signatures[0].signer, 0n);
});

test("an ECDSA entry recovers to the signing key's address", () => {
  const tx = built["ECDSA then ARBITRARY witness"]();
  const [ecdsa, witness] = tx.signatures;
  assert.equal(ecdsa.scheme, SCHEME.SECP256K1);
  assert.equal(ecdsa.signer, addressOf(TEST_KEY));
  assert.equal(recoverSigner(signatureHash(tx), ecdsa.signature), addressOf(TEST_KEY));
  assert.equal(witness.scheme, SCHEME.ARBITRARY);
});

test("decodeRaw rejects other transaction types and malformed envelopes", () => {
  assert.throws(() => decodeRaw(fromHex("0x02c0")));
  assert.throws(() => decodeRaw(fromHex("0x06c3010203")));
});
