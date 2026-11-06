// Hands settles to the chain. The public-mempool rule of one pending transaction per sender still
// holds at the block producer, so the releaser keeps one settle in flight per sender (all settles
// share the pool as sender) and releases the next once the previous one is mined or dropped.
import type { Mempool, Entry } from "./mempool.ts";

export interface ReleaserOptions {
  /** How often to look for work and poll receipts. */
  intervalMs?: number;
  /** Give up on an in-flight settle after this many polls without a receipt. */
  maxPolls?: number;
  /** false: hold and gossip only. Still re-checks on every block, so settles mined through a peer drop out. */
  release?: boolean;
  log?: (line: string) => void;
}

export class Releaser {
  private readonly mempool: Mempool;
  private readonly opts: Required<ReleaserOptions>;
  private readonly polls = new Map<string, number>();
  private timer: NodeJS.Timeout | null = null;
  private busy = false;
  private lastBlock = -1n;

  constructor(mempool: Mempool, options: ReleaserOptions = {}) {
    this.mempool = mempool;
    this.opts = { intervalMs: options.intervalMs ?? 2_000, maxPolls: options.maxPolls ?? 90, release: options.release ?? true, log: options.log ?? (() => {}) };
  }

  start(): void {
    this.timer = setInterval(() => void this.tick(), this.opts.intervalMs);
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  /** One round: re-check on a new block, settle receipts, release the next settle per sender. */
  async tick(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      const block = await this.mempool.chain.blockNumber();
      if (block !== this.lastBlock) {
        this.lastBlock = block;
        await this.mempool.revalidate();
      }
      for (const entry of this.mempool.list().filter((e) => e.status === "inflight")) await this.poll(entry);
      if (!this.opts.release) return;
      for (const sender of this.mempool.senders()) {
        const entry = this.mempool.next(sender);
        if (entry) await this.release(entry);
      }
    } catch (e) {
      this.opts.log(`releaser: ${e instanceof Error ? e.message : e}`);
    } finally {
      this.busy = false;
    }
  }

  private async release(entry: Entry): Promise<void> {
    try {
      const hash = await this.mempool.chain.send(entry.raw);
      if (hash.toLowerCase() !== entry.hash.toLowerCase()) this.opts.log(`releaser: node returned ${hash} for ${entry.hash}`);
      this.mempool.markInflight(entry);
      this.polls.set(entry.hash, 0);
      this.opts.log(`released ${entry.hash}`);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      // A node that already has it is fine: watch for the receipt.
      if (/already known|already exists|known transaction/i.test(message)) {
        this.mempool.markInflight(entry);
        this.polls.set(entry.hash, 0);
        return;
      }
      this.opts.log(`releaser: ${entry.hash} refused by the node: ${message}`);
      this.mempool.drop(entry, "node-refused");
    }
  }

  private async poll(entry: Entry): Promise<void> {
    const receipt = await this.mempool.chain.receipt(entry.hash);
    if (receipt) {
      const ok = receipt.status === "0x1";
      this.polls.delete(entry.hash);
      this.mempool.markMined(entry, ok);
      this.opts.log(`${ok ? "settled" : "reverted"} ${entry.hash} in block ${BigInt(receipt.blockNumber)}`);
      return;
    }
    const n = (this.polls.get(entry.hash) ?? 0) + 1;
    this.polls.set(entry.hash, n);
    if (n >= this.opts.maxPolls) {
      this.polls.delete(entry.hash);
      this.mempool.drop(entry, "no-receipt");
      this.opts.log(`gave up on ${entry.hash}: no receipt after ${n} polls`);
    }
  }
}
