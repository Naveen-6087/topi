/**
 * Adapter for the mixnet client binary (`topi-client`) and the exit's spend queue.
 *
 *   topi-client ping  --topology FILE --identity FILE [--timeout S]
 *   topi-client spend --topology FILE --identity FILE --proof CLIENT.tgz --public PUBLIC.json [--timeout S]
 *
 * The exit queues each spend as ID.json ({"fields": [82 numbers]}) and ID.tgz (the client proof).
 */
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import { run, type RunResult } from "./process.ts";

export interface MixnetOptions {
  /** The client binary. */
  client: string;
  /** The network topology file. */
  topology: string;
  /** The member identity file (from `topi-client register`). */
  identity: string;
  /** Seconds to wait for a reply. Default: the client's own (300). */
  timeoutSeconds?: number;
}

export interface PingResult {
  reply: unknown;
  round_trip_ms: number;
}

/** The exit's reply to a spend. */
export interface SpendReceipt {
  ok: boolean;
  /** The queue id the exit stored the spend under. */
  queued?: string;
  /** The encoded spend's size. */
  bytes?: number;
  reason?: string;
}

export class MixnetError extends Error {
  readonly result: RunResult;
  constructor(message: string, result: RunResult) {
    super(message);
    this.name = "MixnetError";
    this.result = result;
  }
}

/** The last line of output that parses as a JSON object. */
function lastJson(stdout: string): Record<string, unknown> | null {
  const lines = stdout.trim().split(/\r?\n/).reverse();
  for (const line of lines) {
    try {
      const value = JSON.parse(line);
      if (value && typeof value === "object") return value as Record<string, unknown>;
    } catch {
      // not JSON
    }
  }
  return null;
}

export class MixnetClient {
  readonly options: MixnetOptions;

  constructor(options: MixnetOptions) {
    this.options = options;
  }

  private connection(): string[] {
    const args = ["--topology", this.options.topology, "--identity", this.options.identity];
    if (this.options.timeoutSeconds !== undefined) args.push("--timeout", String(this.options.timeoutSeconds));
    return args;
  }

  /** A round trip through every hop and back. */
  async ping(): Promise<PingResult> {
    const result = await run(this.options.client, ["ping", ...this.connection()]);
    const reply = lastJson(result.stdout);
    if (result.status !== 0 || !reply) throw new MixnetError(`ping failed: ${result.stderr.trim().slice(-300)}`, result);
    return reply as unknown as PingResult;
  }

  /**
   * Submits a spend (client proof archive + public fields file) through the mixnet to the exit's
   * prover queue. Resolves with the exit's reply; `ok` is false when the exit refused it.
   */
  async submitSpend(proofArchive: string, publicFieldsFile: string): Promise<SpendReceipt & { stderr: string }> {
    const result = await run(this.options.client, [
      "spend",
      ...this.connection(),
      "--proof",
      proofArchive,
      "--public",
      publicFieldsFile,
    ]);
    const reply = lastJson(result.stdout);
    if (!reply) throw new MixnetError(`no reply from the exit: ${result.stderr.trim().slice(-300)}`, result);
    return { ...(reply as unknown as SpendReceipt), ok: reply.ok === true, stderr: result.stderr };
  }
}

/** One spend waiting in an exit's queue directory. */
export interface QueuedSpend {
  id: string;
  fieldsFile: string;
  proofFile: string;
  fields: number[];
}

/** The spends in a queue directory (ID.json with a matching ID.tgz), sorted by id. */
export function readSpendQueue(dir: string): QueuedSpend[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.slice(0, -5))
    .sort()
    .flatMap((id) => {
      const fieldsFile = join(dir, `${id}.json`);
      const proofFile = join(dir, `${id}.tgz`);
      if (!existsSync(proofFile)) return [];
      const parsed = JSON.parse(readFileSync(fieldsFile, "utf8")) as { fields?: unknown };
      if (!Array.isArray(parsed.fields) || !parsed.fields.every((x) => typeof x === "number")) return [];
      return [{ id, fieldsFile, proofFile, fields: parsed.fields as number[] }];
    });
}
