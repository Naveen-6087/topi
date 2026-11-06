// Differential testing: the Lean model (`pool-policy`, line-delimited JSON) against the chain's own
// validation (ethrex_simulateFrameTransaction). Simulation only; nothing is broadcast.
import { SCHEME, randomBytes, toBigint, toHex, run, type FrameTransaction, type RpcClient } from "../../sdk/src/index.ts";
import { encodeRawHex } from "../../sdk/src/index.ts";
import { settings } from "./common.ts";

const hex = (x: bigint) => `0x${x.toString(16)}`;

/** The transaction as the Lean model reads it. `verifies` says whether ARBITRARY witnesses are valid. */
export function modelTx(tx: FrameTransaction, verifies: boolean, emptySelector: "0x0" | "0x00") {
  return {
    sender: hex(tx.sender),
    nonceKeys: tx.nonceKeys.length,
    nonceSeq: Number(tx.nonceSeq),
    blobs: tx.blobHashes.length,
    frames: tx.frames.map((f) => ({
      target: hex(f.target ?? 0n),
      mode: Number(f.mode),
      flags: Number(f.flags),
      value: Number(f.value),
      dataLen: f.data.length,
      stateLimit: Number(f.stateLimit),
      selector: f.data.length === 0 ? emptySelector : toHex(f.data.subarray(0, 4)),
    })),
    sigs: tx.signatures.map((s) => ({
      signer: hex(s.signer),
      scheme: Number(s.scheme),
      msgEmpty: s.msg.length === 0,
      len: s.signature.length,
      verifies: s.scheme === SCHEME.ARBITRARY && verifies,
    })),
  };
}

export interface Row {
  name: string;
  chain: boolean;
  violation: string | null;
  query: unknown;
}

/** Simulates on chain; an RPC error counts as invalid, with the error as the violation. */
export async function chainVerdict(rpc: RpcClient, tx: FrameTransaction): Promise<{ chain: boolean; violation: string | null }> {
  try {
    const sim = await rpc.simulateFrameTransaction(encodeRawHex(tx));
    return { chain: sim.valid === true, violation: sim.violation };
  } catch (error) {
    return { chain: false, violation: `rpc: ${String(error).slice(0, 120)}` };
  }
}

/** Asks the Lean model about every row, one JSON line each. */
export async function askLean(rows: Row[]): Promise<Record<string, unknown>[]> {
  const exe = settings.leanPolicy();
  const result = await run(exe, [], { input: rows.map((r) => JSON.stringify(r.query)).join("\n") + "\n" });
  if (result.status !== 0) throw new Error(`${exe} exited ${result.status}: ${result.stderr.slice(-300)}`);
  const answers = result.stdout.trim().split(/\r?\n/).map((l) => JSON.parse(l) as Record<string, unknown>);
  if (answers.length !== rows.length) throw new Error(`Lean answered ${answers.length} of ${rows.length} queries`);
  return answers;
}

/** Prints the agreement summary and disagreements; returns the number of disagreements. */
export function report(title: string, rows: Row[], answers: Record<string, unknown>[], detail: (a: Record<string, unknown>) => string): number {
  const bad: string[] = [];
  rows.forEach((r, i) => {
    const a = answers[i];
    if (a.valid !== r.chain) {
      bad.push(`  ${r.name}: chain ${r.chain ? "valid" : "invalid"} (${r.violation}) vs Lean ${a.valid ? "valid" : "invalid"} (${detail(a)})`);
    }
  });
  const valid = rows.filter((r) => r.chain).length;
  console.log(`${title}: ${rows.length} transactions (${valid} valid on chain), Lean agrees on ${rows.length - bad.length}, disagrees on ${bad.length}`);
  for (const line of bad) console.log(line);
  return bad.length;
}

/** A random nonce key of `bytes` bytes. */
export const randomKey = (bytes: number) => toBigint(randomBytes(bytes));
