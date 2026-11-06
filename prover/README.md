# Prover

Scripts that produce the pool's proofs, on Linux or in WSL on Windows.

| Proof | Made by | Cost | Script |
|---|---|---|---|
| Client proof | the wallet | about 1 s to prove, 450 MB | `prove-client.sh INPUT.json OUT.tgz` |
| Exit proof | the exit | about 19 minutes and 12 GB on a laptop; 151 s on a 32-vCPU AWS instance | `prove-exit.sh A.tgz B.tgz OUT_DIR` |

- The **client proof** is a full zero-knowledge Spartan-WHIR proof of the privacy-pools withdrawal
  relation with a lean statement: the 82 public settle fields become private inputs and the statement
  is a digest of them, so the proof is small to carry and cheap to bind on chain.
- The **exit proof** runs leanVM recursion over two client proofs and ends in a grouped Keccak WHIR
  terminal proof. The deployed terminal verifier checks it, and the pool binds its application root to
  the two spends' statements. `prove-exit.sh` refuses a proof whose guest programs differ from the
  deployed verifier's (`contracts/test/fixtures/terminal/manifest.json`).

## Setup

```bash
bash prover/setup.sh
```

`setup.sh` clones the upstream provers at the commits pinned in `env.sh` (ethereum/spartan-whir,
Client-Side-Proving/leanVM, ethereum/sol-whir-p3) into `PROVER_HOME` (default
`~/.local/share/pq-prover`, override in `.env`), installs this repository's wallet circuit
(`patches/spartan-whir`), and builds two binaries:

- `pool-wallet`: `keygen OUT.json`, `witness NOTE.json CHAIN.json REQUEST.json OUT.json`,
  `check FIELDS.json PROOF_DIR` (the exit's check that a proof is for the claimed fields).
- `leanvm-full-zk-spark-fixture`: the client prover.

It also copies `support/terminal_hashes.py` to the place under `PROVER_HOME` where sol-whir-p3's
terminal fixture codec imports it from; fresh installs failed without it.

Requirements: git, a Rust toolchain, python3 with the leanVM scripts' dependencies, and Foundry.

## On AWS

`infra/aws/prover.sh` runs the exit proof on one `c7i.8xlarge` spot instance; `EXIT_PROVER=aws` in
`.env` makes the console and `scripts/exit-prover.ts` use it. See [infra/aws](../infra/aws/README.md).

## Patches

`patches/spartan-whir` holds the files added to or replacing spartan-whir's: the lean-statement
variant of the withdrawal relation, the wallet module, the `pool-wallet` binary, and the fixture
prover's `--input` mode.
