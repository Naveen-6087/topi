import PoolModel.Frames

/-!
# Theorem 1: policy soundness

If the pool's validation approves a transaction, then
* the transaction has exactly three frames: the pool's own VERIFY (execution scope only), another
  account's VERIFY (payment scope only), and one SENDER frame;
* the only code that runs with the pool as `msg.sender` is that one SENDER frame, a zero-value call to
  the pool's own `settle`: nothing can move the pool's funds except `settle`'s own checks;
* the pool never pays gas: the payment approver is a different account.

This is the property that wallets with EIP-8130-style dispatch were shown to lack (a locked wallet's
owner could still route SENDER calls around its `execute` lock).
-/

namespace PoolModel

/-- The shape every approved pool transaction has. -/
structure ApprovedShape (pool : Nat) (tx : Tx) (f0 f1 f2 : Frame) : Prop where
  frames : tx.frames = [f0, f1, f2]
  sender : tx.sender = pool
  keys : tx.nonceKeys ≠ 0
  seq : tx.nonceSeq = 0
  noBlobs : tx.blobs = 0
  f0Target : f0.target = pool
  f0Mode : f0.mode = .verify
  f0Scope : f0.flags = SCOPE_EXECUTION
  f1NotPool : f1.target ≠ pool
  f1Nonzero : f1.target ≠ 0
  f1Mode : f1.mode = .verify
  f1Scope : f1.flags = SCOPE_PAYMENT
  f2Target : f2.target = pool
  f2Mode : f2.mode = .sender
  f2Flags : f2.flags = 0
  f2Value : f2.value = 0
  f2Settle : f2.selector = SETTLE

/-- Each policy check is "if this fails then error n else continue". -/
theorem ite_err_ok {c : Prop} [Decidable c] {n : Nat} {X : Except Nat Unit} :
    ((if c then Except.error n else X) = Except.ok ()) ↔ (¬c ∧ X = Except.ok ()) := by
  by_cases hc : c <;> simp [hc]

theorem poolPolicy_sound (pool : Nat) (tx : Tx) (h : poolPolicy pool tx 0 = .ok ()) :
    ∃ f0 f1 f2, ApprovedShape pool tx f0 f1 f2 := by
  unfold poolPolicy at h
  match hf : tx.frames with
  | [f0, f1, f2] =>
    rw [hf] at h
    simp only [ite_err_ok, List.length, not_or, Decidable.not_not, ne_eq] at h
    obtain ⟨c1, -, -, c4, c5, c6, c10, c11, c12, -, ⟨c20a, c20b⟩, c21, c22, -, c30, c31, c32, c33, -, c35, -⟩ := h
    exact ⟨f0, f1, f2, {
      frames := hf, sender := c1, keys := c5, seq := c6, noBlobs := c4,
      f0Target := c10, f0Mode := c11, f0Scope := c12,
      f1NotPool := c20b, f1Nonzero := c20a, f1Mode := c21, f1Scope := c22,
      f2Target := c30, f2Mode := c31, f2Flags := c32, f2Value := c33, f2Settle := c35 }⟩
  | [] => rw [hf] at h; simp [ite_err_ok] at h
  | [_] => rw [hf] at h; simp [ite_err_ok] at h
  | [_, _] => rw [hf] at h; simp [ite_err_ok] at h
  | _ :: _ :: _ :: _ :: _ => rw [hf] at h; simp [ite_err_ok] at h

/-- The only call made with the pool as caller is the SENDER frame to the pool's own `settle`. -/
theorem only_settle_runs_as_pool (pool : Nat) (tx : Tx) (h : poolPolicy pool tx 0 = .ok ()) :
    ∃ f2, callsAsSender tx = [f2] ∧ f2.target = pool ∧ f2.selector = SETTLE ∧ f2.value = 0 := by
  obtain ⟨f0, f1, f2, s⟩ := poolPolicy_sound pool tx h
  refine ⟨f2, ?_, s.f2Target, s.f2Settle, s.f2Value⟩
  simp [callsAsSender, s.frames, s.f0Mode, s.f1Mode, s.f2Mode]

/-- The pool never pays: in an approved transaction the payment-scope VERIFY frame belongs to
another account, and the pool's own VERIFY frame may only approve execution. -/
theorem pool_never_pays (pool : Nat) (tx : Tx) (h : poolPolicy pool tx 0 = .ok ()) :
    ∃ f0 f1 f2, tx.frames = [f0, f1, f2] ∧ f0.flags = SCOPE_EXECUTION ∧
      f1.flags = SCOPE_PAYMENT ∧ f1.target ≠ pool := by
  obtain ⟨f0, f1, f2, s⟩ := poolPolicy_sound pool tx h
  exact ⟨f0, f1, f2, s.frames, s.f0Scope, s.f1Scope, s.f1NotPool⟩

/-- The paymaster pays only for the pool's transactions that the exit operator signed. -/
theorem paymaster_pays_only_signed (pm pool op : Nat) (tx : Tx)
    (h : paymasterPolicy pm pool op tx 1 = .ok ()) :
    tx.sender = pool ∧ ∃ s, tx.sigs = [s] ∧ s.signer = op ∧ s.scheme = 1 ∧ s.msgEmpty = true := by
  unfold paymasterPolicy at h
  match hf : tx.frames, hs : tx.sigs with
  | _ :: f1 :: _, [s] =>
    rw [hf, hs] at h
    simp only [ite_err_ok, Decidable.not_not, ne_eq] at h
    obtain ⟨c1, -, -, -, -, c7, c8, c9, -⟩ := h
    refine ⟨c1, s, rfl, c7, c8, ?_⟩
    cases hm : s.msgEmpty <;> simp_all
  | _ :: _ :: _, [] => rw [hf, hs] at h; simp only [ite_err_ok] at h; simp at h
  | _ :: _ :: _, _ :: _ :: _ => rw [hf, hs] at h; simp only [ite_err_ok] at h; simp at h
  | [], _ => rw [hf] at h; simp [ite_err_ok] at h
  | [_], _ => rw [hf] at h; simp [ite_err_ok] at h

end PoolModel
