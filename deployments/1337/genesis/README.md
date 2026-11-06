# topi shielded pool for the ERC-8421 devnet genesis

The full topi pool, for the alt mempool devnet (chain 1337) as genesis accounts, the way the devnet
already carries replayed mainnet contracts. It cannot be deployed there by transaction: its terminal
proof verifier (59 KB) and pool logic (50 KB) need about 90M and 77M gas of code deposit, more than the
60M block gas limit (code deposit costs about 1,530 state gas per byte on the devnet).

Add every entry of `alloc.json` to the genesis `alloc`. Nothing else is needed: the pool account's
storage already holds its initialized empty trees, and the paymaster starts with 1 ETH (any balance
works; the operator tops it up by plain transfer).

| Contract | Address | Code bytes | Storage slots |
|---|---|---|---|
| leanSPHINCS verifier | `0x3675eb04797d3a396b744118a235a5761e734e68` | 3,880 | 0 |
| terminal proof verifier (LeanVmGroupedTerminal_KeccakPow28T6Lean) | `0xBC25bd6BEd2614435f3Fe6c466981aECA7035AF0` | 59,013 | 0 |
| ShieldedPool logic | `0x9E88B04d860a586a6566B8e85cD701179f43097E` | 50,100 | 0 |
| pool account (PoolAccount) | `0x879b53730ae5896187e95aa65ffda44059e9b00b` | 580 | 5 |
| PQ paymaster (PQPaymaster) | `0x9c5a4d17220899435e89aff6db3e66a1a4927f46` | 689 | 0 |

- The two verifiers are byte for byte the contracts deployed on chain 8141, where they verify real
  settles (for example blocks 714811, 717928 and 719307).
- `ShieldedPool` is built from `contracts/src/ShieldedPool.sol` with constructor arguments
  `(terminal, operator 0x28Ca5A65213834f97e17707467337fBcf856E08d, testbed false, poolId, scope)`; pool and scope are
  `pq-pool:1337:genesis:pool|scope` (see `scripts/deploy.sh`).
- `PoolAccount` and `PQPaymaster` are `contracts/yul/build/*.bin` with their initcode arguments (the
  logic address; pool, operator, verifier, exit leanSPHINCS key). The PoolAccount's storage is what
  its initcode wrote by calling `initialize()`.
- The addresses come from a throwaway deployer key, so they collide with nothing on the devnet.

`pool.json` is the deployment record the SDK, scripts and console read (`POOL_DEPLOYMENT`).

To rebuild: run `anvil --chain-id 1337 --disable-code-size-limit --gas-limit 1000000000`, deploy the
five contracts in the order above from a fresh key (the verifiers as exact copies of their chain 8141
runtime code), then take each account's code and non-zero storage from `anvil_dumpState`.
