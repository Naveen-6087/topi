// The pending pool. Unlike the public mempool, which keeps one pending transaction per sender and per
// non-canonical paymaster, this one holds many settles from the shared pool sender at once: that is the
// point of an alternative mempool for a privacy pool. Its own rules keep that safe:
//
//   1. the exact settle shape (rules.ts);
//   2. the pool and every accepted paymaster run the code they had when this node started;
//   3. both nullifiers unspent on chain, and not already claimed by another pending settle;
//   4. both spends' roots known to the pool;
//   5. the paymaster's leanSPHINCS witness verifies under the exit key, when the key is known;
//   6. a full simulation of validation and execution passes;
//   7. caps on the pool's size and per paymaster, an expiry, and reputation for peers.
//
// Pending settles are re-checked on every new block, so a spend that settles elsewhere drops out.
import { EventEmitter } from "node:events";

import { decodeRaw, keccak256, signatureHash, toHex, type FrameTransaction, type Hex } from "../../sdk/src/index.ts";

import type { Chain } from "./chain.ts";
import { Rejection, checkSettle, type Policy, type SettleSummary } from "./rules.ts";

export type Status = "pending" | "inflight";

export interface Entry {
  hash: Hex;
  raw: Uint8Array;
  tx: FrameTransaction;
  summary: SettleSummary;
  /** "local" for a direct submission, otherwise the peer it came from. */
  source: string;
  admittedAt: number;
  status: Status;
}

export interface MempoolOptions {
  policy: Policy;
  chain: Chain;
  /** Largest raw transaction accepted, in bytes (a settle is about 110 KB). */
  maxBytes?: number;
  maxPending?: number;
  maxPerPaymaster?: number;
  /** A pending settle is dropped after this long. */
  ttlMs?: number;
  /** Run ethrex_simulateFrameTransaction before admitting (on by default). */
  simulate?: boolean;
  /** Rejections from one peer before it is banned. */
  banAfter?: number;
  now?: () => number;
}

export interface Counters {
  admitted: number;
  known: number;
  rejected: Record<string, number>;
  dropped: Record<string, number>;
  released: number;
  mined: number;
  reverted: number;
}

export interface Admission {
  hash: Hex;
  /** "admitted" the first time, "known" for a transaction already pending. */
  result: "admitted" | "known";
}

/** Emits "admitted" (Entry), "dropped" (Entry, reason) and "mined" (Entry, status). */
export class Mempool extends EventEmitter {
  readonly policy: Policy;
  readonly chain: Chain;
  readonly counters: Counters = { admitted: 0, known: 0, rejected: {}, dropped: {}, released: 0, mined: 0, reverted: 0 };
  private readonly entries = new Map<Hex, Entry>();
  private readonly byNullifier = new Map<bigint, Hex>();
  private readonly strikes = new Map<string, number>();
  private readonly banned = new Set<string>();
  private codeHashes: Map<bigint, Hex> | null = null;
  private readonly opts: Required<Omit<MempoolOptions, "policy" | "chain">>;

  constructor(options: MempoolOptions) {
    super();
    this.policy = options.policy;
    this.chain = options.chain;
    this.opts = {
      maxBytes: options.maxBytes ?? 512 * 1024,
      maxPending: options.maxPending ?? 256,
      maxPerPaymaster: options.maxPerPaymaster ?? 64,
      ttlMs: options.ttlMs ?? 30 * 60_000,
      simulate: options.simulate ?? true,
      banAfter: options.banAfter ?? 5,
      now: options.now ?? Date.now,
    };
  }

  /** Records the pool's and paymasters' code hashes: rule 2 holds them to this code. */
  async start(): Promise<void> {
    const hashes = new Map<bigint, Hex>();
    for (const address of [this.policy.pool, ...this.policy.paymasters]) {
      const hash = await this.chain.codeHash(address);
      if (hash === toHex(keccak256(new Uint8Array(0)))) throw new Error(`no code at 0x${address.toString(16)}`);
      hashes.set(address, hash);
    }
    this.codeHashes = hashes;
  }

  /** The code hashes this node holds the pool and paymasters to. */
  rules(): { pool: Hex; paymasters: Hex[]; codeHashes: Record<Hex, Hex>; maxValidationGas: string } {
    const hex = (v: bigint) => `0x${v.toString(16).padStart(40, "0")}` as Hex;
    return {
      pool: hex(this.policy.pool),
      paymasters: this.policy.paymasters.map(hex),
      codeHashes: Object.fromEntries([...(this.codeHashes ?? new Map())].map(([a, h]) => [hex(a), h])),
      maxValidationGas: this.policy.maxValidationGas.toString(),
    };
  }

  get size(): number {
    return this.entries.size;
  }

  list(): Entry[] {
    return [...this.entries.values()].sort((a, b) => a.admittedAt - b.admittedAt);
  }

  get(hash: Hex): Entry | undefined {
    return this.entries.get(hash);
  }

  isBanned(source: string): boolean {
    return this.banned.has(source);
  }

  /** Checks a raw settle against every rule and holds it. Throws Rejection when it does not pass. */
  async admit(raw: Uint8Array, source = "local"): Promise<Admission> {
    if (this.banned.has(source)) throw new Rejection("banned", `${source} is banned for sending invalid settles`);
    try {
      return await this.check(raw, source);
    } catch (e) {
      const code = e instanceof Rejection ? e.code : "invalid";
      this.counters.rejected[code] = (this.counters.rejected[code] ?? 0) + 1;
      if (source !== "local" && code !== "full" && code !== "nullifier-pending") {
        const strikes = (this.strikes.get(source) ?? 0) + 1;
        this.strikes.set(source, strikes);
        if (strikes >= this.opts.banAfter) this.banned.add(source);
      }
      throw e instanceof Rejection ? e : new Rejection("invalid", e instanceof Error ? e.message : String(e));
    }
  }

