/**
 * Recursive Length Prefix encoding and decoding.
 *
 * Portions derived from soispoke/minimal-shielded-pool (Apache-2.0); see NOTICE.
 */
import { concat, toBigint, toMinimalBytes } from "./bytes.ts";

/** A decoded RLP item: a byte string or a list of items. */
export type RlpItem = Uint8Array | RlpItem[];

/** RLP of a byte string. A single byte below 0x80 is its own encoding. */
export function rlpBytes(bytes: Uint8Array): Uint8Array {
  if (bytes.length === 1 && bytes[0] < 0x80) return Uint8Array.of(bytes[0]);
  return concat(header(0x80, bytes.length), bytes);
}

/** RLP of a list whose items are already RLP-encoded. */
export function rlpList(items: Uint8Array[]): Uint8Array {
  const body = concat(...items);
  return concat(header(0xc0, body.length), body);
}

/** RLP of a non-negative integer: big-endian without leading zeros, so 0 is 0x80. */
export function rlpInt(value: bigint): Uint8Array {
  return rlpBytes(toMinimalBytes(value));
}

/** Encodes a nested item (byte strings and lists). */
export function rlpEncode(item: RlpItem): Uint8Array {
  return item instanceof Uint8Array ? rlpBytes(item) : rlpList(item.map(rlpEncode));
}

function header(offset: number, length: number): Uint8Array {
  if (length < 56) return Uint8Array.of(offset + length);
  const size = toMinimalBytes(BigInt(length));
  return concat(Uint8Array.of(offset + 55 + size.length), size);
}

interface Span {
  list: boolean;
  start: number;
  end: number;
}

/** Where the payload of the item at `at` starts and ends, and whether it is a list. */
function span(bytes: Uint8Array, at: number): Span {
  if (at >= bytes.length) throw new RangeError("truncated RLP");
  const prefix = bytes[at];
  if (prefix < 0x80) return { list: false, start: at, end: at + 1 };
  const list = prefix >= 0xc0;
  const short = prefix - (list ? 0xc0 : 0x80);
  if (short <= 55) {
    const end = at + 1 + short;
    if (end > bytes.length) throw new RangeError("truncated RLP");
    if (!list && short === 1 && bytes[at + 1] < 0x80) throw new RangeError("non-canonical RLP");
    return { list, start: at + 1, end };
  }
  const sizeLength = short - 55;
  const start = at + 1 + sizeLength;
  if (start > bytes.length) throw new RangeError("truncated RLP");
  if (bytes[at + 1] === 0) throw new RangeError("non-canonical RLP length");
  const length = Number(toBigint(bytes.subarray(at + 1, start)));
  if (length < 56) throw new RangeError("non-canonical RLP length");
  if (start + length > bytes.length) throw new RangeError("truncated RLP");
  return { list, start, end: start + length };
}

function decodeAt(bytes: Uint8Array, at: number): [RlpItem, number] {
  const { list, start, end } = span(bytes, at);
  if (!list) return [bytes.slice(start, end), end];
  const items: RlpItem[] = [];
  for (let i = start; i < end; ) {
    const [item, next] = decodeAt(bytes, i);
    if (next > end) throw new RangeError("RLP item overruns its list");
    items.push(item);
    i = next;
  }
  return [items, end];
}

/** Decodes exactly one RLP item; trailing bytes are an error. */
export function rlpDecode(bytes: Uint8Array): RlpItem {
  const [item, end] = decodeAt(bytes, 0);
  if (end !== bytes.length) throw new RangeError("trailing bytes after RLP item");
  return item;
}

/** The top-level items of one RLP list, each still encoded. */
export function rlpItems(encoded: Uint8Array): Uint8Array[] {
  const outer = span(encoded, 0);
  if (!outer.list) throw new TypeError("not an RLP list");
  const items: Uint8Array[] = [];
  for (let at = outer.start; at < outer.end; ) {
    const end = span(encoded, at).end;
    items.push(encoded.subarray(at, end));
    at = end;
  }
  return items;
}
