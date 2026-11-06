/**
 * The shielded pool: deployment records, field packing, events, the chain view a wallet
 * witnesses against, deposit calldata and the settle frame transaction.
 *
 * A pool spend is one frame transaction whose sender is the pool:
 *   frame 0  VERIFY  pool       (data 0x00000000)  policy check, APPROVE(EXECUTION)
 *   frame 1  VERIFY  paymaster  (data 0x00000000)  operator signature check, APPROVE(PAYMENT)
 *   frame 2  SENDER  pool       settle(proof, A, B)
 */
import { readFileSync } from "node:fs";

import { decodeWords, encodeBytesTail, encodeCall, encodeUintArrayTail, eventTopic, selector } from "./abi.ts";
import { concat, keccak256, word, type Hex } from "./bytes.ts";
import { signTransaction } from "./ecdsa.ts";
import { APPROVE, MODE, hasRecentRoots, randomNonceKey, type Frame, type FrameTransaction } from "./frames.ts";
import { addArbitraryWitness, type PQSigner } from "./pq.ts";
import type { FeeCaps, RpcClient, RpcLog } from "./rpc.ts";

// ---------- deployment records ----------

/** A pool deployment record (deployments/<chain>/pool*.json). */
export interface PoolDeployment {
  chainId: number;
  name: string;
  /** The terminal proof verifier the pool calls in settle. */
  terminal: Hex;
  /** The ShieldedPool logic contract. */
  logic: Hex;
  /** The pool account (frame-transaction sender for spends). */
  pool: Hex;
  /** The paymaster account that pays for spends. */
  paymaster: Hex;
  /** The exit operator: fee recipient and (classic paymaster) ECDSA signer. */
  operator: Hex;
  testbed: boolean;
  /** Pool identity, eight field elements. */
  poolId: number[];
  /** Scope identity, eight field elements. */
  scope: number[];
  /** True when the paymaster checks a leanSPHINCS witness instead of an ECDSA signature. */
  pqPaymaster?: boolean;
  exitPkSeed?: Hex;
  exitPkRoot?: Hex;
  /** The shared leanSPHINCS verifier (PQ paymaster). */
  verifier?: Hex;
}

/** A PQ account deployment record (deployments/<chain>/account.json). */
export interface AccountDeployment {
  chainId: number;
  /** The shared leanSPHINCS verifier. */
  verifier: Hex;
  frameModule: Hex;
  /** The account address. */
  account: Hex;
  pkSeed: Hex;
  pkRoot: Hex;
  /** The account's VERIFY-frame selector, 8 hex digits without 0x. */
  verifySelector: string;
}

const isAddress = (v: unknown): v is Hex => typeof v === "string" && /^0x[0-9a-fA-F]{40}$/.test(v);
const isDigest = (v: unknown): v is number[] =>
  Array.isArray(v) && v.length === 8 && v.every((x) => Number.isInteger(x) && x >= 0 && x < 2 ** 32);

/** Validates a parsed pool deployment record. */
export function parsePoolDeployment(json: unknown, what = "pool deployment"): PoolDeployment {
  const d = json as Record<string, unknown>;
  for (const key of ["terminal", "logic", "pool", "paymaster", "operator"]) {
    if (!isAddress(d[key])) throw new TypeError(`${what}: ${key} must be an address`);
  }
  if (!isDigest(d.poolId) || !isDigest(d.scope)) throw new TypeError(`${what}: poolId and scope must be 8 words`);
  if (typeof d.chainId !== "number") throw new TypeError(`${what}: chainId must be a number`);
  return d as unknown as PoolDeployment;
}

/** Validates a parsed PQ account deployment record. */
export function parseAccountDeployment(json: unknown, what = "account deployment"): AccountDeployment {
  const d = json as Record<string, unknown>;
  for (const key of ["verifier", "account"]) {
    if (!isAddress(d[key])) throw new TypeError(`${what}: ${key} must be an address`);
  }
  if (typeof d.verifySelector !== "string" || !/^[0-9a-fA-F]{8}$/.test(d.verifySelector)) {
    throw new TypeError(`${what}: verifySelector must be 8 hex digits`);
  }
  return d as unknown as AccountDeployment;
}

