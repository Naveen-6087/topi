// Generates src/ShieldedPool.sol from codegen/ShieldedPool.tmpl.sol by filling in the empty-subtree
// tables of the pool tree (zeros.txt) and the association tree (zeros-association.txt). Both tables
// are printed by test/Codegen.t.sol. Usage (from contracts/): node codegen/generate.mjs
import { readFileSync, writeFileSync } from "node:fs";

const hex = (f) =>
  readFileSync(new URL(f, import.meta.url), "utf8")
    .trim()
    .split(/\s+/)
    .map((l) => l.replace(/^0x/, ""))
    .join("");
const pool = hex("./zeros.txt");
const assoc = hex("./zeros-association.txt");
if (pool.length !== 33 * 64 || assoc.length !== 33 * 64) throw new Error("zero tables must hold 33 words each");

const src = readFileSync(new URL("./ShieldedPool.tmpl.sol", import.meta.url), "utf8")
  .replace("__POOL_ZEROS__", pool)
  .replace("__ASSOCIATION_ZEROS__", assoc);
const header =
  "// This file is GENERATED from codegen/ShieldedPool.tmpl.sol by codegen/generate.mjs. Edit the template.\n";
const nl = src.indexOf("\n") + 1; // keep the SPDX line first
writeFileSync(new URL("../src/ShieldedPool.sol", import.meta.url), src.slice(0, nl) + header + src.slice(nl));
console.log("wrote src/ShieldedPool.sol");
