import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { addressOf, checksumAddress, parsePrivateKey } from "../../sdk/src/index.ts";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: derive-key.ts <account_dir>");
  process.exit(1);
}

mkdirSync(dir, { recursive: true });
const keyFile = join(dir, "account.key");
if (!existsSync(keyFile)) {
  const priv = randomBytes(32).toString("hex");
  writeFileSync(keyFile, priv + "\n", { mode: 0o600 });
}

const key = parsePrivateKey(readFileSync(keyFile, "utf8"));
const addr = checksumAddress(addressOf(key));
console.log(addr);
