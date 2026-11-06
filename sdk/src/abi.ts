/**
 * The small slice of the Solidity ABI the pool needs: selectors, static words and dynamic
 * `bytes` / `uint256[]` heads.
 */
import { concat, fromHex, keccak256, toBigint, toHex, word, type Hex } from "./bytes.ts";

/** The 4-byte selector of a function signature such as "deposit(uint256[8])". */
export function selector(signature: string): Uint8Array {
  return keccak256(signature).slice(0, 4);
}

/** The topic of an event signature such as "Settled(uint256,...)". */
export function eventTopic(signature: string): Hex {
  return toHex(keccak256(signature));
}

/** selector || one word per argument: calldata for functions with only static word arguments. */
export function encodeCall(signature: string, args: readonly (bigint | number)[] = []): Uint8Array {
  return concat(selector(signature), ...args.map(word));
}

/** Splits ABI-encoded return data or event data into 32-byte words. */
export function decodeWords(data: Hex | string | Uint8Array): bigint[] {
  const bytes = typeof data === "string" ? fromHex(data) : data;
  if (bytes.length % 32 !== 0) throw new RangeError(`ABI data of ${bytes.length} bytes is not whole words`);
  const out: bigint[] = [];
  for (let i = 0; i < bytes.length; i += 32) out.push(toBigint(bytes.subarray(i, i + 32)));
  return out;
}

/** The tail of a dynamic `bytes` argument: length word and the data padded to whole words. */
export function encodeBytesTail(data: Uint8Array): Uint8Array {
  const padding = new Uint8Array((32 - (data.length % 32)) % 32);
  return concat(word(data.length), data, padding);
}

/** The tail of a dynamic `uint256[]` argument: length word and one word per element. */
export function encodeUintArrayTail(values: readonly (bigint | number)[]): Uint8Array {
  return concat(word(values.length), ...values.map(word));
}