  private async check(raw: Uint8Array, source: string): Promise<Admission> {
    if (!this.codeHashes) throw new Error("call start() before admitting");
    if (raw.length > this.opts.maxBytes) throw new Rejection("too-large", `${raw.length} bytes is over ${this.opts.maxBytes}`);
    const hash = toHex(keccak256(raw));
    if (this.entries.has(hash)) {
      this.counters.known++;
      return { hash, result: "known" };
    }
    let tx: FrameTransaction;
    try {
      tx = decodeRaw(raw);
    } catch (e) {
      throw new Rejection("decode", `not a frame transaction: ${e instanceof Error ? e.message : e}`);
    }
    const summary = checkSettle(tx, this.policy);

    // Rule 3, locally: one pending settle per nullifier.
    for (const n of summary.nullifiers) {
      if (this.byNullifier.has(n)) throw new Rejection("nullifier-pending", "a pending settle already spends this nullifier");
    }
    // Rule 7: size caps.
    if (this.entries.size >= this.opts.maxPending) throw new Rejection("full", "the mempool is full");
    const perPaymaster = this.list().filter((e) => e.summary.paymaster === summary.paymaster).length;
    if (perPaymaster >= this.opts.maxPerPaymaster) throw new Rejection("full", "too many pending settles for this paymaster");

    // Rule 2: the code is the code this node vetted.
    for (const address of [summary.sender, summary.paymaster]) {
      if ((await this.chain.codeHash(address)) !== this.codeHashes.get(address)) {
        throw new Rejection("code-changed", `the code at 0x${address.toString(16)} changed`);
      }
    }
    await this.checkChainState(summary);

    // Rule 5: the paymaster's own check, run here too when this is the deployment's paymaster.
    const d = this.policy.deployment;
    if (this.policy.pqPaymaster && summary.paymaster === BigInt(d.paymaster) && d.verifier && d.exitPkSeed && d.exitPkRoot) {
      const ok = await this.chain.verifyLeanSphincs(
        BigInt(d.verifier),
        BigInt(d.exitPkSeed),
        BigInt(d.exitPkRoot),
        signatureHash(tx),
        tx.signatures[0].signature,
      );
      if (!ok) throw new Rejection("witness", "the leanSPHINCS witness does not verify under the exit key");
    }

    // Rule 6: the node's own validation and execution, without committing.
    if (this.opts.simulate) {
      const sim = await this.chain.simulate(raw);
      if (!sim.valid) throw new Rejection("simulation", `validation fails: ${sim.violation ?? "unknown"}`);
      const status = sim.executionStatus?.toString() ?? "";
      if (sim.executionError || /revert|fail|halt|error|0x0$/i.test(status)) {
        throw new Rejection("simulation", `settle would revert: ${sim.executionError ?? sim.executionStatus}`);
      }
    }

    const entry: Entry = { hash, raw, tx, summary, source, admittedAt: this.opts.now(), status: "pending" };
    this.entries.set(hash, entry);
    for (const n of summary.nullifiers) this.byNullifier.set(n, hash);
    this.counters.admitted++;
    this.emit("admitted", entry);
    return { hash, result: "admitted" };
  }

  /** Rules 3 and 4 against the chain. */
  private async checkChainState(s: SettleSummary): Promise<void> {
    const pool = this.policy.pool;
    for (const n of s.nullifiers) {
      if (await this.chain.spent(pool, n)) throw new Rejection("nullifier-spent", "a nullifier is already spent on chain");
    }
    for (const r of s.poolRoots) {
      if (!(await this.chain.knownPoolRoot(pool, r))) throw new Rejection("unknown-root", "a pool root is not known to the pool");
    }
    for (const r of s.associationRoots) {
      if (!(await this.chain.knownAssociationRoot(pool, r))) {
        throw new Rejection("unknown-root", "an association root is not known to the pool");
      }
    }
  }

  /** Drops expired settles and those the chain has made invalid. Call on every new block. */
  async revalidate(): Promise<void> {
    const now = this.opts.now();
    for (const entry of this.list()) {
      if (entry.status === "inflight") continue;
      if (now - entry.admittedAt > this.opts.ttlMs) {
        this.drop(entry, "expired");
        continue;
      }
      try {
        await this.checkChainState(entry.summary);
      } catch (e) {
        this.drop(entry, e instanceof Rejection ? e.code : "invalid");
      }
    }
  }

  /** The next settle to release for `sender`: the oldest pending one, unless one is already in flight. */
  next(sender: bigint): Entry | undefined {
    const mine = this.list().filter((e) => e.summary.sender === sender);
    if (mine.some((e) => e.status === "inflight")) return undefined;
    return mine.find((e) => e.status === "pending");
  }

  senders(): bigint[] {
    return [...new Set(this.list().map((e) => e.summary.sender))];
  }

  markInflight(entry: Entry): void {
    entry.status = "inflight";
    this.counters.released++;
  }

  /** Settled on chain (status true) or reverted (false): either way it leaves the pool. */
  markMined(entry: Entry, success: boolean): void {
    if (success) this.counters.mined++;
    else this.counters.reverted++;
    this.remove(entry);
    this.emit("mined", entry, success);
  }

  drop(entry: Entry, reason: string): void {
    this.counters.dropped[reason] = (this.counters.dropped[reason] ?? 0) + 1;
    this.remove(entry);
    this.emit("dropped", entry, reason);
  }

  private remove(entry: Entry): void {
    this.entries.delete(entry.hash);
    for (const n of entry.summary.nullifiers) if (this.byNullifier.get(n) === entry.hash) this.byNullifier.delete(n);
  }
}