export function loadPoolDeployment(file: string): PoolDeployment {
  return parsePoolDeployment(JSON.parse(readFileSync(file, "utf8")), file);
}

export function loadAccountDeployment(file: string): AccountDeployment {
  return parseAccountDeployment(JSON.parse(readFileSync(file, "utf8")), file);
}

// ---------- field elements and packing ----------

/** The KoalaBear prime, the field the proofs work over. */
export const FIELD_MODULUS = 2_130_706_433n;

/** A digest (eight 32-bit field elements) packed little-endian into one uint256, as the pool stores it. */
export function packDigest(digest: readonly (number | bigint)[]): bigint {
  if (digest.length !== 8) throw new RangeError(`a digest is 8 words, got ${digest.length}`);
  let packed = 0n;
  for (let i = 0; i < 8; i++) packed |= BigInt(digest[i]) << BigInt(32 * i);
  return packed;
}

/** The inverse of packDigest. */
export function unpackDigest(packed: bigint): number[] {
  return Array.from({ length: 8 }, (_, i) => Number((packed >> BigInt(32 * i)) & 0xffffffffn));
}

/** Packs fields[start .. start+8). */
export function packFields(fields: readonly number[], start: number): bigint {
  return packDigest(fields.slice(start, start + 8));
}

/** An address as a spend's context digest: six 28-bit limbs (low first) and two zeros. */
export function contextDigest(address: bigint | Hex): number[] {
  const v = BigInt(address);
  return [...Array.from({ length: 6 }, (_, i) => Number((v >> BigInt(28 * i)) & 0xfffffffn)), 0, 0];
}

/**
 * Pool or scope identity for a new deployment: eight 32-bit words of keccak256(label), each
 * reduced mod FIELD_MODULUS. Existing deployments keep the identities in their records.
 */
export function deriveIdentity(label: string): number[] {
  const h = keccak256(label);
  return Array.from({ length: 8 }, (_, i) => {
    const w = (h[4 * i] << 24) | (h[4 * i + 1] << 16) | (h[4 * i + 2] << 8) | h[4 * i + 3];
    return Number(BigInt(w >>> 0) % FIELD_MODULUS);
  });
}

/** Layout of a spend's 82 public settle fields. */
export const SPEND_FIELDS = {
  count: 82,
  poolId: 0,
  scope: 8,
  context: 16,
  poolRoot: 50,
  associationRoot: 58,
  nullifier: 66,
} as const;

const sameDigest = (fields: readonly number[], start: number, want: readonly number[]) =>
  want.every((x, i) => fields[start + i] === x);

/**
 * Checks a spend's fields against a deployment without the chain: the pool and scope are this
 * pool's, and the fee is committed to `feeRecipient` (default: the deployment's operator).
 * Returns null when they match, or the reason they do not.
 */
export function checkSpendFields(
  fields: readonly number[],
  deployment: PoolDeployment,
  feeRecipient: bigint | Hex = deployment.operator,
): string | null {
  if (fields.length !== SPEND_FIELDS.count) return `expected ${SPEND_FIELDS.count} fields, got ${fields.length}`;
  if (!sameDigest(fields, SPEND_FIELDS.poolId, deployment.poolId)) return "not this pool";
  if (!sameDigest(fields, SPEND_FIELDS.scope, deployment.scope)) return "not this pool's scope";
  if (!sameDigest(fields, SPEND_FIELDS.context, contextDigest(feeRecipient))) return "fee is not committed to this exit";
  return null;
}

// ---------- events ----------

export const DEPOSITED_SIGNATURE = "Deposited(uint256,uint256,uint256,uint256,uint256,uint256,uint256)";
export const SETTLED_SIGNATURE = "Settled(uint256,uint256,uint256,uint256,uint256,uint256)";
export const DEPOSITED_TOPIC = eventTopic(DEPOSITED_SIGNATURE);
export const SETTLED_TOPIC = eventTopic(SETTLED_SIGNATURE);

