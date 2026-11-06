/**
 * Bytes, hex and integers. Protocol integers are bigint throughout; hex strings are "0x"-prefixed
 * and lowercase on output.
 *
 * Portions derived from soispoke/minimal-shielded-pool (Apache-2.0); see NOTICE.
 */
import { keccak_256 } from "@noble/hashes/sha3.js";
import { bytesToHex, concatBytes, hexToBytes } from "@noble/hashes/utils.js";

export type Hex = `0x${string}`;

/** Concatenates byte arrays. */
export function concat(...parts: Uint8Array[]): Uint8Array {
  return concatBytes(...parts);
}

/** keccak256 of bytes, or of a string's UTF-8 encoding. */
export function keccak256(data: Uint8Array | string): Uint8Array {
  return keccak_256(typeof data === "string" ? new TextEncoder().encode(data) : data);
}

/** "0x" followed by two lowercase hex digits per byte. */
export function toHex(bytes: Uint8Array): Hex {
  return `0x${bytesToHex(bytes)}`;
}

/** Decodes hex with or without "0x"; throws on odd length or a non-hex digit. */
export function fromHex(text: string, length?: number): Uint8Array {
  const digits = text.startsWith("0x") || text.startsWith("0X") ? text.slice(2) : text;
  if (digits.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(digits)) {
    throw new TypeError("expected hex with whole bytes");
  }
  const bytes = hexToBytes(digits);
  if (length !== undefined && bytes.length !== length) {
    throw new RangeError(`expected ${length} bytes, got ${bytes.length}`);
  }
  return bytes;
}

/** Big-endian unsigned integer of a byte string; the empty string is 0. */
export function toBigint(bytes: Uint8Array): bigint {
  return bytes.length === 0 ? 0n : BigInt(toHex(bytes));
}

/** Big-endian encoding in exactly `length` bytes; throws instead of truncating. */
export function toBytes(value: bigint, length: number): Uint8Array {
  if (value < 0n || value >= 1n << BigInt(8 * length)) {
    throw new RangeError(`${value} does not fit in ${length} bytes`);
  }
  return hexToBytes(value.toString(16).padStart(2 * length, "0"));
}

/** Big-endian encoding without leading zeros; 0 is the empty string. */
export function toMinimalBytes(value: bigint): Uint8Array {
  if (value < 0n) throw new RangeError(`${value} is negative`);
  return value === 0n ? new Uint8Array(0) : toBytes(value, (value.toString(16).length + 1) >> 1);
}

/** One 32-byte big-endian word (a uint256 or a field element). */
export function word(value: bigint | number): Uint8Array {
  return toBytes(BigInt(value), 32);
}

/** A 20-byte address. */
export function address20(address: bigint): Uint8Array {
  return toBytes(address, 20);
}

/** "0x" and 40 lowercase hex digits. */
export function addressHex(address: bigint): Hex {
  return toHex(address20(address));
}

/** "0x" and the minimal hex digits of a quantity, as JSON-RPC writes them. */
export function quantity(value: bigint | number): Hex {
  return `0x${BigInt(value).toString(16)}`;
}

/** Cryptographically random bytes. */
export function randomBytes(length: number): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(length));
}

export function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((byte, i) => byte === b[i]);
}

export function maxBigint(a: bigint, b: bigint): bigint {
  return a > b ? a : b;
}
