/**
 * Post-quantum signers and the leanSPHINCS PQ account.
 *
 * A PQ signature travels as an ARBITRARY signature entry (empty signer, empty msg) whose bytes
 * are the witness over the transaction's signature hash; a VERIFY frame checks it on chain.
 * Because empty-msg signature bytes are elided from the hash, signing the hash of the
 * transaction with a placeholder entry is final.
 */
import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { fromHex, type Hex } from "./bytes.ts";
import {
  APPROVE,
  MODE,
  SCHEME,
  arbitrarySignaturePlaceholder,
  hasRecentRoots,
  randomNonceKey,
  signatureHash,
  type FrameTransaction,
} from "./frames.ts";
import type { FeeCaps } from "./rpc.ts";

const execFileAsync = promisify(execFile);

/** A leanSPHINCS public key: two 32-byte words. */
export interface PQPublicKey {
  pkSeed: Hex;
  pkRoot: Hex;
}

/** Anything that can produce a post-quantum witness over a 32-byte digest. */
export interface PQSigner {
  publicKey(): Promise<PQPublicKey>;
  sign(digest: Uint8Array): Promise<Uint8Array>;
}

/** The size of a leanSPHINCS (keccak) signature. */
export const LEANSPHINCS_SIGNATURE_BYTES = 5_684;

export interface LskSignerOptions {
  /** The `lsk` binary (leanSPHINCS keccak signer). */
  binary: string;
  /** The key file. Its contents never pass through this process. */
  keyFile: string;
}

/**
 * A PQSigner backed by the `lsk` CLI: `lsk info KEYFILE` prints {pkSeed, pkRoot}, and
 * `lsk sign-key KEYFILE DIGEST` prints 0x || abi.encode(bytes32 pkSeed, bytes32 pkRoot, bytes sig).
 */
export class LskSigner implements PQSigner {
  readonly binary: string;
  readonly keyFile: string;

  constructor(options: LskSignerOptions) {
    this.binary = options.binary;
    this.keyFile = options.keyFile;
  }

  async publicKey(): Promise<PQPublicKey> {
    const { stdout } = await execFileAsync(this.binary, ["info", this.keyFile], { encoding: "utf8" });
    const info = JSON.parse(stdout) as Partial<PQPublicKey>;
    if (typeof info.pkSeed !== "string" || typeof info.pkRoot !== "string") {
      throw new Error("lsk info: unexpected output");
    }
    return { pkSeed: info.pkSeed, pkRoot: info.pkRoot };
  }

  async sign(digest: Uint8Array): Promise<Uint8Array> {
    if (digest.length !== 32) throw new RangeError(`a digest is 32 bytes, got ${digest.length}`);
    const message = Buffer.from(digest).toString("hex");
    let stdout: string;
    try {
      ({ stdout } = await execFileAsync(this.binary, ["sign-key", this.keyFile, message], {
        encoding: "utf8",
        maxBuffer: 1 << 24,
      }));
    } catch (error) {
      // Report the exit, never the command's output or the key path's contents.
      const code = (error as { code?: unknown }).code;
      throw new Error(`lsk sign-key failed (exit ${String(code)})`);
    }
    return decodeLskSignature(stdout.trim());
  }
}

/** Extracts the signature from lsk's abi.encode(bytes32, bytes32, bytes) output. */
export function decodeLskSignature(output: string): Uint8Array {
  const hex = output.startsWith("0x") ? output.slice(2) : output;
  const offset = Number(BigInt(`0x${hex.slice(128, 192)}`)) * 2;
  const length = Number(BigInt(`0x${hex.slice(offset, offset + 64)}`));
  const signature = fromHex(hex.slice(offset + 64, offset + 64 + 2 * length));
  if (signature.length !== LEANSPHINCS_SIGNATURE_BYTES) {
    throw new Error(`unexpected leanSPHINCS signature length ${signature.length}`);
  }
  return signature;
}

/**
 * Fills the transaction's trailing ARBITRARY entry with `signer`'s witness over the signature
 * hash, appending a placeholder entry first if the last entry is not an empty ARBITRARY one.
 * Mutates and returns `tx`.
 */
export async function addArbitraryWitness(tx: FrameTransaction, signer: PQSigner): Promise<FrameTransaction> {
  const last = tx.signatures[tx.signatures.length - 1];
  if (!last || last.scheme !== SCHEME.ARBITRARY || last.signature.length !== 0 || last.msg.length !== 0) {
    tx.signatures.push(arbitrarySignaturePlaceholder());
  }
  const witness = await signer.sign(signatureHash(tx));
  tx.signatures[tx.signatures.length - 1].signature = witness;
  return tx;
}

// ---------- the PQ account ----------

/** One call the account makes in its SENDER frame. */
export interface AccountCall {
  target: bigint;
  value: bigint;
  data: Uint8Array;
  gasLimit: bigint;
  stateLimit: bigint;
}

export interface AccountTransactionParams {
  /** The account address. */
  account: bigint | Hex;
  /** The account's VERIFY-frame selector, 8 hex digits (as in the deployment record). */
  verifySelector: string;
  chainId: bigint;
  fees: FeeCaps;
  call: AccountCall;
  nonceKey?: bigint;
  /** VERIFY frame limits (defaults 120,000 execution and 110,000 state for the nonce key). */
  verifyGas?: bigint;
  verifyState?: bigint;
}

/**
 * An account transaction with an empty ARBITRARY entry, ready for addArbitraryWitness:
 *   frame 0  VERIFY  account  verifySelector   flags 3 (execution + payment)
 *   frame 1  SENDER  target   data, value
 */
export function buildAccountTransaction(params: AccountTransactionParams): FrameTransaction {
  const account = BigInt(params.account);
  return {
    chainId: params.chainId,
    nonceKeys: [params.nonceKey ?? randomNonceKey()],
    nonceSeq: 0n,
    sender: account,
    frames: [
      {
        mode: MODE.VERIFY,
        flags: APPROVE.EXECUTION_AND_PAYMENT,
        target: account,
        gasLimit: params.verifyGas ?? 120_000n,
        stateLimit: params.verifyState ?? 110_000n,
        value: 0n,
        data: fromHex(params.verifySelector, 4),
      },
      {
        mode: MODE.SENDER,
        flags: 0n,
        target: params.call.target,
        gasLimit: params.call.gasLimit,
        stateLimit: params.call.stateLimit,
        value: params.call.value,
        data: params.call.data,
      },
    ],
    signatures: [arbitrarySignaturePlaceholder()],
    maxPriorityFee: params.fees.maxPriorityFee,
    maxFee: params.fees.maxFee,
    maxBlobFee: 0n,
    blobHashes: [],
    ...(hasRecentRoots(params.chainId) ? { recentRootReferences: [] as [] } : {}),
  };
}

/** buildAccountTransaction, then the signer's witness. */
export async function signedAccountTransaction(params: AccountTransactionParams, signer: PQSigner): Promise<FrameTransaction> {
  return addArbitraryWitness(buildAccountTransaction(params), signer);
}
