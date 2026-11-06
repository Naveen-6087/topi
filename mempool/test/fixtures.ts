// A synthetic settle with valid shape, and a chain that tests can steer.
import { readFileSync } from "node:fs";

import {
  SCHEME,
  SPEND_FIELDS,
  buildSettleTransaction,
  contextDigest,
  encodeRaw,
  keccak256,
  parsePoolDeployment,
  randomBytes,
  toHex,
  type FrameSimulation,
  type FrameTransaction,
  type Hex,
  type TransactionReceipt,
} from "../../sdk/src/index.ts";

import type { Chain } from "../src/chain.ts";
import { LEANSPHINCS_SIGNATURE_BYTES, policyFor } from "../src/rules.ts";

export const deployment = parsePoolDeployment(
  JSON.parse(readFileSync(new URL("../../deployments/8141/pool.json", import.meta.url), "utf8")),
);
export const policy = policyFor(deployment);

/** A spend's 82 fields: this pool, its scope, the operator as fee recipient, and a given nullifier. */
export function spendFields(nullifier: number, root = 7): number[] {
  const f = new Array<number>(SPEND_FIELDS.count).fill(0);
  deployment.poolId.forEach((v, i) => (f[SPEND_FIELDS.poolId + i] = v));
  deployment.scope.forEach((v, i) => (f[SPEND_FIELDS.scope + i] = v));
  contextDigest(deployment.operator).forEach((v, i) => (f[SPEND_FIELDS.context + i] = v));
  f[SPEND_FIELDS.poolRoot] = root;
  f[SPEND_FIELDS.associationRoot] = root + 1;
  f[SPEND_FIELDS.nullifier] = nullifier;
  return f;
}

/** An unsigned-but-witnessed settle; `edit` may break it for negative cases. */
export function settleTx(n = 1, edit?: (tx: FrameTransaction) => void): FrameTransaction {
  const tx = buildSettleTransaction({
    deployment,
    chainId: BigInt(deployment.chainId),
    fees: { maxPriorityFee: 1_000_000_000n, maxFee: 2_000_000_000n },
    proofCall: randomBytes(256),
    fields: [spendFields(2 * n), spendFields(2 * n + 1)],
  });
  tx.signatures = [{ scheme: SCHEME.ARBITRARY, signer: 0n, msg: new Uint8Array(0), signature: randomBytes(LEANSPHINCS_SIGNATURE_BYTES) }];
  edit?.(tx);
  return tx;
}

export const rawSettle = (n = 1, edit?: (tx: FrameTransaction) => void) => encodeRaw(settleTx(n, edit));
export const hashOf = (raw: Uint8Array) => toHex(keccak256(raw));

/** A chain whose answers the test sets. */
export class FakeChain implements Chain {
  block = 100n;
  spentSet = new Set<bigint>();
  unknownRoots = new Set<bigint>();
  codeHashes = new Map<bigint, Hex>();
  witnessOk = true;
  simulation: FrameSimulation = { valid: true, violation: null, executionStatus: "success", executionError: null };
  sent: Hex[] = [];
  receipts = new Map<string, TransactionReceipt>();
  refuse: string | null = null;

  async blockNumber() {
    return this.block;
  }
  async codeHash(address: bigint): Promise<Hex> {
    return this.codeHashes.get(address) ?? (`0x${"ab".repeat(31)}${(address % 256n).toString(16).padStart(2, "0")}` as Hex);
  }
  async spent(_pool: bigint, n: bigint) {
    return this.spentSet.has(n);
  }
  async knownPoolRoot(_pool: bigint, r: bigint) {
    return !this.unknownRoots.has(r);
  }
  async knownAssociationRoot(_pool: bigint, r: bigint) {
    return !this.unknownRoots.has(r);
  }
  async verifyLeanSphincs() {
    return this.witnessOk;
  }
  async simulate() {
    return this.simulation;
  }
  async send(raw: Uint8Array) {
    if (this.refuse) throw new Error(this.refuse);
    const hash = hashOf(raw);
    this.sent.push(hash);
    return hash;
  }
  async receipt(hash: Hex) {
    return this.receipts.get(hash) ?? null;
  }
  mine(hash: Hex, status: "0x1" | "0x0" = "0x1") {
    this.receipts.set(hash, { transactionHash: hash, status, blockNumber: `0x${(++this.block).toString(16)}`, gasUsed: "0x0", logs: [] });
  }
}
