# Contributing

## Layout

| Path | What | Toolchain |
|---|---|---|
| `crates/` | Rust workspace: packet format, rate limiting, mixnet nodes and client, the `topi` terminal | Rust (stable, 1.88+) |
| `contracts/` | Solidity library: the shielded pool, Yul accounts, interfaces, the terminal verifier | Foundry |
| `lean/` | Lean 4 model of validation and settlement, with proofs | elan (toolchain pinned in `lean/lean-toolchain`) |
| `prover/` | Client and exit proving on top of pinned upstream provers | Rust, python3 (Linux or WSL) |
| `sdk/` | TypeScript SDK | Node 22.18+ |
| `scripts/` | Operational scripts built on the SDK | Node 22.18+ |
| `mempool/` | The ERC-8421 alt mempool: rules, admission policy, sidecar patch | Node 22.18+, Bun (sidecar, Linux or WSL) |
| `infra/aws/` | Terraform and scripts for a four-region mixnet and the exit prover instance | Terraform, AWS CLI |
| `docs/` | Documentation site | Mintlify |
| `website/` | Landing page | static |
| `deployments/` | Addresses of deployed contracts, per chain | |

Local settings go in `.env` (copy `.env.example`); secrets in `.secrets/`; runtime data in `data/`;
logs in `logs/`. None of these are committed.

## Checks

Run what CI runs before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --release -- -D warnings
cargo test --workspace --release
(cd contracts && forge test && bash script/build-yul.sh --check)
(cd lean && lake build && lake env lean Audit.lean)
npm ci && npm run typecheck && npm test
(cd docs && npx mint broken-links)
```

## Conventions

- Names describe what a thing does. The project name appears only in package and binary names.
- Comments explain why, not what. No commented-out code.
- A change to the Yul accounts or `ShieldedPool` changes deployed behaviour: keep the Lean model
  (`lean/PoolModel`) in step, and rerun the differential tests against the chain.
- `mempool/RULES.md` is the mempool's definition: its IPFS hash is the mempool id. Changing it
  changes the id (`node mempool/src/mempool-id.ts`); update the docs that quote it.
- The console embeds runtime files (prover scripts, the sidecar's setup and patch, SDK sources, AWS
  scripts, deployment records, bytecode). A change to one of them reaches an installed `topi` only
  after a rebuild.
- A change to a wire format (`crates/sphinx`, `crates/mixnet/src/message.rs`, the spend envelope) needs
  every node and client upgraded together; bump the version label in the format.

## License

By contributing you agree that your contributions are licensed under the MIT license or the Apache
License 2.0, at the user's option, as the rest of the project.
