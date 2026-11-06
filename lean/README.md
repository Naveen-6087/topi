# pool-model

A Lean 4 model of the shielded pool's transaction validation and settlement, with machine-checked
theorems about it. `lake build` checks every proof; `lake env lean Audit.lean` prints the axioms each
theorem depends on: only `propext` and `Quot.sound`, never `sorryAx`.

## Models

- `PoolModel/Frames.lean`: EIP-8141 frame transactions as the pool's validation code sees them, and
  the validation policies transcribed check by check, with the same failure codes, from
  `contracts/yul/PoolAccount.yul` (`poolPolicy`), `contracts/yul/ClassicPaymaster.yul`
  (`paymasterPolicy`), `contracts/yul/PQPaymaster.yul` (`paymasterPQPolicy`) and the leanSPHINCS account
  (`pqAccountPolicy`). `valid` adds the protocol rule that the payment approver's state budget covers
  97,920 per fresh nonce key.
- `PoolModel/Settle.lean`: `ShieldedPool.settle` and `deposit` as a state machine (proof and binding
  results abstracted, Merkle roots abstracted), with ghost state `live` (unspent notes and their
  values) and `step` (a reverted settle leaves the state unchanged).

## Theorems

| # | Theorem | Statement |
|---|---|---|
| 1 | `poolPolicy_sound` | an approved pool transaction has exactly three frames: the pool's own VERIFY (execution scope), another account's VERIFY (payment scope), one zero-value SENDER call to the pool's `settle` |
| 1 | `only_settle_runs_as_pool` | the only frame that runs with the pool as caller is that `settle` call |
| 1 | `pool_never_pays` | the payment approver is never the pool |
| 1 | `paymaster_pays_only_signed` | the classic paymaster approves only the pool's transactions with exactly one operator signature over the transaction hash |
| 2 | `settle_ok_nullifiers` | a successful settle consumes two distinct, previously unspent nullifiers and nothing else |
| 2 | `run_distinct` | over any history of settles, no nullifier is ever spent twice |
| 3 | `failed_settle_changes_nothing`, `failed_settle_keeps_notes_spendable` | a reverted settle changes nothing; its notes stay spendable |
| 4 | `settle_preserves_inv`, `deposit_preserves_inv` | balance equals the value of live notes and live nullifiers are distinct and unspent: the pool never pays out value that was not deposited |
| 5 | `paymasterPQ_pays_only_verified` | the PQ paymaster pays only for the pool's transactions carrying exactly one ARBITRARY leanSPHINCS witness over the signature hash that the verifier accepts under the operator's key |
| 5 | `pqAccount_needs_witness`, `validFromAccount_needs_witness` | a leanSPHINCS account approves execution or payment only with a verifying witness over the transaction hash |
| 5 | `pq_pool_tx_has_no_ecdsa` | with the PQ paymaster, every signature in a valid pool transaction is a hash-based ARBITRARY witness |

Assumed, not proven: soundness of leanSPHINCS (`Sig.verifies` stands for the on-chain verifier
accepting) and of the zero-knowledge proof system (`Sound`: an accepted spend opens a live note whose
value is withdrawn plus fee plus change), fresh change-note nullifiers (`Fresh`), and that the model
matches the deployed bytecode, which the differential tests check rather than prove.

## Differential tests against chain 8141

The executable `pool-policy` answers validity queries as line-delimited JSON (`Main.lean`). The
differential tests in `scripts/` mutate valid transactions (sender, frame count, targets, modes, flags,
values, selector, data length, nonce keys, state budget, signatures), ask the chain's
`ethrex_simulateFrameTransaction` and `pool-policy` for a verdict, and compare.

The first run disagreed on 2 of 84 transactions, both with two nonce keys: the model was missing the rule
that fresh nonce keys are billed to the payment approver's state budget. With the rule added, 528
transactions over three seeds agree. The post-quantum paths agree too: the pool with the PQ paymaster on
90 of 90 (corrupted witness, another key, ECDSA instead of a witness, both), the classic pool on 56 of 56,
and the leanSPHINCS account on 42 of 42.

## Build

```bash
lake build                  # library and the pool-policy executable
lake env lean Audit.lean    # axioms per theorem
```
