/**
 * Running external tools, including Linux tools through WSL on Windows.
 */
import { spawn } from "node:child_process";
import { resolve, win32 } from "node:path";

export interface RunResult {
  status: number | null;
  stdout: string;
  stderr: string;
}

export interface RunOptions {
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  /** Forward output to this process's stdout/stderr as it arrives (it is still captured). */
  echo?: boolean;
  /** Text written to the child's stdin. */
  input?: string;
}

/** Runs a command to completion, capturing output. Never throws on a non-zero exit. */
export function run(command: string, args: readonly string[], options: RunOptions = {}): Promise<RunResult> {
  return new Promise((done, fail) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env ?? process.env,
      stdio: [options.input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    // WSL relays can interleave NUL bytes; drop them.
    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      const text = chunk.replace(/\0/g, "");
      stdout += text;
      if (options.echo) process.stdout.write(text);
    });
    child.stderr?.on("data", (chunk: string) => {
      const text = chunk.replace(/\0/g, "");
      stderr += text;
      if (options.echo) process.stderr.write(text);
    });
    if (options.input !== undefined) child.stdin?.end(options.input);
    child.on("error", fail);
    child.on("close", (status) => done({ status, stdout, stderr }));
  });
}

/** A Windows path as WSL sees it (C:\x\y -> /mnt/c/x/y); other paths are returned resolved. */
export function toWslPath(path: string): string {
  // A drive path is already absolute, whichever platform this runs on.
  const absolute = /^[A-Za-z]:[\\/]/.test(path) ? win32.normalize(path) : resolve(path);
  const drive = /^([A-Za-z]):[\\/]?(.*)$/.exec(absolute);
  if (!drive) return absolute.replace(/\\/g, "/");
  return `/mnt/${drive[1].toLowerCase()}/${drive[2].replace(/\\/g, "/")}`;
}

export interface LinuxShellOptions {
  /** On Windows, the WSL distribution to run in. Default "Ubuntu". */
  wslDistro?: string;
  /** Override the platform check (for tests). */
  platform?: NodeJS.Platform;
}

/**
 * A bash runner for Linux tools: bash directly on Linux and macOS, `wsl -d DISTRO -- bash` on
 * Windows. `script` receives `args` as $1, $2, ...; `paths` converts a local path for it.
 */
export class LinuxShell {
  readonly windows: boolean;
  readonly distro: string;

  constructor(options: LinuxShellOptions = {}) {
    this.windows = (options.platform ?? process.platform) === "win32";
    this.distro = options.wslDistro ?? "Ubuntu";
  }

  /** A local path as the shell sees it. */
  path(local: string): string {
    return this.windows ? toWslPath(local) : resolve(local);
  }

  /** Runs `bash -c script` with positional arguments. */
  run(script: string, args: readonly string[] = [], options: RunOptions = {}): Promise<RunResult> {
    const bashArgs = ["-c", script, "sdk-shell", ...args];
    if (!this.windows) return run("bash", bashArgs, options);
    // MSYS_NO_PATHCONV keeps Git Bash from rewriting /mnt/... arguments.
    const env = { ...(options.env ?? process.env), MSYS_NO_PATHCONV: "1" };
    // --exec passes argv to bash as is (plain `wsl --` re-parses it through the login shell).
    return run("wsl", ["-d", this.distro, "--exec", "bash", ...bashArgs], { ...options, env });
  }
}
