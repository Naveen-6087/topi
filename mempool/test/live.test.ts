// Replays settles already mined on chain 8141 through the mempool. Needs the network, so it runs only
// with TOPI_LIVE=1. Each one must rebuild byte for byte, pass every shape rule and the paymaster's
// witness check, and be refused for exactly one reason: its nullifiers are already spent.
import assert from "node:assert/strict";
import { test } from "node:test";

import { RpcClient, encodeRaw, toHex, keccak256 } from "../../sdk/src/index.ts";

import { RpcChain } from "../src/chain.ts";
import { Mempool } from "../src/mempool.ts";
import { fromRpcTransaction, type RpcFrameTransaction } from "../src/rpc-tx.ts";
import { Rejection, checkSettle, policyFor } from "../src/rules.ts";
import { deployment } from "./fixtures.ts";

const live = process.env.TOPI_LIVE === "1";
const RPC = process.env.RPC_URL ?? "https://rpc1.privacy.ethrex.xyz";
// Settles in blocks 612,907 (the deployment's paymaster) and 710,082 (a second PQ paymaster).
const SETTLES = [
  "0xc46faf007b8fd5720f77179af97f66b80ebd1281cc5814345a5448bc1258c680",
  "0xaf4b70e8f7fffeaaf1c3ecbf5b0ef27909b08ce6d097b6f766b11ae09c6c0c7f",
] as const;
const SECOND_PAYMASTER = 0x98f4e0c284fbb848adccb4470f74d08054569ea4n;

test("mined settles replay through the mempool and are refused only as already spent", { skip: !live && "set TOPI_LIVE=1" }, async () => {
  const rpc = new RpcClient(RPC);
  const policy = policyFor(deployment, [SECOND_PAYMASTER]);
  const mempool = new Mempool({ policy, chain: new RpcChain(rpc) });
  await mempool.start();
  for (const hash of SETTLES) {
    const tx = fromRpcTransaction(await rpc.request<RpcFrameTransaction>("eth_getTransactionByHash", [hash]));
    const raw = encodeRaw(tx);
    assert.equal(toHex(keccak256(raw)), hash, "the settle rebuilds byte for byte");
    checkSettle(tx, policy);
    await assert.rejects(mempool.admit(raw), (e: unknown) => e instanceof Rejection && e.code === "nullifier-spent");
  }
  assert.deepEqual(mempool.counters.rejected, { "nullifier-spent": SETTLES.length });
});
