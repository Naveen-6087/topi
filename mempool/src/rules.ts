// Admission rules that need no chain access: the settle transaction's exact shape, as the pool
// account's VERIFY policy and the PQ paymaster check it on chain (contracts/yul), plus the two spends'
// public fields. A transaction that fails here would fail validation on chain too.
import {
  APPROVE,
  FIELD_MODULUS,
  MODE,
  SCHEME,
  SETTLE_SIGNATURE,
  SPEND_FIELDS,
  checkSpendFields,
  decodeWords,
  equalBytes,
  packFields,
  selector,
  type FrameTransaction,
  type PoolDeployment,
} from "../../sdk/src/index.ts";

/** A leanSPHINCS witness, as the PQ paymaster requires it. */
export const LEANSPHINCS_SIGNATURE_BYTES = 5_684;
const SETTLE_SELECTOR = selector(SETTLE_SIGNATURE);

/** Why a transaction was refused: a stable code for metrics and peers, and a message for people. */
export class Rejection extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "Rejection";
    this.code = code;
  }
}

const reject = (code: string, message: string): never => {
  throw new Rejection(code, message);
};

/** What this mempool admits. */
export interface Policy {
  chainId: bigint;
  /** The pool account: every settle's sender. */
  pool: bigint;
  /** Paymasters allowed to pay for settles (frame 1's target). */
  paymasters: bigint[];
  /** True when the paymasters are leanSPHINCS paymasters (one ARBITRARY witness). */
  pqPaymaster: boolean;
  deployment: PoolDeployment;
  /**
   * The validation budget this mempool accepts: frame 0 plus frame 1 execution limits. Our own rule,
   * looser than the public mempool's 100,000, because the leanSPHINCS check alone uses about 61,000.
   */
  maxValidationGas: bigint;
}

export function policyFor(deployment: PoolDeployment, extraPaymasters: bigint[] = [], maxValidationGas = 200_000n): Policy {
  const paymasters = [BigInt(deployment.paymaster), ...extraPaymasters].filter((v, i, all) => all.indexOf(v) === i);
  return {
    chainId: BigInt(deployment.chainId),
    pool: BigInt(deployment.pool),
    paymasters,
    pqPaymaster: deployment.pqPaymaster === true,
    deployment,
    maxValidationGas,
  };
}

/** The facts about an admissible settle that the mempool indexes and checks against the chain. */
export interface SettleSummary {
  sender: bigint;
  paymaster: bigint;
  nullifiers: [bigint, bigint];
  poolRoots: [bigint, bigint];
  associationRoots: [bigint, bigint];
  fields: [number[], number[]];
  nonceKeys: bigint[];
  maxFee: bigint;
  validationGas: bigint;
}

/** Splits settle(bytes proofCall, uint256[] a, uint256[] b) calldata into the two spends' fields. */
export function decodeSettleFields(data: Uint8Array): [number[], number[]] {
  if (data.length < 4 + 96 || !equalBytes(data.subarray(0, 4), SETTLE_SELECTOR)) reject("not-settle", "frame 2 does not call settle");
  const body = data.subarray(4);
  if (body.length % 32 !== 0) reject("bad-calldata", "settle calldata is not whole words");
  const words = decodeWords(body);
  const array = (offset: bigint): number[] => {
    const at = Number(offset / 32n);
    if (offset % 32n !== 0n || at >= words.length) reject("bad-calldata", "settle array offset out of range");
    const length = Number(words[at]);
    if (length !== SPEND_FIELDS.count || at + 1 + length > words.length) {
      reject("bad-calldata", `a spend must have ${SPEND_FIELDS.count} fields, got ${length}`);
    }
    return words.slice(at + 1, at + 1 + length).map((w) => {
      if (w >= FIELD_MODULUS) reject("bad-field", "a spend field is not below the KoalaBear modulus");
      return Number(w);
    });
  };
  return [array(words[1]), array(words[2])];
}

