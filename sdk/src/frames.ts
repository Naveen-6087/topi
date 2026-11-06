/**
 * EIP-8141 frame transactions (type 0x06), as implemented by ethrex on chain 8141:
 *
 *   raw       = 0x06 || rlp([chain_id, nonce_keys, nonce_seq, sender, frames, signatures,
 *                            fees, blob_hashes])
 *   fees      = [max_priority_fee, max_fee, max_blob_fee]
 *   frame     = [mode, flags, target or empty, [execution, state], value, data]
 *   signature = [scheme, signer, msg, signature bytes]
 *   sig_hash  = keccak256(0x06 || rlp(envelope with empty-msg signatures' bytes elided))
 *
 * ARBITRARY signature entries carry an empty signer (EIP-8141): bespoke schemes such as
 * leanSPHINCS put their witness in the signature bytes and verify it in a VERIFY frame.
 *
 * Portions derived from soispoke/minimal-shielded-pool (Apache-2.0); see NOTICE.
 */
import { address20, concat, fromHex, keccak256, maxBigint, randomBytes, toBigint, toHex, type Hex } from "./bytes.ts";
import { rlpBytes, rlpDecode, rlpInt, rlpList, type RlpItem } from "./rlp.ts";

/** The transaction type byte. */
export const FRAME_TX_TYPE = 0x06;

/** Frame modes: DEFAULT runs as the entry point, VERIFY validates, SENDER runs as the sender. */
export const MODE = { DEFAULT: 0n, VERIFY: 1n, SENDER: 2n } as const;

/**
 * Approval scope in bits 0 and 1 of a VERIFY frame's flags: EXECUTION lets later frames act as
 * the sender, PAYMENT makes the frame's target pay for the transaction.
 */
export const APPROVE = { NONE: 0n, PAYMENT: 1n, EXECUTION: 2n, EXECUTION_AND_PAYMENT: 3n } as const;

/** Bit 2 of a frame's flags: the atomic-batch flag. */
export const ATOMIC_BATCH = 4n;

/** Signature schemes. ARBITRARY entries have an empty signer and are checked by a VERIFY frame. */
export const SCHEME = { ARBITRARY: 0n, SECP256K1: 1n, P256: 2n } as const;

/** Intrinsic verification cost per scheme, indexed by scheme. */
export const SIGNATURE_VERIFICATION_COST: readonly bigint[] = [100n, 2_800n, 6_700n];

/** The EIP-7825 per-transaction execution gas cap (2^24). */
export const EXECUTION_GAS_CAP = 1n << 24n;

export interface Frame {
  mode: bigint;
  flags: bigint;
  /** null is a targetless frame, encoded as the empty string. */
  target: bigint | null;
  /** The frame's execution gas limit. */
  gasLimit: bigint;
  /** The frame's EIP-8037 state gas budget. */
  stateLimit: bigint;
  value: bigint;
  data: Uint8Array;
}

export interface FrameSignature {
  scheme: bigint;
  /** Ignored (encoded empty) for ARBITRARY entries. */
  signer: bigint;
  msg: Uint8Array;
  /** secp256k1: v (0 or 1) || r || s. ARBITRARY: the scheme's witness. Empty before signing. */
  signature: Uint8Array;
}

export interface FrameTransaction {
  chainId: bigint;
  nonceKeys: bigint[];
  nonceSeq: bigint;
  sender: bigint;
  frames: Frame[];
  signatures: FrameSignature[];
  maxPriorityFee: bigint;
  maxFee: bigint;
  maxBlobFee: bigint;
  blobHashes: Uint8Array[];
  /**
   * EIP-8272 recent-root references. Present only on chains that encode them (the ERC-8421 devnet,
   * chain 1337), whose frame transactions have a ninth field; absent, the transaction has 8 fields
   * (chain 8141). Only the empty list is supported here.
   */
  recentRootReferences?: [];
}

/** Whether a chain's frame transactions carry EIP-8272 recent-root references (9 fields). */
export const hasRecentRoots = (chainId: bigint) => chainId === 1337n;

/** A fresh random 128-bit nonce key. */
export function randomNonceKey(): bigint {
  return toBigint(randomBytes(16));
}

/** An ARBITRARY entry with empty msg and signature, to be filled with a witness after hashing. */
export function arbitrarySignaturePlaceholder(): FrameSignature {
  return { scheme: SCHEME.ARBITRARY, signer: 0n, msg: new Uint8Array(0), signature: new Uint8Array(0) };
}

const signerBytes = (sig: FrameSignature) =>
  sig.scheme === SCHEME.ARBITRARY ? new Uint8Array(0) : address20(sig.signer);

