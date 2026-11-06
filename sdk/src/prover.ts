/**
 * Adapter for the prover workspace (`prover/` in the repository): the `pool-wallet` binary
 * (keys, witnesses, spend checks) and the client and exit proving scripts. They run on Linux,
 * or in WSL on Windows. `prover/env.sh` reads the repository's `.env` (PROVER_HOME) and
 * defines WALLET_BIN; every command here sources it first.
 */
import { join } from "node:path";

import { LinuxShell, type RunResult } from "./process.ts";

export interface ProverOptions {
  /** The directory holding env.sh, prove-client.sh and prove-exit.sh (the repo's `prover/`). */
  proverDir: string;
  /** On Windows, the WSL distribution. Default "Ubuntu". */
  wslDistro?: string;
  /** Override the wallet binary instead of env.sh's WALLET_BIN (a path as the shell sees it). */
  walletBin?: string;
  /** Forward tool output as it arrives. Default false. */
  echo?: boolean;
  /** Override the platform check (for tests). */
  platform?: NodeJS.Platform;
}

export class ProverError extends Error {
  readonly result: RunResult;
  constructor(what: string, result: RunResult) {
    const tail = (result.stderr || result.stdout).trim().split("\n").slice(-5).join("\n");
    super(`${what} failed (exit ${result.status})${tail ? `:\n${tail}` : ""}`);
    this.name = "ProverError";
    this.result = result;
  }
}

export class Prover {
  readonly shell: LinuxShell;
  private readonly options: ProverOptions;

  constructor(options: ProverOptions) {
    this.options = options;
    this.shell = new LinuxShell({ wslDistro: options.wslDistro, platform: options.platform });
  }

  /** Runs `script` after sourcing env.sh; $1.. are `args`. */
  private async bash(what: string, script: string, args: string[], echo = this.options.echo): Promise<RunResult> {
    const envPath = this.shell.path(join(this.options.proverDir, "env.sh"));
    const walletOverride = this.options.walletBin ? `WALLET_BIN=${quote(this.options.walletBin)} && ` : "";
    const result = await this.shell.run(`. "$1" && shift && ${walletOverride}${script}`, [envPath, ...args], { echo });
    if (result.status !== 0) throw new ProverError(what, result);
    return result;
  }

  /** `pool-wallet keygen OUT`: fresh note secrets (Poseidon2 precommitment). */
  async keygen(outFile: string): Promise<void> {
    await this.bash("pool-wallet keygen", `"$WALLET_BIN" keygen "$1"`, [this.shell.path(outFile)]);
  }

  /** `pool-wallet witness NOTE CHAIN REQUEST OUT`: the withdrawal's witness and public fields. */
  async witness(noteFile: string, chainFile: string, requestFile: string, outFile: string): Promise<void> {
    await this.bash("pool-wallet witness", `"$WALLET_BIN" witness "$1" "$2" "$3" "$4"`, [
      this.shell.path(noteFile),
      this.shell.path(chainFile),
      this.shell.path(requestFile),
      this.shell.path(outFile),
    ]);
  }

  /**
   * Checks that a spend's public fields hash to its client proof's statement: unpacks the proof
   * into a temporary directory and runs `pool-wallet check FIELDS DIR`.
   */
  async checkSpend(fieldsFile: string, proofArchive: string): Promise<boolean> {
    const script =
      `dir=$(mktemp -d) && trap 'rm -rf "$dir"' EXIT && tar -xzf "$2" -C "$dir" && "$WALLET_BIN" check "$1" "$dir"`;
    try {
      const result = await this.bash("pool-wallet check", script, [this.shell.path(fieldsFile), this.shell.path(proofArchive)], false);
      return result.stdout.includes('"ok":true');
    } catch (error) {
      if (error instanceof ProverError) return false;
      throw error;
    }
  }

  /** prove-client.sh INPUT OUT.tgz: the full-ZK client proof for one withdrawal. */
  async proveClient(inputFile: string, outArchive: string): Promise<RunResult> {
    return this.bash("prove-client.sh", `bash "$1" "$2" "$3"`, [
      this.shell.path(join(this.options.proverDir, "prove-client.sh")),
      this.shell.path(inputFile),
      this.shell.path(outArchive),
    ]);
  }

  /**
   * prove-exit.sh A.tgz B.tgz OUT_DIR: leanVM recursion over two client proofs to terminal
   * calldata. Returns the path of OUT_DIR/calldata.bin. Slow (minutes) and memory-hungry.
   */
  async proveExit(proofA: string, proofB: string, outDir: string): Promise<string> {
    await this.bash("prove-exit.sh", `bash "$1" "$2" "$3" "$4"`, [
      this.shell.path(join(this.options.proverDir, "prove-exit.sh")),
      this.shell.path(proofA),
      this.shell.path(proofB),
      this.shell.path(outDir),
    ]);
    return join(outDir, "calldata.bin");
  }
}

const quote = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;
