// The mempool's JSON-RPC endpoint and its gossip with peers.
//
//   eth_sendFrameTransaction [tx]          submit a settle as JSON (the ERC-8421 sidecar's method)
//   eth_sendRawTransaction [raw]           submit a settle as raw bytes
//   altmempool_relay       [raw, hops, from] a peer relaying a settle it admitted
//   altmempool_status      []              counters, size, peers
//   altmempool_pending     []              the pending settles
//   altmempool_rules       []              the code hashes and limits this node admits by
//   eth_chainId            []
//   GET /metrics                           the counters in Prometheus text format
import { createServer, type IncomingMessage, type Server } from "node:http";

import { encodeRaw, fromFrameTxRequest, fromHex, toHex, type FrameTxRequest, type Hex } from "../../sdk/src/index.ts";

import type { Entry, Mempool } from "./mempool.ts";
import { Rejection } from "./rules.ts";

/** Gossip stops after this many relays, so a settle never circulates forever. */
export const MAX_HOPS = 3;

interface Request {
  id?: unknown;
  method?: string;
  params?: unknown[];
}

const hex = (v: bigint) => `0x${v.toString(16)}` as Hex;

export function describe(e: Entry, now = Date.now()) {
  return {
    hash: e.hash,
    status: e.status,
    source: e.source,
    ageSeconds: Math.floor((now - e.admittedAt) / 1000),
    paymaster: hex(e.summary.paymaster),
    nullifiers: e.summary.nullifiers.map(hex),
    bytes: e.raw.length,
  };
}

/** Relays every settle this node admits to its peers, except back to where it came from. */
export class Gossip {
  /** This node's own URL as its peers reach it: peers use it to avoid relaying a settle back. */
  readonly id: string;
  readonly peers: string[];
  private readonly hops = new Map<Hex, number>();
  private readonly log: (line: string) => void;

  constructor(mempool: Mempool, id: string, peers: string[], log: (line: string) => void = () => {}) {
    this.id = id;
    this.peers = peers;
    this.log = log;
    // On the next turn, after the relay handler has recorded how many hops the settle already made.
    mempool.on("admitted", (entry: Entry) => setImmediate(() => void this.relay(entry)));
  }

  /** Remembers how far a peer's settle has already travelled. */
  noteHops(hash: Hex, hops: number): void {
    this.hops.set(hash, hops);
  }

  private async relay(entry: Entry): Promise<void> {
    const hops = (this.hops.get(entry.hash) ?? 0) + 1;
    this.hops.delete(entry.hash);
    if (hops > MAX_HOPS) return;
    await Promise.all(
      this.peers
        .filter((url) => `peer:${url}` !== entry.source)
        .map(async (url) => {
          try {
            const res = await fetch(url, {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "altmempool_relay", params: [toHex(entry.raw), hops, this.id] }),
            });
            const body = (await res.json()) as { error?: { message: string } };
            if (body.error && !/known|nullifier-pending/.test(body.error.message)) this.log(`peer ${url} refused ${entry.hash}: ${body.error.message}`);
          } catch (e) {
            this.log(`peer ${url} unreachable: ${e instanceof Error ? e.message : e}`);
          }
        }),
    );
  }
}

function metrics(mempool: Mempool): string {
  const c = mempool.counters;
  const lines = [
    "# HELP altmempool_pending Settles held, by status.",
    "# TYPE altmempool_pending gauge",
    `altmempool_pending{status="pending"} ${mempool.list().filter((e) => e.status === "pending").length}`,
    `altmempool_pending{status="inflight"} ${mempool.list().filter((e) => e.status === "inflight").length}`,
    "# HELP altmempool_admitted_total Settles admitted.",
    "# TYPE altmempool_admitted_total counter",
    `altmempool_admitted_total ${c.admitted}`,
    "# HELP altmempool_rejected_total Submissions refused, by rule.",
    "# TYPE altmempool_rejected_total counter",
    ...Object.entries(c.rejected).map(([r, n]) => `altmempool_rejected_total{reason="${r}"} ${n}`),
    "# HELP altmempool_dropped_total Pending settles dropped, by reason.",
    "# TYPE altmempool_dropped_total counter",
    ...Object.entries(c.dropped).map(([r, n]) => `altmempool_dropped_total{reason="${r}"} ${n}`),
    "# HELP altmempool_released_total Settles handed to the chain.",
    "# TYPE altmempool_released_total counter",
    `altmempool_released_total ${c.released}`,
    "# HELP altmempool_mined_total Released settles that were mined, by outcome.",
    "# TYPE altmempool_mined_total counter",
    `altmempool_mined_total{status="success"} ${c.mined}`,
    `altmempool_mined_total{status="reverted"} ${c.reverted}`,
  ];
  return `${lines.join("\n")}\n`;
}