/** Checks everything that can be checked offline, mirroring the on-chain policies, and summarizes. */
export function checkSettle(tx: FrameTransaction, policy: Policy): SettleSummary {
  if (tx.chainId !== policy.chainId) reject("wrong-chain", `chain ${tx.chainId}, expected ${policy.chainId}`);
  if (tx.sender !== policy.pool) reject("not-pool", "the sender is not the pool");
  if (tx.frames.length !== 3) reject("shape", `expected 3 frames, got ${tx.frames.length}`);
  if (tx.blobHashes.length !== 0) reject("shape", "settles carry no blobs");
  if (tx.nonceKeys.length < 1 || tx.nonceSeq !== 0n) reject("nonce", "a settle uses a fresh nonce key at sequence 0");

  const [pool, payer, call] = tx.frames;
  if (pool.mode !== MODE.VERIFY || pool.flags !== APPROVE.EXECUTION || pool.target !== policy.pool || pool.value !== 0n) {
    reject("shape", "frame 0 must be the pool's VERIFY with execution scope and no value");
  }
  if (payer.mode !== MODE.VERIFY || payer.flags !== APPROVE.PAYMENT || payer.value !== 0n || payer.target === null) {
    reject("shape", "frame 1 must be a VERIFY with payment scope and no value");
  }
  const paymaster = payer.target!;
  if (paymaster === policy.pool) reject("shape", "the pool never pays for its own transactions");
  if (!policy.paymasters.includes(paymaster)) reject("paymaster", `paymaster 0x${paymaster.toString(16)} is not accepted here`);
  if (call.mode !== MODE.SENDER || call.flags !== 0n || call.target !== policy.pool || call.value !== 0n) {
    reject("shape", "frame 2 must be one SENDER call to the pool with no value and no flags");
  }

  if (policy.pqPaymaster) {
    const sigs = tx.signatures;
    if (sigs.length !== 1) reject("signature", `expected one signature entry, got ${sigs.length}`);
    const [sig] = sigs;
    if (sig.scheme !== SCHEME.ARBITRARY || sig.msg.length !== 0) reject("signature", "the witness must be an empty-msg ARBITRARY entry");
    if (sig.signature.length !== LEANSPHINCS_SIGNATURE_BYTES) {
      reject("signature", `a leanSPHINCS witness is ${LEANSPHINCS_SIGNATURE_BYTES} bytes, got ${sig.signature.length}`);
    }
  }

  const validationGas = pool.gasLimit + payer.gasLimit;
  if (validationGas > policy.maxValidationGas) {
    reject("validation-gas", `validation budget ${validationGas} exceeds ${policy.maxValidationGas}`);
  }

  const fields = decodeSettleFields(call.data);
  for (const f of fields) {
    // The fee recipient is any exit's: this mempool carries every exit's settles, not one operator's.
    const why = checkSpendFields(f, policy.deployment, contextOf(f));
    if (why) reject("fields", why);
  }
  const nullifiers: [bigint, bigint] = [packFields(fields[0], SPEND_FIELDS.nullifier), packFields(fields[1], SPEND_FIELDS.nullifier)];
  if (nullifiers[0] === nullifiers[1]) reject("fields", "both spends use the same nullifier");

  return {
    sender: tx.sender,
    paymaster,
    nullifiers,
    poolRoots: [packFields(fields[0], SPEND_FIELDS.poolRoot), packFields(fields[1], SPEND_FIELDS.poolRoot)],
    associationRoots: [packFields(fields[0], SPEND_FIELDS.associationRoot), packFields(fields[1], SPEND_FIELDS.associationRoot)],
    fields,
    nonceKeys: tx.nonceKeys,
    maxFee: tx.maxFee,
    validationGas,
  };
}

/** The address a spend's context digest commits to (six 28-bit limbs, low first). */
function contextOf(fields: readonly number[]): bigint {
  let v = 0n;
  for (let i = 0; i < 6; i++) v |= BigInt(fields[SPEND_FIELDS.context + i]) << BigInt(28 * i);
  return v;
}
