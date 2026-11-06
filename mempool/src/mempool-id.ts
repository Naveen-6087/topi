// This alternative mempool's id: the IPFS CIDv0 of the document that defines its rules (ERC-8421:
// a mempool's topic is "conventionally the IPFS hash of a document that describes its rules").
//
//   node mempool/src/mempool-id.ts [FILE]     (default: mempool/RULES.md)
//
// Same result as `ipfs add --cid-version 0 FILE` for a file up to 256 KiB (one UnixFS chunk).
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function base58(bytes: Uint8Array): string {
  let n = BigInt(`0x${Buffer.from(bytes).toString("hex")}`);
  let out = "";
  while (n > 0n) {
    out = B58[Number(n % 58n)] + out;
    n /= 58n;
  }
  for (const b of bytes) {
    if (b !== 0) break;
    out = `1${out}`;
  }
  return out;
}

function varint(n: number): number[] {
  const out: number[] = [];
  while (n >= 0x80) {
    out.push((n & 0x7f) | 0x80);
    n >>>= 7;
  }
  out.push(n);
  return out;
}

const field = (tag: number, bytes: Uint8Array | number[]) => [tag, ...varint(bytes.length), ...bytes];

/** CIDv0 of `content` as one UnixFS file node (dag-pb), as `ipfs add` builds it. */
export function cidV0(content: Uint8Array): string {
  if (content.length > 262_144) throw new RangeError("more than one UnixFS chunk; use ipfs add");
  // UnixFS Data { Type = File (2), Data = content, filesize = length }
  const unixfs = [0x08, 0x02, ...(content.length ? field(0x12, content) : []), 0x18, ...varint(content.length)];
  // PBNode { Data = unixfs } (no links)
  const node = Uint8Array.from(field(0x0a, unixfs));
  const digest = createHash("sha256").update(node).digest();
  return base58(Uint8Array.from([0x12, 0x20, ...digest]));
}

export const RULES_FILE = join(import.meta.dirname, "..", "RULES.md");

/** This mempool's id: the CIDv0 of RULES.md. */
export function mempoolId(file = RULES_FILE): string {
  return cidV0(readFileSync(file));
}

if (import.meta.main ?? process.argv[1]?.endsWith("mempool-id.ts")) {
  console.log(mempoolId(process.argv[2] ?? RULES_FILE));
}
