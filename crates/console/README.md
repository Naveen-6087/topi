# topi

`topi`: the terminal for the whole system. A post-quantum wallet, the mixnet on a world map, the exit
prover, the ERC-8421 alt mempool and the chain, with every action streaming into a jobs pane.

## Install

```bash
cargo install --locked --git https://github.com/Naveen-6087/topi topi-tui topi-mixnet   # topi, topi-node, topi-client
topi                                                                                  # from any folder
```

From a checkout: `cargo install --path crates/console && cargo install --path crates/mixnet`. Prebuilt
`topi`, `topi-node` and `topi-client` for Linux, macOS and Windows come with each GitHub release
(`.github/workflows/release.yml`, on a `v*` tag).

`topi` carries what it runs besides its own code: the prover scripts, the alt mempool sidecar's setup,
patch, rules and admission policy (with the TypeScript SDK it imports), the AWS scripts, the deployment
records and the contracts' bytecode. Run outside a checkout, it unpacks them into `~/.topi` (or
`TOPI_HOME`) and keeps its data and keys there; inside a checkout it uses the checkout.
`TOPI_BUNDLED=1` forces the unpacked copy. Settings come from `.env` in that root (see `.env.example`).

External tools it drives: WSL on Windows (the prover and the sidecar run on Linux), Bun (sidecar),
`lsk` (leanSPHINCS signing, `LSK_BIN`), and the AWS CLI with Terraform for the cloud network.

## Tabs and keys

| Tab | Shows | Keys |
|---|---|---|
| 1 Wallet | accounts, private notes and their stage, the exit prover | `c` account, `n` note, `d` deposit, `w` witness, `p` prove, `s` send, `r` settle, `m` alt mempool, `e` mempool explorer, `F` fund, `D` full demo |
| 2 Mixnet map | the nodes on a world map with packets in flight | `l` start local, `k` stop local, `p` ping, `z` zoom |
| 3 Chain | balances, the pool's deposits and settles | `o` the chain's explorer (Dora on 8141) |

`F` tops the depositing key and the paymaster up to 0.04 ETH from where demo money collects (console
accounts and the leanSPHINCS PQ account). `D` runs the whole private payment for demos: funding first,
an account if there is none, the local mixnet, two notes
deposited, witnessed, proven and sent through the mixnet, then the exit prover settles them (through
the alt mempool when it runs). About 5 minutes with `EXIT_PROVER=aws` (see `infra/aws/README.md`),
about 20 locally.

`m` starts two nodes of the topi privacy-pool mempool (`mempool/README.md`) that gossip with each
other; `r` and `D` then settle through them. `e` opens explorer.altmempool.wtf on them, every
admission rule by rule.

Global keys: `tab` or `1`-`3` switch tabs, `j` shows the jobs pane, `[` `]` pick a job, `x` stops it,
`o` opens the website (the explorer on the Chain tab) and `g` the repository, `q` quits (local nodes and
the mempool stop with it).
