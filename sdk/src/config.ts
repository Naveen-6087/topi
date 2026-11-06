/**
 * Settings from a repository's `.env` and the process environment, and repo-relative paths.
 */
import { existsSync, readFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";

export interface Config {
  /** The directory relative paths resolve against. */
  readonly root: string;
  /** A setting, or `fallback` when unset or empty. */
  get(key: string, fallback?: string): string | undefined;
  /** A setting that must be present. */
  require(key: string): string;
  /** A path setting resolved against `root`, or `fallback` resolved the same way. */
  path(key: string, fallback?: string): string | undefined;
  /** A path setting that must be present. */
  requirePath(key: string): string;
  /** Resolves a path against `root` (absolute paths pass through). */
  resolve(path: string): string;
}

export interface LoadConfigOptions {
  /** The env file, relative to root. Default ".env". Missing files are fine. */
  envFile?: string;
  /** Variables to read instead of process.env (process.env is still where .env is loaded). */
  env?: NodeJS.ProcessEnv;
}

/** Resolves `path` against `root` unless it is absolute. */
export function resolvePath(root: string, path: string): string {
  return isAbsolute(path) ? path : resolve(join(root, path));
}

/**
 * Loads `<root>/.env` into process.env (variables already set win) and returns accessors.
 */
export function loadConfig(root: string, options: LoadConfigOptions = {}): Config {
  const file = join(root, options.envFile ?? ".env");
  if (existsSync(file)) loadEnvFile(file);
  const env = options.env ?? process.env;
  const get = (key: string, fallback?: string) => {
    const value = env[key];
    return value === undefined || value === "" ? fallback : value;
  };
  const config: Config = {
    root: resolve(root),
    get,
    require(key) {
      const value = get(key);
      if (value === undefined) throw new Error(`${key} is not set (add it to ${file} or the environment)`);
      return value;
    },
    path(key, fallback) {
      const value = get(key, fallback);
      return value === undefined ? undefined : resolvePath(root, value);
    },
    requirePath(key) {
      return resolvePath(root, config.require(key));
    },
    resolve: (path) => resolvePath(root, path),
  };
  return config;
}

/** `process.loadEnvFile` where the runtime has it (Node 20.12+), else the same KEY=VALUE parsing (Bun). */
function loadEnvFile(file: string): void {
  if (typeof process.loadEnvFile === "function") {
    process.loadEnvFile(file);
    return;
  }
  for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
    const m = /^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/.exec(line);
    if (!m || line.trimStart().startsWith("#")) continue;
    let value = m[2];
    if (/^(["']).*\1$/.test(value)) value = value.slice(1, -1);
    else value = value.replace(/\s+#.*$/, "");
    if (process.env[m[1]] === undefined) process.env[m[1]] = value;
  }
}
