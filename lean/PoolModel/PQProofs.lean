import PoolModel.Frames
import PoolModel.PolicyProofs

/-!
# Theorem group 5: post-quantum authorization

* `paymasterPQ_pays_only_verified`: the PQ paymaster approves payment only for the pool's transactions
  carrying exactly one signature entry: an ARBITRARY leanSPHINCS witness over compute_sig_hash(tx) of
  the right length that the verifier accepts under the exit's key.
* `pqAccount_needs_witness`, `validFromAccount_needs_witness`: a leanSPHINCS account approves execution
  or payment only with a verifying witness over the transaction hash: no key, no spend.
* `pq_pool_tx_has_no_ecdsa`: in a valid pool transaction under a PQ-paymaster deployment, every signature
  is an ARBITRARY (hash-based) witness: no secp256k1 signature authorizes anything on the pool path.

The verifier's soundness (`Sig.verifies` means a valid leanSPHINCS signature) is the assumption; the
theorems are about who can get approvals given that.
-/

namespace PoolModel

theorem paymasterPQ_pays_only_verified (pm pool : Nat) (tx : Tx)
    (h : paymasterPQPolicy pm pool tx 1 = .ok ()) :
    tx.sender = pool ∧ ∃ s, tx.sigs = [s] ∧ s.scheme = ARBITRARY ∧ s.msgEmpty = true ∧
      s.len = LEANSPHINCS_SIG_BYTES ∧ s.verifies = true := by
  unfold paymasterPQPolicy at h
  match hf : tx.frames, hs : tx.sigs with
  | _ :: f1 :: _, [s] =>
    rw [hf, hs] at h
    simp only [ite_err_ok, Decidable.not_not, ne_eq] at h
    obtain ⟨c1, -, -, -, -, c7, c8, c9, c11, -⟩ := h
    refine ⟨c1, s, rfl, c7, ?_, c9, ?_⟩
    · cases hm : s.msgEmpty <;> simp_all
    · cases hv : s.verifies <;> simp_all
  | _ :: _ :: _, [] => rw [hf, hs] at h; simp only [ite_err_ok] at h; simp at h
  | _ :: _ :: _, _ :: _ :: _ => rw [hf, hs] at h; simp only [ite_err_ok] at h; simp at h
  | [], _ => rw [hf] at h; simp [ite_err_ok] at h
  | [_], _ => rw [hf] at h; simp [ite_err_ok] at h

theorem firstWitness_spec : ∀ {l : List Sig} {s : Sig}, firstWitness l = some s →
    s ∈ l ∧ s.scheme = ARBITRARY ∧ s.msgEmpty = true
  | [], _, h => by simp [firstWitness] at h
  | x :: rest, s, h => by
    unfold firstWitness at h
    by_cases c : x.scheme = ARBITRARY ∧ x.msgEmpty = true
    · simp only [c, and_self, if_true] at h
      cases h
      exact ⟨List.mem_cons_self .., c.1, c.2⟩
    · simp only [c, if_false] at h
      obtain ⟨m, r⟩ := firstWitness_spec h
      exact ⟨List.mem_cons_of_mem _ m, r⟩

theorem pqAccount_needs_witness (tx : Tx) (i scope : Nat) (h : pqAccountPolicy tx i = .ok scope) :
    scope ≠ 0 ∧ ∃ s, s ∈ tx.sigs ∧ s.scheme = ARBITRARY ∧ s.msgEmpty = true ∧ s.verifies = true := by
  unfold pqAccountPolicy at h
  match hw : firstWitness tx.sigs, h with
  | none, h => simp at h
  | some s, h =>
    simp only [] at h
    cases hv : s.verifies
    · simp [hv] at h
    · simp only [hv, Bool.not_true, Bool.false_eq_true, if_false] at h
      obtain ⟨m, sc, me⟩ := firstWitness_spec hw
      match hf : tx.frames[i]?, h with
      | none, h => simp at h
      | some f, h =>
        simp only [] at h
        by_cases c : f.flags % 4 = 0
        · simp [c] at h
        · simp only [c, if_false, Except.ok.injEq] at h
          subst h
          exact ⟨c, s, m, sc, me, hv⟩

theorem validFromAccount_needs_witness (acc : Nat) (tx : Tx) (h : validFromAccount acc tx = true) :
    tx.sender = acc ∧ ∃ s, s ∈ tx.sigs ∧ s.scheme = ARBITRARY ∧ s.msgEmpty = true ∧ s.verifies = true := by
  unfold validFromAccount at h
  match hf : tx.frames, h with
  | [], h => simp at h
  | f0 :: _, h =>
    simp only [Bool.and_eq_true, decide_eq_true_eq] at h
    obtain ⟨⟨⟨⟨hs, -⟩, -⟩, hp⟩, -⟩ := h
    refine ⟨hs, ?_⟩
    match hq : pqAccountPolicy tx 0, hp with
    | .ok sc, _ => exact (pqAccount_needs_witness tx 0 sc hq).2
    | .error _, hp => simp at hp

/-- The pool's own validation never approves as frame 1. -/
theorem poolPolicy_not_at_1 (pool : Nat) (tx : Tx) : isOk (poolPolicy pool tx 1) = false := by
  cases h : poolPolicy pool tx 1 with
  | error _ => rfl
  | ok u =>
    exfalso
    cases u
    unfold poolPolicy at h
    simp only [ite_err_ok] at h
    simp at h

/-- Under a PQ-paymaster deployment, a valid pool transaction carries only hash-based witnesses. -/
theorem pq_pool_tx_has_no_ecdsa (d : Deployment) (tx : Tx) (hpq : d.pqPaymaster = true)
    (hpp : d.paymaster ≠ d.pool) (h : valid d tx = true) :
    ∀ s ∈ tx.sigs, s.scheme = ARBITRARY := by
  unfold valid at h
  match hf : tx.frames, h with
  | [], h => simp at h
  | [_], h => simp at h
  | f0 :: f1 :: rest, h =>
    simp only [Bool.and_eq_true, decide_eq_true_eq] at h
    obtain ⟨⟨-, h1⟩, -⟩ := h
    unfold verifyAt at h1
    simp only [hpq, if_true, decide_eq_true_eq, Bool.decide_and, Bool.decide_or] at h1
    have np := poolPolicy_not_at_1 d.pool tx
    -- frame 1 must be the paymaster's, under the PQ policy
    have pm : isOk (paymasterPQPolicy d.paymaster d.pool tx 1) = true := by
      revert h1
      simp [np]
    cases hq : paymasterPQPolicy d.paymaster d.pool tx 1 with
    | error _ => simp [hq, isOk] at pm
    | ok u =>
      cases u
      obtain ⟨-, s, hs, sc, -⟩ := paymasterPQ_pays_only_verified _ _ _ hq
      intro t ht
      rw [hs] at ht
      simp at ht
      subst ht
      exact sc

end PoolModel