/** One frame's RLP. */
export function encodeFrame(frame: Frame): Uint8Array {
  return rlpList([
    rlpInt(frame.mode),
    rlpInt(frame.flags),
    rlpBytes(frame.target === null ? new Uint8Array(0) : address20(frame.target)),
    rlpList([rlpInt(frame.gasLimit), rlpInt(frame.stateLimit)]),
    rlpInt(frame.value),
    rlpBytes(frame.data),
  ]);
}

function envelope(tx: FrameTransaction, elide: boolean): Uint8Array {
  const signature = (sig: FrameSignature) =>
    rlpList([
      rlpInt(sig.scheme),
      rlpBytes(signerBytes(sig)),
      rlpBytes(sig.msg),
      rlpBytes(elide && sig.msg.length === 0 ? new Uint8Array(0) : sig.signature),
    ]);
  return rlpList([
    rlpInt(tx.chainId),
    rlpList(tx.nonceKeys.map(rlpInt)),
    rlpInt(tx.nonceSeq),
    rlpBytes(address20(tx.sender)),
    rlpList(tx.frames.map(encodeFrame)),
    rlpList(tx.signatures.map(signature)),
    rlpList([rlpInt(tx.maxPriorityFee), rlpInt(tx.maxFee), rlpInt(tx.maxBlobFee)]),
    rlpList(tx.blobHashes.map(rlpBytes)),
    ...(tx.recentRootReferences ? [rlpList([])] : []),
  ]);
}

/** The envelope's RLP with every signature in place, without the type byte. */
export function encodeEnvelope(tx: FrameTransaction): Uint8Array {
  return envelope(tx, false);
}

/** The bytes eth_sendRawTransaction takes: 0x06 || envelope. */
export function encodeRaw(tx: FrameTransaction): Uint8Array {
  return concat(Uint8Array.of(FRAME_TX_TYPE), envelope(tx, false));
}

/** encodeRaw as 0x-hex. */
export function encodeRawHex(tx: FrameTransaction): Hex {
  return toHex(encodeRaw(tx));
}

/** What every signer signs: keccak256 of the raw transaction with empty-msg signatures elided. */
export function signatureHash(tx: FrameTransaction): Uint8Array {
  return keccak256(concat(Uint8Array.of(FRAME_TX_TYPE), envelope(tx, true)));
}

// ---------- decoding ----------

const bytesOf = (item: RlpItem, what: string): Uint8Array => {
  if (!(item instanceof Uint8Array)) throw new TypeError(`${what}: expected a byte string`);
  return item;
};
const listOf = (item: RlpItem, what: string, length?: number): RlpItem[] => {
  if (item instanceof Uint8Array) throw new TypeError(`${what}: expected a list`);
  if (length !== undefined && item.length !== length) throw new TypeError(`${what}: expected ${length} items`);
  return item;
};
const intOf = (item: RlpItem, what: string): bigint => {
  const bytes = bytesOf(item, what);
  if (bytes.length > 0 && bytes[0] === 0) throw new RangeError(`${what}: integer with leading zero`);
  return toBigint(bytes);
};
const addressOf = (item: RlpItem, what: string): bigint => {
  const bytes = bytesOf(item, what);
  if (bytes.length !== 20) throw new RangeError(`${what}: expected 20 bytes`);
  return toBigint(bytes);
};