async function readBody(req: IncomingMessage, limit: number): Promise<string> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of req) {
    size += (chunk as Buffer).length;
    if (size > limit) throw new Rejection("too-large", "request too large");
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

const rawParam = (params: unknown[] | undefined): Uint8Array => {
  const raw = params?.[0];
  if (typeof raw !== "string" || !/^0x[0-9a-fA-F]*$/.test(raw)) throw new Rejection("params", "expected a 0x-prefixed raw transaction");
  return fromHex(raw);
};

export function serve(mempool: Mempool, gossip: Gossip | null, port: number, host = "127.0.0.1"): Promise<Server> {
  const handle = async (body: Request, peerAddress: string): Promise<unknown> => {
    switch (body.method) {
      case "eth_sendRawTransaction":
        return (await mempool.admit(rawParam(body.params), "local")).hash;
      case "eth_sendFrameTransaction": {
        const req = body.params?.[0];
        if (typeof req !== "object" || req === null) throw new Rejection("params", "expected a frame transaction object");
        let raw: Uint8Array;
        try {
          raw = encodeRaw(fromFrameTxRequest(req as FrameTxRequest));
        } catch (e) {
          throw new Rejection("params", `malformed frame transaction: ${e instanceof Error ? e.message : e}`);
        }
        return (await mempool.admit(raw, "local")).hash;
      }
      case "altmempool_relay": {
        const raw = rawParam(body.params);
        const hops = Number(body.params?.[1] ?? 1);
        const from = typeof body.params?.[2] === "string" ? (body.params[2] as string) : peerAddress;
        const source = `peer:${from}`;
        const result = await mempool.admit(raw, source).then((a) => {
          if (a.result === "admitted") gossip?.noteHops(a.hash, hops);
          return a;
        });
        return result;
      }
      case "altmempool_status":
        return {
          size: mempool.size,
          pending: mempool.list().filter((e) => e.status === "pending").length,
          inflight: mempool.list().filter((e) => e.status === "inflight").length,
          counters: mempool.counters,
          peers: gossip?.peers ?? [],
          id: gossip?.id ?? null,
        };
      case "altmempool_pending":
        return mempool.list().map((e) => describe(e));
      case "altmempool_rules":
        return mempool.rules();
      case "eth_chainId":
        return hex(mempool.policy.chainId);
      default:
        throw Object.assign(new Error(`method ${body.method} is not supported by this mempool`), { rpcCode: -32601 });
    }
  };

  const server = createServer(async (req, res) => {
    if (req.method === "GET" && req.url === "/metrics") {
      res.writeHead(200, { "content-type": "text/plain; version=0.0.4" }).end(metrics(mempool));
      return;
    }
    if (req.method !== "POST") {
      res.writeHead(405).end();
      return;
    }
    let body: Request = {};
    try {
      body = JSON.parse(await readBody(req, 1024 * 1024)) as Request;
      const result = await handle(body, req.socket.remoteAddress ?? "unknown");
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ jsonrpc: "2.0", id: body.id ?? null, result }));
    } catch (e) {
      const code = e instanceof Rejection ? -32000 : ((e as { rpcCode?: number }).rpcCode ?? -32603);
      const message = e instanceof Rejection ? `${e.code}: ${e.message}` : e instanceof Error ? e.message : String(e);
      const data = e instanceof Rejection ? { reason: e.code } : undefined;
      res
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify({ jsonrpc: "2.0", id: body.id ?? null, error: { code, message, data } }));
    }
  });
  return new Promise((resolve) => server.listen(port, host, () => resolve(server)));
}
