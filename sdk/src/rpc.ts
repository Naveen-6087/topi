/**
 * A small JSON-RPC client over fetch, with the ethrex frame-transaction extensions.
 */
import { addressHex, quantity, toHex, type Hex } from "./bytes.ts";
import { toFrameTxRequest, type FrameTransaction } from "./frames.ts";

export interface RpcLog {
  address: Hex;
  topics: Hex[];
  data: Hex;
  blockNumber: Hex;
  transactionHash: Hex;
  logIndex: Hex;
}

export interface FrameReceipt {
  status: Hex;
  gasUsed?: Hex;
  logs?: RpcLog[];
}

export interface TransactionReceipt {
  transactionHash: Hex;
  blockNumber: Hex;
  status: Hex;
  gasUsed: Hex;
  logs: RpcLog[];
  /** Frame transactions: who paid. */
  payer?: Hex;
  /** Frame transactions: one receipt per frame. */
  frameReceipts?: FrameReceipt[];
}

export interface Block {
  number: Hex;
  hash: Hex;
  timestamp: Hex;
  baseFeePerGas: Hex;
}

/** The result of ethrex_simulateFrameTransaction: validation, then execution, without state change. */
export interface FrameSimulation {
  valid: boolean;
  /** Why validation failed, when it did. */
  violation: string | null;
  prefixShape?: unknown;
  payer?: Hex | null;
  executionStatus?: string | null;
  executionError?: string | null;
  frames?: unknown[];
  gasUsed?: Hex | null;
  [key: string]: unknown;
}

export interface LogFilter {
  address: bigint | Hex;
  topics?: (Hex | Hex[] | null)[];
  fromBlock?: bigint | Hex | "earliest" | "latest";
  toBlock?: bigint | Hex | "latest";
}

export interface FeeCaps {
  maxPriorityFee: bigint;
  maxFee: bigint;
}

export class RpcError extends Error {
  readonly code: number | undefined;
  readonly data: unknown;
  constructor(method: string, error: { code?: number; message?: string; data?: unknown }) {
    super(`${method}: ${error.message ?? JSON.stringify(error)}`);
    this.name = "RpcError";
    this.code = error.code;
    this.data = error.data;
  }
}

export interface RpcClientOptions {
  /** A fetch implementation; defaults to the global one. */
  fetch?: typeof fetch;
  /** Extra request headers. */
  headers?: Record<string, string>;
}

const blockTag = (b: bigint | Hex | string) => (typeof b === "bigint" ? quantity(b) : b);
const addressParam = (a: bigint | Hex) => (typeof a === "bigint" ? addressHex(a) : a);

export class RpcClient {
  readonly url: string;
  private readonly fetchImpl: typeof fetch;
  private readonly headers: Record<string, string>;
  private nextId = 1;

  constructor(url: string, options: RpcClientOptions = {}) {
    this.url = url;
    this.fetchImpl = options.fetch ?? globalThis.fetch;
    this.headers = { "content-type": "application/json", ...options.headers };
  }

  /** One JSON-RPC call; throws RpcError on an error response. */
  async request<T = unknown>(method: string, params: unknown[] = []): Promise<T> {
    const response = await this.fetchImpl(this.url, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({ jsonrpc: "2.0", id: this.nextId++, method, params }),
    });
    if (!response.ok) throw new RpcError(method, { code: response.status, message: `HTTP ${response.status}` });
    const body = (await response.json()) as { result?: T; error?: { code?: number; message?: string; data?: unknown } };
    if (body.error) throw new RpcError(method, body.error);
    return body.result as T;
  }

  async chainId(): Promise<bigint> {
    return BigInt(await this.request<Hex>("eth_chainId"));
  }

  async blockNumber(): Promise<bigint> {
    return BigInt(await this.request<Hex>("eth_blockNumber"));
  }

  async latestBlock(): Promise<Block> {
    return this.request<Block>("eth_getBlockByNumber", ["latest", false]);
  }

  async balance(address: bigint | Hex, block: bigint | "latest" = "latest"): Promise<bigint> {
    return BigInt(await this.request<Hex>("eth_getBalance", [addressParam(address), blockTag(block)]));
  }

  async transactionCount(address: bigint | Hex, block: bigint | "latest" | "pending" = "pending"): Promise<bigint> {
    return BigInt(await this.request<Hex>("eth_getTransactionCount", [addressParam(address), blockTag(block)]));
  }

  async code(address: bigint | Hex): Promise<Hex> {
    return this.request<Hex>("eth_getCode", [addressParam(address), "latest"]);
  }

  /** eth_call; returns the raw return data. */
  async call(to: bigint | Hex, data: Uint8Array | Hex, block: bigint | "latest" = "latest"): Promise<Hex> {
    const input = typeof data === "string" ? data : toHex(data);
    return this.request<Hex>("eth_call", [{ to: addressParam(to), data: input }, blockTag(block)]);
  }

  async getLogs(filter: LogFilter): Promise<RpcLog[]> {
    return this.request<RpcLog[]>("eth_getLogs", [
      {
        address: addressParam(filter.address),
        topics: filter.topics,
        fromBlock: blockTag(filter.fromBlock ?? "earliest"),
        toBlock: blockTag(filter.toBlock ?? "latest"),
      },
    ]);
  }

  async getReceipt(hash: Hex): Promise<TransactionReceipt | null> {
    return this.request<TransactionReceipt | null>("eth_getTransactionReceipt", [hash]);
  }

  /** Polls for a receipt; throws if none arrives within `attempts * intervalMs`. */
  async waitForReceipt(hash: Hex, { intervalMs = 2_000, attempts = 60 } = {}): Promise<TransactionReceipt> {
    for (let i = 0; i < attempts; i++) {
      const receipt = await this.getReceipt(hash);
      if (receipt) return receipt;
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
    throw new Error(`transaction ${hash} not mined after ${attempts} polls`);
  }

  /** Broadcasts a signed raw transaction; returns its hash. This changes chain state. */
  async sendRawTransaction(raw: Uint8Array | Hex): Promise<Hex> {
    return this.request<Hex>("eth_sendRawTransaction", [typeof raw === "string" ? raw : toHex(raw)]);
  }
  /**
   * `eth_sendFrameTransaction` (ERC-8421 alternative mempools): `tx` as JSON, not raw bytes, so the
   * mempool runs its own admission rules (raw bytes sent to a mempool are redirected to its node).
   * Returns the transaction hash, keccak256(0x06 || rlp(tx)).
   */
  async sendFrameTransaction(tx: FrameTransaction): Promise<Hex> {
    return this.request<Hex>("eth_sendFrameTransaction", [toFrameTxRequest(tx)]);
  }


  /** ethrex_simulateFrameTransaction: validates and executes a frame transaction without committing. */
  async simulateFrameTransaction(raw: Uint8Array | Hex): Promise<FrameSimulation> {
    return this.request<FrameSimulation>("ethrex_simulateFrameTransaction", [typeof raw === "string" ? raw : toHex(raw)]);
  }

  /** Fee caps from the latest block: priority `tip`, max fee 2 * base fee + tip. */
  async feeCaps(tip = 1_000_000_000n): Promise<FeeCaps> {
    const block = await this.latestBlock();
    return { maxPriorityFee: tip, maxFee: 2n * BigInt(block.baseFeePerGas) + tip };
  }
}
