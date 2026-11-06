# @topi/sdk

TypeScript SDK for post-quantum private payments on an EIP-8141 frame-transaction chain: frame
transaction encoding and signing, the shielded pool (events, chain view, deposit and settle
transactions), leanSPHINCS-signed account transactions, and adapters for the prover and the
mixnet client.

ESM only, strict TypeScript, Node 22.18 or later. Runtime dependencies: `@noble/hashes` and
`@noble/curves`.

## Install

```sh
npm install @topi/sdk
```

The package is not on npm yet. The release workflow (`.github/workflows/release.yml`) publishes it
on a `v*` tag only when the repository variable `PUBLISH_SDK` is `true`. Until then, use it from a
checkout.

Inside this repository the package is an npm workspace: `npm install` at the root, then
`npm run build` (emits `dist/` with declarations) and `npm test`. The operational scripts import
`sdk/src/index.ts` directly, which Node runs with type stripping, so they need no build step.

## Examples

### Read the pool and rebuild the chain view

```ts
import { PoolReader, RpcClient, loadPoolDeployment, packDigest } from "@topi/sdk";

const rpc = new RpcClient("https://rpc1.privacy.ethrex.xyz");
const deployment = loadPoolDeployment("deployments/8141/pool.json");
const pool = new PoolReader(rpc, deployment.pool);

for (const event of await pool.events()) {
  if (event.kind === "deposited") console.log(`deposit ${event.amount} wei at leaf ${event.leafIndex}`);
  else console.log(`settle at leaf ${event.leafIndex}`);
}

// Both trees' leaves, current roots and the pool identity: what `pool-wallet witness` reads.
const view = await pool.chainView(deployment);
console.log(await pool.knownPoolRoot(packDigest(view.pool_root))); // true
```

### Build and simulate a frame transaction

```ts
import { APPROVE, MODE, RpcClient, encodeRawHex, randomNonceKey, signTransaction, parsePrivateKey } from "@topi/sdk";
import type { FrameTransaction } from "@topi/sdk";

const rpc = new RpcClient("https://rpc1.privacy.ethrex.xyz");
const tx: FrameTransaction = {
  chainId: await rpc.chainId(),
  nonceKeys: [randomNonceKey()],
  nonceSeq: 0n,
  sender: 0x1111111111111111111111111111111111111111n,
  frames: [
    { mode: MODE.VERIFY, flags: APPROVE.EXECUTION_AND_PAYMENT, target: 0x1111111111111111111111111111111111111111n,
      gasLimit: 100_000n, stateLimit: 0n, value: 0n, data: new Uint8Array(0) },
    { mode: MODE.SENDER, flags: 0n, target: 0xdeadn, gasLimit: 50_000n, stateLimit: 0n, value: 1n, data: new Uint8Array(0) },
  ],
  signatures: [],
  ...(await rpc.feeCaps()),
  maxBlobFee: 0n,
  blobHashes: [],
};
signTransaction(tx, parsePrivateKey(process.env.KEY!)); // secp256k1 over signatureHash(tx)
const result = await rpc.simulateFrameTransaction(encodeRawHex(tx)); // no state change
console.log(result.valid, result.violation);
```

`signatureHash(tx)` elides the bytes of every empty-msg signature, so a signer can hash the
transaction with a placeholder entry and fill it in afterwards. `executionCapUsage(tx)` and
`totalGasLimit(tx)` mirror the chain's gas accounting.

### Submit a settle to an ERC-8421 alternative mempool

```ts
import { RpcClient, fromFrameTxRequest, hasRecentRoots, toFrameTxRequest } from "@topi/sdk";

// A mempool takes the transaction as JSON, not raw bytes, so it runs its own admission rules.
const mempool = new RpcClient("http://127.0.0.1:47500/rpc");
const hash = await mempool.sendFrameTransaction(tx);       // eth_sendFrameTransaction [toFrameTxRequest(tx)]
const status = await mempool.request("skandha_frameTransactionStatus", [hash]);

// Round trip between the JSON shape and the SDK's model.
const same = fromFrameTxRequest(toFrameTxRequest(tx));
```

Chains differ in one field. The organizers' ERC-8421 devnet (chain 1337) encodes EIP-8272 recent-root
references as a ninth field; chain 8141 has 8 fields. `hasRecentRoots(chainId)` says which, and the
transaction builders (`buildSettleTransaction`, `buildAccountTransaction`) set
`recentRootReferences: []` for chain 1337 by themselves. Only the empty list is supported.

### A PQ-signed account transaction

```ts
import { LskSigner, RpcClient, depositCalldata, encodeRawHex, loadAccountDeployment, signedAccountTransaction } from "@topi/sdk";

const rpc = new RpcClient("https://rpc1.privacy.ethrex.xyz");
const account = loadAccountDeployment("deployments/8141/account.json");
const signer = new LskSigner({ binary: process.env.LSK_BIN!, keyFile: ".secrets/pq/user.key" });

const tx = await signedAccountTransaction(
  {
    account: account.account,
    verifySelector: account.verifySelector,
    chainId: await rpc.chainId(),
    fees: await rpc.feeCaps(),
    call: { target: 0xdeadn, value: 1n, data: new Uint8Array(0), gasLimit: 50_000n, stateLimit: 0n },
  },
  signer,
);
// One ARBITRARY entry (empty signer, empty msg) holding the leanSPHINCS witness.
console.log(await rpc.simulateFrameTransaction(encodeRawHex(tx)));
```

