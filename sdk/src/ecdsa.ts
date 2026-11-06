/**
 * secp256k1 keys, addresses and signatures for frame transactions.
 *
 * Portions derived from soispoke/minimal-shielded-pool (Apache-2.0); see NOTICE.
 */
import { secp256k1 } from "@noble/curves/secp256k1.js";

import { address20, fromHex, keccak256, toBigint, toHex, type Hex } from "./bytes.ts";
import { SCHEME, signatureHash, type FrameSignature, type FrameTransaction } from "./frames.ts";

/** The order of secp256k1's group. */
export const SECP256K1_N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;

/**
 * Parses a private key: 64 hex digits with or without "0x", surrounding whitespace ignored.
 * The error never repeats the input, which is a secret.
 */
export function parsePrivateKey(text: string): Uint8Array {
  const trimmed = text.trim();
  if (/^(0x)?[0-9a-fA-F]{64}$/.test(trimmed)) {
    const key = fromHex(trimmed.slice(-64));
    if (secp256k1.utils.isValidSecretKey(key)) return key;
  }
  throw new TypeError("not a secp256k1 private key");
}

/** Signs a 32-byte hash as given: v (0 or 1) || r || s, RFC 6979 nonce, low s. */
export function signHash(hash: Uint8Array, privateKey: Uint8Array): Uint8Array {
  if (hash.length !== 32) throw new RangeError(`a message hash is 32 bytes, got ${hash.length}`);
  return secp256k1.sign(hash, privateKey, { prehash: false, lowS: true, format: "recovered" });
}

/** The address that produced a v || r || s signature over a 32-byte hash. */
export function recoverSigner(hash: Uint8Array, signature: Uint8Array): bigint {
  if (hash.length !== 32) throw new RangeError(`a message hash is 32 bytes, got ${hash.length}`);
  if (signature.length !== 65 || signature[0] > 1) {
    throw new RangeError("a secp256k1 signature is v (0 or 1), r and s in 65 bytes");
  }
  const key = secp256k1.Signature.fromBytes(signature, "recovered").recoverPublicKey(hash);
  return publicKeyAddress(key.toBytes(false));
}

/** The Ethereum address of a private key. */
export function addressOf(privateKey: Uint8Array): bigint {
  return publicKeyAddress(secp256k1.getPublicKey(privateKey, false));
}

function publicKeyAddress(uncompressed: Uint8Array): bigint {
  return toBigint(keccak256(uncompressed.subarray(1)).subarray(12));
}

/** EIP-55 mixed-case address. */
export function checksumAddress(address: bigint): Hex {
  const digits = toHex(address20(address)).slice(2);
  const hash = toHex(keccak256(digits)).slice(2);
  return `0x${[...digits].map((d, i) => (hash[i] >= "8" ? d.toUpperCase() : d)).join("")}`;
}

/**
 * Signs a frame transaction with secp256k1 (empty msg, so the signature covers the signature
 * hash). The ECDSA entry goes first, followed by `after` (for example an ARBITRARY placeholder
 * still to be filled). Mutates and returns `tx`.
 */
export function signTransaction(tx: FrameTransaction, key: Uint8Array, ...after: FrameSignature[]): FrameTransaction {
  const unsigned = { scheme: SCHEME.SECP256K1, signer: addressOf(key), msg: new Uint8Array(0) };
  tx.signatures = [{ ...unsigned, signature: new Uint8Array(0) }, ...after];
  tx.signatures = [{ ...unsigned, signature: signHash(signatureHash(tx), key) }, ...after];
  return tx;
}
