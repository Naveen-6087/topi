import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  LEANSPHINCS_SIGNATURE_BYTES,
  SCHEME,
  addArbitraryWitness,
  buildAccountTransaction,
  decodeLskSignature,
  eip1559SigningHash,
  encodeRaw,
  fromHex,
  keccak256,
  readSpendQueue,
  signEip1559,
  signTransaction,
  signatureHash,
  toHex,
  toWslPath,
  word,
  concat,
  type PQSigner,
} from "../src/index.ts";
import { TEST_KEY } from "./frame-cases.ts";

/** A deterministic stand-in signer: the witness is the digest repeated. */
const fakeSigner = (): PQSigner & { signed: Uint8Array[] } => {
  const signed: Uint8Array[] = [];
  return {
    signed,
    publicKey: async () => ({ pkSeed: `0x${"11".repeat(32)}`, pkRoot: `0x${"22".repeat(32)}` }),
    sign: async (digest) => {
      signed.push(digest);
      const out = new Uint8Array(LEANSPHINCS_SIGNATURE_BYTES);
      for (let i = 0; i < out.length; i++) out[i] = digest[i % 32];
      return out;
    },
  };
};

const accountParams = () => ({
  account: "0xB6f6Dc5f1139568fB72e34dE4174C9496a6C7872" as const,
  verifySelector: "82f4ffda",
  chainId: 8141n,
  fees: { maxPriorityFee: 1_000_000_000n, maxFee: 3_000_000_000n },
  call: { target: 0xdeadn, value: 1n, data: new Uint8Array(0), gasLimit: 50_000n, stateLimit: 0n },
  nonceKey: 42n,
});

test("account transaction: VERIFY(account, flags 3) then SENDER, with an empty ARBITRARY entry", () => {
  const tx = buildAccountTransaction(accountParams());
  assert.equal(tx.frames.length, 2);
  assert.equal(tx.frames[0].flags, 3n);
  assert.equal(toHex(tx.frames[0].data), "0x82f4ffda");
  assert.equal(tx.signatures.length, 1);
  assert.equal(tx.signatures[0].scheme, SCHEME.ARBITRARY);
  assert.equal(tx.signatures[0].signature.length, 0);
});

test("addArbitraryWitness signs the signature hash and leaves it unchanged", async () => {
  const signer = fakeSigner();
  const tx = buildAccountTransaction(accountParams());
  const hash = signatureHash(tx);
  await addArbitraryWitness(tx, signer);
  assert.deepEqual(signer.signed, [hash]);
  assert.deepEqual(signatureHash(tx), hash);
  assert.equal(tx.signatures[0].signature.length, LEANSPHINCS_SIGNATURE_BYTES);
});

test("addArbitraryWitness after an ECDSA signature fills the trailing placeholder", async () => {
  const signer = fakeSigner();
  const tx = buildAccountTransaction(accountParams());
  signTransaction(tx, TEST_KEY, ...tx.signatures);
  await addArbitraryWitness(tx, signer);
  assert.deepEqual(tx.signatures.map((s) => s.scheme), [SCHEME.SECP256K1, SCHEME.ARBITRARY]);
  assert.equal(tx.signatures[1].signature.length, LEANSPHINCS_SIGNATURE_BYTES);
});

test("addArbitraryWitness appends a placeholder when there is none", async () => {
  const tx = buildAccountTransaction(accountParams());
  tx.signatures = [];
  await addArbitraryWitness(tx, fakeSigner());
  assert.equal(tx.signatures.length, 1);
  assert.ok(encodeRaw(tx).length > LEANSPHINCS_SIGNATURE_BYTES);
});

test("lsk output decoding: abi.encode(bytes32, bytes32, bytes)", () => {
  const sig = new Uint8Array(LEANSPHINCS_SIGNATURE_BYTES).map((_, i) => i & 0xff);
  const padding = new Uint8Array((32 - (sig.length % 32)) % 32);
  const encoded = concat(word(1n), word(2n), word(96n), word(BigInt(sig.length)), sig, padding);
  assert.deepEqual(decodeLskSignature(toHex(encoded)), sig);
  const short = concat(word(1n), word(2n), word(96n), word(3n), new Uint8Array(32));
  assert.throws(() => decodeLskSignature(toHex(short)));
});

test("EIP-1559 signing matches cast mktx", () => {
  const key = new Uint8Array(32).fill(1);
  const raw = signEip1559(
    {
      chainId: 8141n,
      nonce: 7n,
      maxPriorityFee: 1_000_000_000n,
      maxFee: 3_000_000_014n,
      gasLimit: 15_500_000n,
      to: 0x66268c9e7735f331c48c30eecf2fe2e76b80a9d7n,
      value: 10_000_000_000_000_000n,
      data: fromHex("0x09d009b8" + Array.from({ length: 8 }, (_, i) => (i + 1).toString(16).padStart(64, "0")).join("")),
    },
    key,
  );
  assert.equal(
    toHex(keccak256(raw)),
    toHex(
      keccak256(
        fromHex(
          "0x02f9017a821fcd07843b9aca0084b2d05e0e83ec82e09466268c9e7735f331c48c30eecf2fe2e76b80a9d7872386f26fc10000b9010409d009b8" +
            Array.from({ length: 8 }, (_, i) => (i + 1).toString(16).padStart(64, "0")).join("") +
            "c001a066b58a262dc4165cf06d8ca6dc02f44a038ba0fe370eef72323e43faa9ddf50ba024c3cd250c08f5bb6037d9c795d4b93f6a79d183dea703a22484b809eb20fac3",
        ),
      ),
    ),
  );
  assert.equal(eip1559SigningHash({ chainId: 1n, nonce: 0n, maxPriorityFee: 0n, maxFee: 0n, gasLimit: 0n, to: null, value: 0n, data: new Uint8Array(0) }).length, 32);
});

test("WSL path conversion", () => {
  assert.equal(toWslPath("C:\\Users\\x\\repo\\a b.json"), "/mnt/c/Users/x/repo/a b.json");
  assert.equal(toWslPath("D:/data/q"), "/mnt/d/data/q");
});

test("spend queue: ID.json with a matching ID.tgz", () => {
  const dir = mkdtempSync(join(tmpdir(), "queue-"));
  try {
    writeFileSync(join(dir, "b.json"), JSON.stringify({ fields: [1, 2, 3] }));
    writeFileSync(join(dir, "b.tgz"), "");
    writeFileSync(join(dir, "a.json"), JSON.stringify({ fields: [4] }));
    writeFileSync(join(dir, "a.tgz"), "");
    writeFileSync(join(dir, "c.json"), JSON.stringify({ fields: [5] })); // no proof yet
    const queue = readSpendQueue(dir);
    assert.deepEqual(queue.map((s) => s.id), ["a", "b"]);
    assert.deepEqual(queue[1].fields, [1, 2, 3]);
    assert.deepEqual(readSpendQueue(join(dir, "missing")), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