export interface DepositedEvent {
  kind: "deposited";
  /** Association-tree index (indexed). */
  index: bigint;
  /** Pool-tree index of the note leaf; the next leaf is a zero leaf. */
  leafIndex: bigint;
  note: bigint;
  label: bigint;
  amount: bigint;
  poolRoot: bigint;
  associationRoot: bigint;
  blockNumber: bigint;
  transactionHash: Hex;
}

export interface SettledEvent {
  kind: "settled";
  /** Pool-tree index of changeA; changeB follows. */
  leafIndex: bigint;
  nullifierA: bigint;
  nullifierB: bigint;
  changeA: bigint;
  changeB: bigint;
  poolRoot: bigint;
  blockNumber: bigint;
  transactionHash: Hex;
}

export type PoolEvent = DepositedEvent | SettledEvent;

/** Decodes a Deposited or Settled log; null for any other log. */
export function decodePoolLog(log: Pick<RpcLog, "topics" | "data" | "blockNumber" | "transactionHash">): PoolEvent | null {
  const topic = log.topics[0]?.toLowerCase();
  const meta = { blockNumber: BigInt(log.blockNumber ?? 0), transactionHash: log.transactionHash };
  if (topic === DEPOSITED_TOPIC) {
    const [leafIndex, note, label, amount, poolRoot, associationRoot] = decodeWords(log.data);
    return { kind: "deposited", index: BigInt(log.topics[1]), leafIndex, note, label, amount, poolRoot, associationRoot, ...meta };
  }
  if (topic === SETTLED_TOPIC) {
    const [leafIndex, nullifierA, nullifierB, changeA, changeB, poolRoot] = decodeWords(log.data);
    return { kind: "settled", leafIndex, nullifierA, nullifierB, changeA, changeB, poolRoot, ...meta };
  }
  return null;
}

// ---------- trees and the chain view ----------

export interface PoolTrees {
  /** Pool-tree leaves in index order, each an unpacked digest. */
  poolLeaves: number[][];
  /** Association-tree leaves (deposit labels) in index order. */
  associationLeaves: number[][];
}

const ZERO_LEAF = (): number[] => Array(8).fill(0);

/** Rebuilds both trees' leaves from the pool's events (in log order). */
export function buildTrees(events: readonly PoolEvent[]): PoolTrees {
  const poolLeaves: number[][] = [];
  const associationLeaves: number[][] = [];
  const put = (index: bigint, leaf: number[]) => {
    const i = Number(index);
    while (poolLeaves.length <= i) poolLeaves.push(ZERO_LEAF());
    poolLeaves[i] = leaf;
  };
  for (const e of events) {
    if (e.kind === "deposited") {
      put(e.leafIndex, unpackDigest(e.note));
      put(e.leafIndex + 1n, ZERO_LEAF());
      associationLeaves[Number(e.index)] = unpackDigest(e.label);
    } else {
      put(e.leafIndex, unpackDigest(e.changeA));
      put(e.leafIndex + 1n, unpackDigest(e.changeB));
    }
  }
  for (let i = 0; i < associationLeaves.length; i++) associationLeaves[i] ??= ZERO_LEAF();
  return { poolLeaves, associationLeaves };
}

/** What `pool-wallet witness` reads: both trees, the current roots and the pool's identity. */
export interface ChainView {
  pool: number[];
  scope: number[];
  pool_leaves: number[][];
  association_leaves: number[][];
  pool_root: number[];
  association_root: number[];
  next_leaf: number;
}

/** Read-only access to a deployed pool. */
export class PoolReader {
  readonly rpc: RpcClient;
  readonly address: Hex;

  constructor(rpc: RpcClient, address: Hex) {
    this.rpc = rpc;
    this.address = address;
  }

  private async word(signature: string, args: bigint[] = []): Promise<bigint> {
    const [value] = decodeWords(await this.rpc.call(this.address, encodeCall(signature, args)));
    if (value === undefined) throw new Error(`${signature}: empty return data from ${this.address}`);
    return value;
  }

