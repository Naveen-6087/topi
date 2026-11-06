# Soundness

What a theorem in this package guarantees, what it assumes, and where the gap between the model and the
deployed system lies. Every row is a reason a theorem could hold while the system misbehaves.

## Trusted base

| ID | Item | Status |
|---|---|---|
| T1 | Lean 4 kernel (toolchain pinned in `lean-toolchain`) | trusted |
| T2 | Axioms `propext` and `Quot.sound` | used; `lake env lean Audit.lean` lists them per theorem |
| T3 | `Classical.choice`, `sorryAx`, user `axiom` declarations | not used; CI fails if `Audit.lean` shows `sorryAx` or `Classical.choice` |
| T4 | `native_decide`, `@[implemented_by]`, `@[extern]`, `opaque` | not used anywhere in the package |

## Modelling assumptions

| ID | Assumption | Where | Discharged by |
|---|---|---|---|
| A1 | The proof system is sound: a spend the terminal verifier accepts opens a live note worth withdrawn + fee + change | `Sound` (`PoolModel/SettleProofs.lean`), hypothesis of `settle_preserves_inv` | not discharged; rests on Spartan-WHIR, leanVM and WHIR soundness |
| A2 | The terminal verifier's answer and the statement binding are opaque booleans | `settle` takes `proofOk`, `bindOk` | the binding computation is tested in `contracts/test/StatementBinding.t.sol` against real lean statements |
| A3 | Change notes receive fresh nullifiers | `Fresh` | not discharged; follows from fresh wallet secrets and collision resistance of the note hash |
| A4 | leanSPHINCS is unforgeable: `Sig.verifies = true` means the key holder signed | `Sig.verifies` (`PoolModel/Frames.lean`) | not discharged; the verifier contract is upstream code |
| A5 | Merkle roots are a function of the leaf list | `rootOf : List Nat → Nat` | not discharged; hash collision resistance |
| A6 | A reverted SENDER frame leaves state unchanged | `step` | EVM revert semantics; observed on chain (a settle that ran out of gas changed nothing) |

## Model-to-code gap

| ID | Gap | Mitigation |
|---|---|---|
| G1 | `poolPolicy`, `paymasterPolicy`, `paymasterPQPolicy`, `pqAccountPolicy` are hand transcriptions of the Yul accounts | differential tests against the chain's `ethrex_simulateFrameTransaction` (`scripts/differential/`): 90/90, 56/56, 42/42 and 528 policy cases agree |
| G2 | Constants (modes, scopes, frame indices, signature length, selectors, failure codes) could drift between Lean and Yul | `contracts/test/LeanParity.t.sol` reads `PoolModel/Frames.lean` and fails on any mismatch |
| G3 | `settle` and `deposit` are hand transcriptions of `ShieldedPool.sol` | Foundry tests of the same behaviours (`contracts/test/ShieldedPool.t.sol`); not differentially tested |
| G4 | The model counts state gas per nonce key (97,920) but no execution gas | out of scope; gas limits are measured on chain |
| G5 | `Main.lean` parses JSON at an unverified boundary | used only by the differential tests; a parsing bug shows up as a disagreement |

## Out of scope

EVM semantics beyond what the policies read, reentrancy, the WHIR verifier's correctness, the mixnet,
the rate limiter, and network-level privacy. None of the theorems speak about anonymity.

The ERC-8421 alt mempool (`mempool/`) and `DevnetSettle` (`contracts/src/devnet/`, the stand-in pool
logic on the ERC-8421 devnet, chain 1337) are also outside the model. Neither changes the modelled
pool on chain 8141, and the model is unchanged by them.
