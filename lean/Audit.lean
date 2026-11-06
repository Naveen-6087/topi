import PoolModel

/-! Axiom audit: every theorem must depend only on propext, Quot.sound and Classical.choice (no sorryAx).
Run: lake env lean Audit.lean -/

open PoolModel

#print axioms deposit_preserves_inv
#print axioms distinct_removeN
#print axioms failed_settle_changes_nothing
#print axioms failed_settle_keeps_notes_spendable
#print axioms firstWitness_spec
#print axioms ite_err_ok
#print axioms keys_removeN_sub
#print axioms mem_keys
#print axioms mem_removeN
#print axioms only_settle_runs_as_pool
#print axioms paymasterPQ_pays_only_verified
#print axioms paymaster_pays_only_signed
#print axioms poolPolicy_not_at_1
#print axioms poolPolicy_sound
#print axioms pool_never_pays
#print axioms pqAccount_needs_witness
#print axioms pq_pool_tx_has_no_ecdsa
#print axioms removeN_gone
#print axioms run_distinct
#print axioms settle_ok_nullifiers
#print axioms settle_ok_split
#print axioms settle_preserves_inv
#print axioms spendOne_ok
#print axioms step_distinct
#print axioms total_append
#print axioms total_removeN
#print axioms validFromAccount_needs_witness