  poolRoot(): Promise<bigint> {
    return this.word("poolRoot()");
  }
  associationRoot(): Promise<bigint> {
    return this.word("associationRoot()");
  }
  nextPoolIndex(): Promise<bigint> {
    return this.word("nextPoolIndex()");
  }
  async knownPoolRoot(packedRoot: bigint): Promise<boolean> {
    return (await this.word("knownPoolRoot(uint256)", [packedRoot])) !== 0n;
  }
  async knownAssociationRoot(packedRoot: bigint): Promise<boolean> {
    return (await this.word("knownAssociationRoot(uint256)", [packedRoot])) !== 0n;
  }
  async spent(packedNullifier: bigint): Promise<boolean> {
    return (await this.word("spent(uint256)", [packedNullifier])) !== 0n;
  }

  /** Every Deposited and Settled event from `fromBlock`. */
  async events(fromBlock: bigint | "earliest" = "earliest"): Promise<PoolEvent[]> {
    const logs = await this.rpc.getLogs({
      address: this.address,
      fromBlock,
      toBlock: "latest",
      topics: [[DEPOSITED_TOPIC, SETTLED_TOPIC]],
    });
    return logs.map(decodePoolLog).filter((e): e is PoolEvent => e !== null);
  }

  /**
   * Checks a spend against the pool's state: both roots known and the nullifier unspent.
   * Returns null when it can settle, or the reason it cannot.
   */
  async checkSpendState(fields: readonly number[]): Promise<string | null> {
    if (!(await this.knownPoolRoot(packFields(fields, SPEND_FIELDS.poolRoot)))) return "unknown pool root";
    if (!(await this.knownAssociationRoot(packFields(fields, SPEND_FIELDS.associationRoot)))) return "unknown association root";
    if (await this.spent(packFields(fields, SPEND_FIELDS.nullifier))) return "nullifier already spent";
    return null;
  }

  /**
   * Rebuilds the chain view from the pool's logs and current roots. Throws if the logs do not
   * account for every pool leaf.
   */
  async chainView(deployment: PoolDeployment, fromBlock: bigint | "earliest" = "earliest"): Promise<ChainView> {
    const { poolLeaves, associationLeaves } = buildTrees(await this.events(fromBlock));
    const [poolRoot, associationRoot, next] = await Promise.all([this.poolRoot(), this.associationRoot(), this.nextPoolIndex()]);
    if (Number(next) !== poolLeaves.length) {
      throw new Error(`logs give ${poolLeaves.length} pool leaves, the pool says ${next}`);
    }
    return {
      pool: deployment.poolId,
      scope: deployment.scope,
      pool_leaves: poolLeaves,
      association_leaves: associationLeaves,
      pool_root: unpackDigest(poolRoot),
      association_root: unpackDigest(associationRoot),
      next_leaf: Number(next),
    };
  }
}

// ---------- calldata ----------

export const DEPOSIT_SIGNATURE = "deposit(uint256[8])";
export const SETTLE_SIGNATURE = "settle(bytes,uint256[],uint256[])";

/** deposit(precommitment): selector and eight words. */
export function depositCalldata(precommitment: readonly (number | bigint)[]): Uint8Array {
  if (precommitment.length !== 8) throw new RangeError(`a precommitment is 8 words, got ${precommitment.length}`);
  return encodeCall(DEPOSIT_SIGNATURE, precommitment);
}

/** settle(proofCall, a, b), where proofCall is the terminal verifier calldata. */
export function settleCalldata(proofCall: Uint8Array, a: readonly number[], b: readonly number[]): Uint8Array {
  const bytesPart = encodeBytesTail(proofCall);
  const offsetA = 96 + bytesPart.length;
  const offsetB = offsetA + 32 * (a.length + 1);
  return concat(
    selector(SETTLE_SIGNATURE),
    word(96),
    word(offsetA),
    word(offsetB),
    bytesPart,
    encodeUintArrayTail(a),
    encodeUintArrayTail(b),
  );
}