/** Decodes a raw frame transaction (0x06 || envelope). */
export function decodeRaw(raw: Uint8Array): FrameTransaction {
  if (raw[0] !== FRAME_TX_TYPE) throw new TypeError(`not a frame transaction (type ${raw[0]})`);
  const fields = listOf(rlpDecode(raw.subarray(1)), "envelope");
  if (fields.length !== 8 && fields.length !== 9) throw new TypeError(`envelope: expected 8 or 9 items, got ${fields.length}`);
  const [chainId, nonceKeys, nonceSeq, sender, frames, signatures, fees, blobHashes, roots] = fields;
  if (roots !== undefined && listOf(roots, "recent_root_references").length > 0) {
    throw new RangeError("recent root references are not supported");
  }
  const [maxPriorityFee, maxFee, maxBlobFee] = listOf(fees, "fees", 3);
  return {
    chainId: intOf(chainId, "chain_id"),
    nonceKeys: listOf(nonceKeys, "nonce_keys").map((k) => intOf(k, "nonce key")),
    nonceSeq: intOf(nonceSeq, "nonce_seq"),
    sender: addressOf(sender, "sender"),
    frames: listOf(frames, "frames").map((f, i) => {
      const [mode, flags, target, limits, value, data] = listOf(f, `frame ${i}`, 6);
      const [gasLimit, stateLimit] = listOf(limits, `frame ${i} limits`, 2);
      const targetBytes = bytesOf(target, `frame ${i} target`);
      return {
        mode: intOf(mode, `frame ${i} mode`),
        flags: intOf(flags, `frame ${i} flags`),
        target: targetBytes.length === 0 ? null : addressOf(target, `frame ${i} target`),
        gasLimit: intOf(gasLimit, `frame ${i} execution limit`),
        stateLimit: intOf(stateLimit, `frame ${i} state limit`),
        value: intOf(value, `frame ${i} value`),
        data: bytesOf(data, `frame ${i} data`),
      };
    }),
    signatures: listOf(signatures, "signatures").map((s, i) => {
      const [scheme, signer, msg, signature] = listOf(s, `signature ${i}`, 4);
      const signerBytes = bytesOf(signer, `signature ${i} signer`);
      return {
        scheme: intOf(scheme, `signature ${i} scheme`),
        signer: signerBytes.length === 0 ? 0n : addressOf(signer, `signature ${i} signer`),
        msg: bytesOf(msg, `signature ${i} msg`),
        signature: bytesOf(signature, `signature ${i} bytes`),
      };
    }),
    maxPriorityFee: intOf(maxPriorityFee, "max_priority_fee"),
    maxFee: intOf(maxFee, "max_fee"),
    maxBlobFee: intOf(maxBlobFee, "max_blob_fee"),
    blobHashes: listOf(blobHashes, "blob_hashes").map((h) => bytesOf(h, "blob hash")),
    ...(roots === undefined ? {} : { recentRootReferences: [] as [] }),
  };
}

// ---------- gas: a client-side mirror of EIP-8141 max_gas ----------

const sum = <T>(items: readonly T[], f: (item: T) => bigint) => items.reduce((total, item) => total + f(item), 0n);

/** The fields EIP-8141 prices as data: frame data, signature signer/msg/bytes, nonce keys + seq. */
function dataFields(tx: FrameTransaction): Uint8Array[] {
  return [
    ...tx.frames.map((frame) => frame.data),
    ...tx.signatures.flatMap((sig) => [signerBytes(sig), sig.msg, sig.signature]),
    concat(rlpList(tx.nonceKeys.map(rlpInt)), rlpInt(tx.nonceSeq)),
  ];
}

function calldataGas(tx: FrameTransaction): bigint {
  return sum(dataFields(tx), (field) => field.reduce((gas, byte) => gas + (byte ? 16n : 4n), 0n));
}

/** What EIP-8141 charges to verify the signatures. */
export function signatureVerificationCost(tx: FrameTransaction): bigint {
  return sum(tx.signatures, (sig) => {
    const cost = SIGNATURE_VERIFICATION_COST[Number(sig.scheme)];
    if (cost === undefined) throw new RangeError(`unknown signature scheme ${sig.scheme}`);
    return cost;
  });
}

/** 6,000 per frame that moves value to an account other than the sender. */
export function valueTransferCost(tx: FrameTransaction): bigint {
  const moves = (frame: Frame) => frame.value > 0n && frame.target !== null && frame.target !== tx.sender;
  return 6_000n * BigInt(tx.frames.filter(moves).length);
}

/** Intrinsic 12,000, 475 per frame, signature verification and value transfers. */
export function mandatoryGas(tx: FrameTransaction): bigint {
  return 12_000n + 475n * BigInt(tx.frames.length) + signatureVerificationCost(tx) + valueTransferCost(tx);
}

/** The declared EIP-8037 state budgets of all frames. */
export function stateGasLimit(tx: FrameTransaction): bigint {
  return sum(tx.frames, (frame) => frame.stateLimit);
}

function executionGas(tx: FrameTransaction): bigint {
  return mandatoryGas(tx) + calldataGas(tx) + sum(tx.frames, (frame) => frame.gasLimit);
}

/** Mandatory gas, calldata gas and every frame's execution and state budget. */
export function standardGasLimit(tx: FrameTransaction): bigint {
  return executionGas(tx) + stateGasLimit(tx);
}

/** Mandatory gas plus the EIP-7976 floor: 64 gas per data byte. */
export function calldataFloorGas(tx: FrameTransaction): bigint {
  return mandatoryGas(tx) + 64n * sum(dataFields(tx), (field) => BigInt(field.length));
}

