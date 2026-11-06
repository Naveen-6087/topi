# Alternative mempool

The shielded pool's settle transactions travel through an ERC-8421 alternative mempool: the topi
privacy-pool mempool, defined by [RULES.md](RULES.md). Its id is that document's IPFS hash:

```bash
node mempool/src/mempool-id.ts        # QmSwvDuyR1MBau6NdGtWv91dcrz6doYvVfZDTWNFk9wNRh for this RULES.md
```

A settle cannot use the public mempool. Every settle has the same sender (the pool) and the same
paymaster, while the public mempool holds one pending transaction per sender and per non-canonical
paymaster. Its validation prefix also verifies a 5,684-byte leanSPHINCS post-quantum witness,
which with the pool's own check needs 160,100 validation gas (pool 10,000, paymaster 150,000, signature 100), more than the public 100,000.

## The sidecar

The mempool runs on the ERC-8421 sidecar ([forshtat/mempool-workshop-sidecar](https://github.com/forshtat/mempool-workshop-sidecar),
the skandha fork for EIP-8141 frame transactions), pinned to one commit and extended by
[sidecar/sidecar.patch](sidecar/sidecar.patch):

| Setting | What the patch adds |
|---|---|
| `maxVerifyGas` | this mempool's validation budget (200,000), in place of the fixed 100,000 |
| `admissionPolicy` | a module with the mempool's own rules ([src/policy.ts](src/policy.ts)), run after the sidecar's checks at admission and on every block, reported as the explorer's `policy` rule |
| `traceSimulation: false` | nodes without ethrex's tracer: simulate untraced, admit only whitelisted entities (the pool and its paymasters), whose rules the policy supplies |
| `eip8272: false` | chains whose frame transactions have 8 fields (no recent-root references), such as chain 8141 |
| `rpcEndpointSubmitMethod: "eth_sendRawTransaction"` | relay to a public node instead of a co-located ethrex's privileged method |

The policy is the same code as this package's own mempool (`src/mempool.ts`, `src/rules.ts`): the
settle's shape, pool and paymaster code hashes, nullifiers unspent and not already pending, roots
known, and the leanSPHINCS witness verified under the exit key on the shared verifier.

```bash
bash mempool/sidecar/setup.sh            # Linux or WSL: clone the pinned commit, apply the patch, build (Bun)
bash mempool/sidecar/start.sh            # http://127.0.0.1:47500/rpc, relaying to RPC_URL
bash mempool/sidecar/start.sh --p2p                                  # node A: gossip on 4337
bash mempool/sidecar/start.sh --port 47501 --p2p-port 4338 --join 47500   # node B: dials node A
```

The console's `m` key (Wallet tab) starts both nodes (through WSL on Windows); `r` then settles through
them: a settle submitted to node A reaches node B by gossip, and both relay it (a duplicate relay is not
an error).

On chain 8141 the sidecar reads the ERC-8421 staking registry in `deployments/8141/staking.json` (the
devnet's `NaiveStakingRegistry`, `contracts/src/staking/`). The deployed pool account and paymaster
cannot stake themselves (staking is keyed on the caller and they have no call to `addStake`), so they
stay whitelisted entities; a later version of either contract can stake. Validation is not traced on
8141: its ethrex has no ERC-7562 tracer, so the whitelist plus this mempool's own rules stand in for the
opcode and storage checks. Scripts take `--mempool http://127.0.0.1:47500/rpc` or `MEMPOOL_URL`.

| JSON-RPC method | Does |
|---|---|
| `eth_sendFrameTransaction [tx]` | submits a settle as JSON (`toFrameTxRequest` in the SDK); the error names the rule it broke |
| `skandha_frameTransactionStatus [hash]` | `New`, `Submitted`, `OnChain`, `Reverted`, ... |
| `skandha_explorer_listAdmissions [{}]` | every submission with each rule's verdict |
| `skandha_explorer_listTransactions [{}]` | the pool and its history |
| `skandha_config []` | the node's settings |

## On the organizers' ERC-8421 devnet (chain 1337)

The same post-quantum settle shape also runs through the public alt mempool devnet. There, settles go
to the devnet's own sidecars, which apply the standard ERC-8421 rules with full ERC-7562 tracing; no
custom rules are involved. The devnet's 60M block gas limit cannot hold the terminal proof verifier or
the pool logic, so `deployments/1337/pool.json` pairs the real PoolAccount, PQPaymaster and leanSPHINCS
verifier with `DevnetSettle`, a compact logic that records nullifiers but does not verify the proof
(that happens on chain 8141). `deployments/1337/genesis/` packages the full pool for the devnet's
genesis instead.

```bash
RPC_URL=https://altmempool.forshtat.com node scripts/settle.ts --deployment deployments/1337/pool.json \
  --calldata CALLDATA.bin --fields VECTORS.json --paymaster-gas 95000 --settle-gas 3000000 \
  --send --mempool https://altmempool.forshtat.com/rpc
```

| | |
|---|---|
| Admitted and included | [`0x763107be…f5b21a152`](https://explorer.altmempool.wtf/tx/0x763107bea261cf6add7240d634540da223ba748aeed73458db3b085f5b21a152?node=https://altmempool.forshtat.com/rpc), block 8944: 24 of 24 admission checks passed, all three frames succeeded |
| Validation | pool 10,000 + leanSPHINCS paymaster 95,000 limits, each within the unstaked per-entity budget |
| Encoding | frame transactions there carry a ninth field (EIP-8272 recent roots); the SDK adds it for chain 1337 |

The devnet gateway admits these settles too, but does not pass a 110 KB transaction on to the miner,
so submit to the miner's sidecar (`/rpc` on `altmempool.forshtat.com`) for inclusion.

## A lighter mempool for development

`src/main.ts` is a self-contained mempool with the same rules, without the sidecar: JSON-RPC
(`eth_sendFrameTransaction`, `eth_sendRawTransaction`, `altmempool_status`, `altmempool_pending`,
`altmempool_rules`, `GET /metrics`), gossip between peers, and release to the chain one settle at a
time.

```bash
node mempool/src/main.ts --port 47501 --peers http://10.0.0.2:47501
```

## Tests

```bash
npm test --workspace mempool                          # rules, admission, release, gossip, RPC, mempool id
TOPI_LIVE=1 node --test mempool/test/live.test.ts     # replays mined settles from chain 8141
```