// ---------- the settle transaction ----------

/** Per-frame limits of a settle transaction. */
export interface SettleLimits {
  poolGas: bigint;
  poolState: bigint;
  paymasterGas: bigint;
  paymasterState: bigint;
  settleGas: bigint;
  settleState: bigint;
}

/**
 * Limits measured on chain 8141: settle needs about 14.25M execution plus EIP-150 headroom; the
 * PQ paymaster about 61k, the classic one far less.
 */
export function defaultSettleLimits(pqPaymaster: boolean): SettleLimits {
  return {
    poolGas: 10_000n,
    poolState: 0n,
    paymasterGas: pqPaymaster ? 150_000n : 60_000n,
    paymasterState: 110_000n,
    settleGas: 14_600_000n,
    settleState: 5_000_000n,
  };
}

export interface SettleTransactionParams {
  deployment: PoolDeployment;
  chainId: bigint;
  fees: FeeCaps;
  /** Terminal verifier calldata (calldata.bin from the exit prover). */
  proofCall: Uint8Array;
  /** The two spends' public settle fields. */
  fields: [readonly number[], readonly number[]];
  limits?: Partial<SettleLimits>;
  nonceKey?: bigint;
}

/** The unsigned [VERIFY pool, VERIFY paymaster, SENDER settle] transaction. */
export function buildSettleTransaction(params: SettleTransactionParams): FrameTransaction {
  const { deployment } = params;
  const limits = { ...defaultSettleLimits(deployment.pqPaymaster === true), ...params.limits };
  const pool = BigInt(deployment.pool);
  const zero4 = new Uint8Array(4);
  const frames: Frame[] = [
    { mode: MODE.VERIFY, flags: APPROVE.EXECUTION, target: pool, gasLimit: limits.poolGas, stateLimit: limits.poolState, value: 0n, data: zero4 },
    {
      mode: MODE.VERIFY,
      flags: APPROVE.PAYMENT,
      target: BigInt(deployment.paymaster),
      gasLimit: limits.paymasterGas,
      stateLimit: limits.paymasterState,
      value: 0n,
      data: zero4,
    },
    {
      mode: MODE.SENDER,
      flags: 0n,
      target: pool,
      gasLimit: limits.settleGas,
      stateLimit: limits.settleState,
      value: 0n,
      data: settleCalldata(params.proofCall, params.fields[0], params.fields[1]),
    },
  ];
  return {
    chainId: params.chainId,
    nonceKeys: [params.nonceKey ?? randomNonceKey()],
    nonceSeq: 0n,
    sender: pool,
    frames,
    signatures: [],
    maxPriorityFee: params.fees.maxPriorityFee,
    maxFee: params.fees.maxFee,
    maxBlobFee: 0n,
    blobHashes: [],
    ...(hasRecentRoots(params.chainId) ? { recentRootReferences: [] as [] } : {}),
  };
}

/** The paymaster operator's authorization: an ECDSA key (classic paymaster) or a PQ signer. */
export type OperatorSigner = { ecdsaKey: Uint8Array } | { pqSigner: PQSigner };

/**
 * Signs a settle transaction as the paymaster's operator: a leanSPHINCS ARBITRARY witness for a
 * PQ paymaster, a secp256k1 signature otherwise. Mutates and returns `tx`.
 */
export async function signSettleTransaction(
  tx: FrameTransaction,
  deployment: PoolDeployment,
  signer: OperatorSigner,
): Promise<FrameTransaction> {
  if (deployment.pqPaymaster === true) {
    if (!("pqSigner" in signer)) throw new TypeError("this deployment's paymaster takes a leanSPHINCS witness");
    tx.signatures = [];
    return addArbitraryWitness(tx, signer.pqSigner);
  }
  if (!("ecdsaKey" in signer)) throw new TypeError("this deployment's paymaster takes an ECDSA signature");
  return signTransaction(tx, signer.ecdsaKey);
}