/** Declared execution against the EIP-7825 cap (EXECUTION_GAS_CAP). State is separate. */
export function executionCapUsage(tx: FrameTransaction): bigint {
  return maxBigint(executionGas(tx), calldataFloorGas(tx));
}

/** EIP-8141 max_gas = max(standard_gas_limit, calldata_floor_gas + sum(limits.state)). */
export function totalGasLimit(tx: FrameTransaction): bigint {
  return maxBigint(standardGasLimit(tx), calldataFloorGas(tx) + stateGasLimit(tx));
}

/** The most the transaction can cost its payer at the given blob base fee. */
export function maxCost(tx: FrameTransaction, blobBaseFee = 0n): bigint {
  return tx.maxFee * totalGasLimit(tx) + BigInt(tx.blobHashes.length) * 131_072n * blobBaseFee;
}

/** A frame transaction in the JSON shape of `eth_sendFrameTransaction` (ERC-8421 alternative mempools). */
export interface FrameTxRequest {
  chainId: Hex;
  nonceKeys: Hex[];
  nonceSeq: Hex;
  sender: Hex;
  frames: { mode: number; flags: number; target: Hex | null; limits: { execution: Hex; state: Hex }; value: Hex; data: Hex }[];
  signatures: { scheme: number; signer: Hex; msg: Hex; signature: Hex }[];
  fees: { maxPriorityFeePerGas: Hex; maxFeePerGas: Hex; maxFeePerBlobGas: Hex };
  blobVersionedHashes: Hex[];
  recentRootReferences: never[];
}

const quantity = (v: bigint) => `0x${v.toString(16)}` as Hex;

/**
 * `tx` as `eth_sendFrameTransaction` takes it: quantities as hex, mode/flags/scheme as numbers, an
 * ARBITRARY entry's signer and an empty msg as "0x". Lossless: the mempool re-encodes it to the same
 * raw bytes and hash as `encodeRaw` (on a chain without EIP-8272 recent roots).
 */
export function toFrameTxRequest(tx: FrameTransaction): FrameTxRequest {
  return {
    chainId: quantity(tx.chainId),
    nonceKeys: tx.nonceKeys.map(quantity),
    nonceSeq: quantity(tx.nonceSeq),
    sender: toHex(address20(tx.sender)),
    frames: tx.frames.map((f) => ({
      mode: Number(f.mode),
      flags: Number(f.flags),
      target: f.target === null ? null : toHex(address20(f.target)),
      limits: { execution: quantity(f.gasLimit), state: quantity(f.stateLimit) },
      value: quantity(f.value),
      data: toHex(f.data),
    })),
    signatures: tx.signatures.map((s) => ({
      scheme: Number(s.scheme),
      signer: toHex(signerBytes(s)),
      msg: toHex(s.msg),
      signature: toHex(s.signature),
    })),
    fees: { maxPriorityFeePerGas: quantity(tx.maxPriorityFee), maxFeePerGas: quantity(tx.maxFee), maxFeePerBlobGas: quantity(tx.maxBlobFee) },
    blobVersionedHashes: tx.blobHashes.map(toHex),
    recentRootReferences: [],
  };
}

/** The inverse of `toFrameTxRequest`: an `eth_sendFrameTransaction` parameter as a FrameTransaction. */
export function fromFrameTxRequest(req: FrameTxRequest): FrameTransaction {
  const big = (v: string | number) => BigInt(v);
  const addr = (v: string) => BigInt(v);
  return {
    chainId: big(req.chainId),
    nonceKeys: req.nonceKeys.map(big),
    nonceSeq: big(req.nonceSeq),
    sender: addr(req.sender),
    frames: req.frames.map((f) => ({
      mode: big(f.mode),
      flags: big(f.flags),
      target: f.target === null || f.target === undefined ? null : addr(f.target),
      gasLimit: big(f.limits.execution),
      stateLimit: big(f.limits.state),
      value: big(f.value),
      data: fromHex(f.data),
    })),
    signatures: req.signatures.map((s) => ({
      scheme: big(s.scheme),
      signer: s.signer === "0x" ? 0n : addr(s.signer),
      msg: fromHex(s.msg),
      signature: fromHex(s.signature),
    })),
    maxPriorityFee: big(req.fees.maxPriorityFeePerGas),
    maxFee: big(req.fees.maxFeePerGas),
    maxBlobFee: big(req.fees.maxFeePerBlobGas),
    blobHashes: (req.blobVersionedHashes ?? []).map((h) => fromHex(h)),
    ...(hasRecentRoots(big(req.chainId)) ? { recentRootReferences: [] as [] } : {}),
  };
}
