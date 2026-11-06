# Operational scripts

Command-line tools for the shielded pool, built on `@topi/sdk` (`../sdk`). Run them from the
repository root with Node 22.18 or later (type stripping; no build step):

```sh
npm install                      # once, at the repository root
node scripts/wallet.ts --help
```

Every script takes `--help`. Settings come from `.env` at the repository root (copy
`.env.example`); paths in it are relative to the root. State-changing commands only simulate
unless told to broadcast (`--send`), except the `demo.ts` steps, which act by default.

| Setting | Used for | Default |
| --- | --- | --- |
| `RPC_URL` | every chain call | `https://rpc1.privacy.ethrex.xyz` |
| `POOL_DEPLOYMENT`, `ACCOUNT_DEPLOYMENT` | deployment records | `deployments/8141/pool.json`, `deployments/8141/account.json` |
| `DEPLOYER_KEY` | ECDSA deployer, deposits from a key, classic operator | `.secrets/deployer.key` |
| `OPERATOR_KEY` | ECDSA operator of a classic paymaster | `DEPLOYER_KEY` |
| `EXIT_PQ_KEY`, `ACCOUNT_PQ_KEY` | leanSPHINCS key files (exit operator, PQ account) | `.secrets/pq/exit.key`, `.secrets/pq/user.key` |
| `LSK_BIN` | leanSPHINCS signer | `lsk` |
| `MEMBER_IDENTITY` | mixnet member identity | `.secrets/member.id` |
| `MIXNET_CLIENT` | mixnet client binary | `target/release/topi-client` |
| `LEAN_POLICY_BIN` | Lean policy model | `lean/.lake/build/bin/pool-policy` |
| `MEMPOOL_URL` | ERC-8421 alt mempool for settles (`exit-prover.ts`, `settle.ts`) | unset: the node at `RPC_URL` |
| `EXIT_PROVER` | where the recursion runs: `local` or `aws` (`infra/aws/prover.sh`) | `local` |
| `PROVER_HOME`, `WSL_DISTRO` | prover workspace (read by `prover/env.sh`), WSL distribution on Windows | see `.env.example` |

Data locations: wallet files in `data/wallet/` (demo notes in `data/wallet/demo/`), spend queues
in `data/aws/spend-queue/` (fetched from the AWS exit) and `data/local/spend-queue/` (local
network), topologies in `data/aws/topology.json` and `data/local/topology.json`, progress in
`logs/pipeline.log` as `<unix-seconds> <source> <message>` lines.

## Scripts

### `wallet.ts`: chain side of the wallet

```sh
node scripts/wallet.ts chain [--out data/wallet/chain.json] [--from-block N] [--deployment FILE]
node scripts/wallet.ts deposit-pq --note NOTE.json [--amount WEI] [--account FILE] [--pq-key FILE] [--send]
node scripts/wallet.ts deposit --note NOTE.json [--amount WEI] [--key FILE] [--send]
```

`chain` rebuilds both trees from the pool's `Deposited` and `Settled` logs and writes the chain
view `pool-wallet witness` reads; it checks that the pool knows the current roots. It is
read-only. `deposit-pq` deposits from the leanSPHINCS PQ account (frame transaction),
`deposit` from an ECDSA key (EIP-1559); with `--send` the note file is updated with amount,
label, leaf index, transaction and block.

### `demo.ts`: one private spend, step by step

```sh
node scripts/demo.ts status
node scripts/demo.ts new NAME
node scripts/demo.ts deposit NAME [--amount WEI] [--dry-run]
node scripts/demo.ts witness NAME
node scripts/demo.ts prove NAME
node scripts/demo.ts send NAME [--topology FILE | --network aws|local]
```

The console's Wallet tab runs these. Notes live in `data/wallet/demo/`. The PQ account deposits
and receives; `witness` withdraws 40% to it, pays 10% to the exit and keeps 50% as change.
`send` goes through the mixnet (default topology `data/aws/topology.json`).

### `exit-prover.ts`: queued spends to settled transactions

```sh
node scripts/exit-prover.ts [--queue DIR] [--deployment FILE] [--key FILE] [--check-only] [--no-send]
                             [--mempool URL] [--prover local|aws]
```

Checks every queued spend (pool, scope, fee recipient, known roots, unspent nullifier, fields
against the proof statement with `pool-wallet check`), moves failures to `QUEUE/rejected`, runs
the recursion on the first ready pair (`prover/prove-exit.sh`: about 19 minutes and 12 GB on a
laptop, about 151 s with `--prover aws`), and settles it, through the alt mempool when `--mempool`
or `MEMPOOL_URL` is set, moving both to `QUEUE/done`. The default queue is `data/aws/spend-queue`.

### `settle.ts`: settle two proven spends

```sh
node scripts/settle.ts --calldata calldata.bin --fields vectors.json [--variant NAME] [--send] [--mempool URL]
```

Builds `[VERIFY pool, VERIFY paymaster, SENDER settle]`, signs it as the paymaster's operator
(leanSPHINCS witness or ECDSA, per the deployment), prints the declared execution against the
2^24 cap and the simulation. Variants `no-signature`, `other-target`, `deposit-selector`,
`extra-frame` and `corrupt-pq` are negative cases; frame limits can be overridden with
`--pool-gas`, `--paymaster-gas`, `--paymaster-state` and `--settle-gas`.

### `account-send.ts`: a call from the PQ account

```sh
node scripts/account-send.ts --target ADDRESS [--value WEI] [--data HEX] [--corrupt | --wrong-key FILE] [--send]
```

### `differential/policy.ts` and `differential/account.ts`: Lean model against the chain

```sh
node scripts/differential/policy.ts [--deployment FILE] [--count 20] [--seed 1]
node scripts/differential/account.ts [--account FILE]
```

Each builds valid transactions and mutations of them, asks the chain
(`ethrex_simulateFrameTransaction`, no state change) and the Lean model (`pool-policy`) whether
each is valid, and reports agreement. They exit 1 on any disagreement.

### `deploy.sh`: deploy a pool

```sh
scripts/deploy.sh NAME [--pq-key FILE] [--testbed] [--terminal ADDR] [--fund ETH] [--out FILE]
```

Deploys `ShieldedPool`, the `PoolAccount` and a `PQPaymaster` (with `--pq-key`) or
`ClassicPaymaster`, funds the paymaster and writes `deployments/<chain>/pool-NAME.json`. Needs
`forge`, `cast` and `node`, and the Yul builds in `contracts/yul/build/`.

## End-to-end flow

1. `demo.ts new a` and `demo.ts new b`: note secrets.
2. `demo.ts deposit a`, `demo.ts deposit b`: deposits from the PQ account.
3. `demo.ts witness a`, `demo.ts witness b`: chain view and withdrawal witnesses.
4. `demo.ts prove a`, `demo.ts prove b`: client proofs.
5. `demo.ts send a`, `demo.ts send b`: through the mixnet into the exit's queue.
6. `infra/aws/deploy.sh fetch-queue` (AWS network) copies the queue to `data/aws/spend-queue`.
7. `exit-prover.ts --queue data/aws/spend-queue`: checks, recursion, settle.

`logs/pipeline.log` records each stage; the console's pipeline view follows it.