Any object with `publicKey()` and `sign(digest)` can replace `LskSigner` (`PQSigner`).

### Submit a spend through the mixnet

```ts
import { MixnetClient, readSpendQueue } from "@topi/sdk";

const mixnet = new MixnetClient({
  client: "target/release/topi-client",
  topology: "data/aws/topology.json",
  identity: ".secrets/member.id",
});
const receipt = await mixnet.submitSpend("send/a.tgz", "public-a.json");
if (receipt.ok) console.log(`queued at the exit as ${receipt.queued}`);

// On the exit side: ID.json + ID.tgz pairs.
for (const spend of readSpendQueue("data/aws/spend-queue")) console.log(spend.id, spend.fields.length);
```

### Prove

```ts
import { Prover } from "@topi/sdk";

// Runs on Linux, or in WSL on Windows; prover/env.sh resolves PROVER_HOME from the repo's .env.
const prover = new Prover({ proverDir: "prover", wslDistro: "Ubuntu", echo: true });
await prover.keygen("note.json");
await prover.witness("note.json", "chain.json", "request.json", "input.json");
await prover.proveClient("input.json", "send/a.tgz");
const ok = await prover.checkSpend("queue/ID.json", "queue/ID.tgz");
const calldata = await prover.proveExit("a.tgz", "b.tgz", "out"); // out/calldata.bin
```

## API overview

| Module | Exports |
| --- | --- |
| `frames` | `FrameTransaction` (optional `recentRootReferences`), `Frame`, `FrameSignature`; `MODE`, `APPROVE`, `ATOMIC_BATCH`, `SCHEME`, `FRAME_TX_TYPE`, `EXECUTION_GAS_CAP`; `encodeFrame`, `encodeEnvelope`, `encodeRaw`, `encodeRawHex`, `decodeRaw`, `signatureHash`, `randomNonceKey`, `arbitrarySignaturePlaceholder`, `hasRecentRoots`; mempool JSON: `FrameTxRequest`, `toFrameTxRequest`, `fromFrameTxRequest`; gas: `mandatoryGas`, `executionCapUsage`, `totalGasLimit`, `calldataFloorGas`, `stateGasLimit`, `maxCost` |
| `rlp` | `rlpBytes`, `rlpInt`, `rlpList`, `rlpEncode`, `rlpDecode`, `rlpItems` |
| `ecdsa` | `parsePrivateKey`, `signHash`, `recoverSigner`, `addressOf`, `checksumAddress`, `signTransaction` |
| `eip1559` | `signEip1559`, `eip1559SigningHash` (plain ECDSA-account transactions) |
| `abi` | `selector`, `eventTopic`, `encodeCall`, `decodeWords`, dynamic-argument tails |
| `rpc` | `RpcClient` (`request`, `chainId`, `latestBlock`, `call`, `getLogs`, `getReceipt`, `waitForReceipt`, `sendRawTransaction`, `sendFrameTransaction`, `simulateFrameTransaction`, `feeCaps`), `RpcError` |
| `pool` | `PoolDeployment`, `AccountDeployment` and loaders; `packDigest`, `unpackDigest`, `packFields`, `contextDigest`, `deriveIdentity`, `SPEND_FIELDS`, `checkSpendFields`; `DEPOSITED_TOPIC`, `SETTLED_TOPIC`, `decodePoolLog`, `buildTrees`; `PoolReader` (roots, nullifiers, events, `checkSpendState`, `chainView`); `depositCalldata`, `settleCalldata`, `buildSettleTransaction`, `defaultSettleLimits`, `signSettleTransaction` |
| `pq` | `PQSigner`, `LskSigner`, `decodeLskSignature`, `LEANSPHINCS_SIGNATURE_BYTES`, `addArbitraryWitness`, `buildAccountTransaction`, `signedAccountTransaction` |
| `prover` | `Prover` (`keygen`, `witness`, `checkSpend`, `proveClient`, `proveExit`), `ProverError` |
| `mixnet` | `MixnetClient` (`ping`, `submitSpend`), `readSpendQueue`, `MixnetError` |
| `config` | `loadConfig` (loads `<root>/.env` under Node and Bun; `get`, `require`, `path`, `resolve`), `resolvePath` |
| `process` | `run`, `LinuxShell`, `toWslPath` |

Everything is also exported from the package root. Integers that the protocol treats as
uint256 are `bigint`; addresses in deployment records are `0x` strings.

## Licence

MIT OR Apache-2.0. Parts of the frame-transaction code derive from
soispoke/minimal-shielded-pool (Apache-2.0); see `NOTICE`.
