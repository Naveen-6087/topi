// Shared plumbing for the operational scripts: repository root, settings, CLI parsing, the
// pipeline log, signers and default paths. Everything machine-specific comes from `.env`.
import { appendFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { parseArgs, type ParseArgsConfig } from "node:util";

import {
  LskSigner,
  RpcClient,
  loadAccountDeployment,
  loadConfig,
  loadPoolDeployment,
  parsePrivateKey,
  type AccountDeployment,
  type PoolDeployment,
} from "../../sdk/src/index.ts";

/** The repository root, derived from this file's location. */
export const ROOT = join(import.meta.dirname, "..", "..");
export const config = loadConfig(ROOT);

const exe = process.platform === "win32" ? ".exe" : "";

/** Settings with their defaults (repo-relative paths resolve against ROOT). */
export const settings = {
  rpcUrl: () => config.get("RPC_URL", "https://rpc1.privacy.ethrex.xyz")!,
  /** An alternative mempool (mempool/) to submit settles through; unset sends straight to RPC_URL. */
  mempoolUrl: () => config.get("MEMPOOL_URL"),
  poolDeployment: () => config.path("POOL_DEPLOYMENT", "deployments/8141/pool.json")!,
  accountDeployment: () => config.path("ACCOUNT_DEPLOYMENT", "deployments/8141/account.json")!,
  deployerKey: () => config.path("DEPLOYER_KEY", ".secrets/deployer.key")!,
  /** ECDSA operator key for a classic (ECDSA) paymaster; defaults to the deployer key. */
  operatorKey: () => config.path("OPERATOR_KEY") ?? settings.deployerKey(),
  exitPqKey: () => config.path("EXIT_PQ_KEY", ".secrets/pq/exit.key")!,
  accountPqKey: () => config.path("ACCOUNT_PQ_KEY", ".secrets/pq/user.key")!,
  memberIdentity: () => config.path("MEMBER_IDENTITY", ".secrets/member.id")!,
  lskBin: () => config.get("LSK_BIN", "lsk")!,
  wslDistro: () => config.get("WSL_DISTRO", "Ubuntu")!,
  mixnetClient: () => config.path("MIXNET_CLIENT", `target/release/topi-client${exe}`)!,
  leanPolicy: () => config.path("LEAN_POLICY_BIN", `lean/.lake/build/bin/pool-policy${exe}`)!,
  proverDir: () => join(ROOT, "prover"),
  walletDir: () => join(ROOT, "data", "wallet"),
  demoDir: () => config.path("DEMO_WALLET_DIR") ?? join(ROOT, "data", "wallet", "demo"),
  topology: (network: string) => join(ROOT, "data", network, "topology.json"),
  spendQueue: (network: string) => join(ROOT, "data", network, "spend-queue"),
  pipelineLog: () => join(ROOT, "logs", "pipeline.log"),
};

export const rpc = () => new RpcClient(settings.rpcUrl());

/** The alternative mempool to submit settles through: the flag, else MEMPOOL_URL, else none. */
export const mempoolClient = (flag?: string) => {
  const url = flag ?? settings.mempoolUrl();
  return url ? new RpcClient(url) : undefined;
};

/** A path relative to the repository root, for messages. */
export const rel = (path: string) => relative(ROOT, path).replace(/\\/g, "/") || ".";

/** Appends `<unix-seconds> <source> <message>` to logs/pipeline.log (the console tracks it). */
export function pipelineLog(source: string, message: string): void {
  const file = settings.pipelineLog();
  mkdirSync(dirname(file), { recursive: true });
  appendFileSync(file, `${Math.floor(Date.now() / 1000)} ${source} ${message}\n`);
}

export const readJson = <T = any>(file: string): T => JSON.parse(readFileSync(file, "utf8")) as T;

export const loadPool = (file?: string): PoolDeployment => loadPoolDeployment(file ?? settings.poolDeployment());
export const loadAccount = (file?: string): AccountDeployment => loadAccountDeployment(file ?? settings.accountDeployment());

/** An ECDSA key from a key file; the key is never printed. */
export const readEcdsaKey = (file: string): Uint8Array => {
  try {
    return parsePrivateKey(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${rel(file)}: ${(error as Error).message}`);
  }
};

export const lskSigner = (keyFile: string) => new LskSigner({ binary: settings.lskBin(), keyFile });

type Options = NonNullable<ParseArgsConfig["options"]>;

/**
 * Parses argv with node:util parseArgs. `--help` / `-h` prints `usage` and exits 0; a parse
 * error prints it to stderr and exits 2.
 */
export function cli<const O extends Options>(usage: string, options: O, argv = process.argv.slice(2)) {
  if (argv.includes("--help") || argv.includes("-h")) {
    console.log(usage.trim());
    process.exit(0);
  }
  try {
    return parseArgs({ args: argv, options, allowPositionals: true as const, strict: true as const });
  } catch (error) {
    console.error(`${(error as Error).message}\n\n${usage.trim()}`);
    process.exit(2);
  }
}

/** Exits 2 with the usage text. */
export function usageError(usage: string, message: string): never {
  console.error(`${message}\n\n${usage.trim()}`);
  process.exit(2);
}

/** Runs an async main, printing errors without stack noise. */
export function main(fn: () => Promise<void>): void {
  fn().catch((error: unknown) => {
    console.error(error instanceof Error ? `error: ${error.message}` : error);
    process.exit(1);
  });
}

/** A decimal or 0x integer option. */
export const bigintOption = (value: string | undefined, fallback: bigint, what: string): bigint => {
  if (value === undefined) return fallback;
  if (!/^(0x[0-9a-fA-F]+|[0-9]+)$/.test(value)) throw new Error(`${what} must be an integer`);
  return BigInt(value);
};
