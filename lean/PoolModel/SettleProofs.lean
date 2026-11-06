import PoolModel.Settle

/-!
# Theorems 2–4: no double spend, no burn, no inflation
-/

namespace PoolModel

def Distinct : List Nat → Prop
  | [] => True
  | x :: xs => x ∉ xs ∧ Distinct xs

def total : List (Nat × Nat) → Nat
  | [] => 0
  | p :: ps => p.2 + total ps

def keys (l : List (Nat × Nat)) : List Nat := l.map Prod.fst

/-! ## Theorem 2: no double spend -/

theorem spendOne_ok {s s' : State} {x : Spend} (h : spendOne s x = .ok s') :
    x.nullifier ∉ s.spent ∧ s'.spent = x.nullifier :: s.spent ∧
      s'.balance = s.balance - (x.withdrawn + x.fee) ∧ x.withdrawn + x.fee ≤ s.balance ∧
      s'.live = s.live ∧ s'.leaves = s.leaves ∧ s'.knownPool = s.knownPool := by
  unfold spendOne at h
  by_cases c1 : x.poolOk = false <;> simp only [c1, ↓reduceIte] at h
  · cases h
  by_cases c2 : x.poolRoot ∉ s.knownPool ∨ x.assocRoot ∉ s.knownAssoc <;>
    simp only [c2, ↓reduceIte] at h
  · cases h
  by_cases c3 : x.nullifier ∈ s.spent <;> simp only [c3, ↓reduceIte] at h
  · cases h
  by_cases c4 : s.balance < x.withdrawn + x.fee <;> simp only [c4, ↓reduceIte] at h
  · cases h
  cases h
  exact ⟨c3, rfl, rfl, by omega, rfl, rfl, rfl⟩

/-- The shape of a successful settle: both spends passed, then the change notes were added. -/
theorem settle_ok_split {rootOf : List Nat → Nat} {s s' : State} {p q : Bool} {a b : Spend}
    (h : settle rootOf s p q a b = .ok s') :
    ∃ s1 s2, spendOne s a = .ok s1 ∧ spendOne s1 b = .ok s2 ∧
      s' = { s2 with
        leaves := s2.leaves ++ [a.changeLeaf, b.changeLeaf]
        knownPool := rootOf (s2.leaves ++ [a.changeLeaf, b.changeLeaf]) :: s2.knownPool
        live := removeN b.nullifier (removeN a.nullifier s2.live) ++
          [(a.changeNullifier, a.change), (b.changeNullifier, b.change)] } := by
  unfold settle at h
  cases p <;> cases q <;> simp only [Bool.true_eq_false, ↓reduceIte] at h <;> try cases h
  split at h
  · cases h
  · rename_i s1 h1
    split at h
    · cases h
    · rename_i s2 h2
      exact ⟨s1, s2, h1, h2, (Except.ok.inj h).symm⟩

/-- A successful settle consumes two distinct, previously unspent nullifiers, and nothing else. -/
theorem settle_ok_nullifiers {rootOf : List Nat → Nat} {s s' : State} {p q : Bool} {a b : Spend}
    (h : settle rootOf s p q a b = .ok s') :
    a.nullifier ∉ s.spent ∧ b.nullifier ∉ s.spent ∧ a.nullifier ≠ b.nullifier ∧
      s'.spent = b.nullifier :: a.nullifier :: s.spent := by
  obtain ⟨s1, s2, h1, h2, rfl⟩ := settle_ok_split h
  obtain ⟨na, sa, -⟩ := spendOne_ok h1
  obtain ⟨nb, sb, -⟩ := spendOne_ok h2
  rw [sa] at nb
  refine ⟨na, fun hb => nb (List.mem_cons_of_mem _ hb), fun e => nb (by simp [e]), ?_⟩
  simp [sb, sa]

/-- Spent nullifiers never repeat, across any history of settle transactions. -/
theorem step_distinct {rootOf : List Nat → Nat} {s : State} {p q : Bool} {a b : Spend}
    (h : Distinct s.spent) : Distinct (step rootOf s p q a b).spent := by
  unfold step
  match hs : settle rootOf s p q a b with
  | .error _ => exact h
  | .ok s' =>
    obtain ⟨na, nb, ne, sp⟩ := settle_ok_nullifiers hs
    simp only [sp, Distinct]
    exact ⟨by simp [Ne.symm ne, nb], na, h⟩

/-- A history of settle transactions (each with its verifier answers and two spends). -/
def run (rootOf : List Nat → Nat) : State → List (Bool × Bool × Spend × Spend) → State
  | s, [] => s
  | s, (p, q, a, b) :: rest => run rootOf (step rootOf s p q a b) rest

theorem run_distinct {rootOf : List Nat → Nat} :
    ∀ (s : State) (txs : List (Bool × Bool × Spend × Spend)), Distinct s.spent → Distinct (run rootOf s txs).spent
  | _, [], h => h
  | s, (p, q, a, b) :: rest, h => run_distinct _ rest (step_distinct (a := a) (b := b) (p := p) (q := q) h)

/-! ## Theorem 3: no burn -/

/-- A reverted settle changes nothing: no nullifier is consumed, no value moves, no note disappears. -/
theorem failed_settle_changes_nothing {rootOf : List Nat → Nat} {s : State} {p q : Bool} {a b : Spend}
    {e : Err} (h : settle rootOf s p q a b = .error e) : step rootOf s p q a b = s := by
  simp [step, h]

theorem failed_settle_keeps_notes_spendable {rootOf : List Nat → Nat} {s : State} {p q : Bool}
    {a b : Spend} {e : Err} (h : settle rootOf s p q a b = .error e) (n : Nat) (hn : n ∉ s.spent) :
    n ∉ (step rootOf s p q a b).spent := by
  rw [failed_settle_changes_nothing h]
  exact hn

/-! ## Theorem 4: no inflation -/

theorem mem_keys {n : Nat} {l : List (Nat × Nat)} : n ∈ keys l ↔ ∃ v, (n, v) ∈ l := by
  simp [keys, List.mem_map]

theorem total_removeN {n v : Nat} :
    ∀ {l : List (Nat × Nat)}, Distinct (keys l) → (n, v) ∈ l → total l = total (removeN n l) + v
  | [], _, h => by simp at h
  | p :: ps, hd, h => by
    simp only [keys, List.map, Distinct] at hd
    obtain ⟨hp, hd⟩ := hd
    rcases List.mem_cons.mp h with h | h
    · subst h
      simp [removeN, total] <;> omega
    · have hn : p.1 ≠ n := by
        intro e
        exact hp (e ▸ mem_keys.mpr ⟨v, h⟩)
      simp only [removeN, hn, if_false, total]
      rw [total_removeN hd h]
      omega

theorem keys_removeN_sub {n m : Nat} : ∀ {l : List (Nat × Nat)}, m ∈ keys (removeN n l) → m ∈ keys l
  | [], h => by simp [removeN, keys] at h
  | p :: ps, h => by
    unfold removeN at h
    by_cases hp : p.1 = n <;> simp only [hp, ↓reduceIte] at h
    · exact List.mem_cons_of_mem _ h
    · simp only [keys, List.map, List.mem_cons] at h ⊢
      rcases h with h | h
      · exact Or.inl h
      · exact Or.inr (keys_removeN_sub h)

theorem distinct_removeN {n : Nat} : ∀ {l : List (Nat × Nat)}, Distinct (keys l) → Distinct (keys (removeN n l))
  | [], _ => by simp [removeN, keys, Distinct]
  | p :: ps, hd => by
    simp only [keys, List.map, Distinct] at hd
    unfold removeN
    by_cases hp : p.1 = n <;> simp only [hp, ↓reduceIte]
    · exact hd.2
    · simp only [keys, List.map, Distinct]
      exact ⟨fun hm => hd.1 (keys_removeN_sub hm), distinct_removeN hd.2⟩

theorem removeN_gone {n : Nat} : ∀ {l : List (Nat × Nat)}, Distinct (keys l) → n ∉ keys (removeN n l)
  | [], _ => by simp [removeN, keys]
  | p :: ps, hd => by
    simp only [keys, List.map, Distinct] at hd
    unfold removeN
    by_cases hp : p.1 = n <;> simp only [hp, ↓reduceIte]
    · rw [← hp]; exact hd.1
    · simp only [keys, List.map, List.mem_cons, not_or]
      exact ⟨fun e => hp e.symm, removeN_gone hd.2⟩

theorem mem_removeN {n : Nat} {q : Nat × Nat} : ∀ {l : List (Nat × Nat)}, q ∈ l → q.1 ≠ n → q ∈ removeN n l
  | [], h, _ => by simp at h
  | p :: ps, h, hq => by
    unfold removeN
    rcases List.mem_cons.mp h with h | h
    · subst h; simp [hq]
    · by_cases hp : p.1 = n <;> simp only [hp, ↓reduceIte]
      · exact h
      · exact List.mem_cons_of_mem _ (mem_removeN h hq)

theorem total_append (l m : List (Nat × Nat)) : total (l ++ m) = total l + total m := by
  induction l with
  | nil => simp [total]
  | cons p ps ih => simp [total, ih]; omega

/-- The pool's accounting invariant: the balance is exactly the value of the live notes, live notes
have distinct nullifiers, and none of them is spent. -/
structure Inv (s : State) : Prop where
  conserve : s.balance = total s.live
  distinctKeys : Distinct (keys s.live)
  liveUnspent : ∀ n, n ∈ keys s.live → n ∉ s.spent

/-- What the zero-knowledge proof guarantees about a spend it accepts (circuit soundness, assumed): the
spender owns a live note with this nullifier whose value is withdrawn + fee + change. -/
def Sound (s : State) (x : Spend) : Prop := (x.nullifier, x.withdrawn + x.fee + x.change) ∈ s.live

/-- The change notes get fresh nullifiers (they come from fresh secrets). -/
structure Fresh (s : State) (a b : Spend) : Prop where
  aNew : a.changeNullifier ∉ keys s.live ∧ a.changeNullifier ∉ s.spent
  bNew : b.changeNullifier ∉ keys s.live ∧ b.changeNullifier ∉ s.spent
  apart : a.changeNullifier ≠ b.changeNullifier
  notSpent : a.changeNullifier ≠ a.nullifier ∧ a.changeNullifier ≠ b.nullifier ∧
    b.changeNullifier ≠ a.nullifier ∧ b.changeNullifier ≠ b.nullifier

/-- No inflation: a successful settle of sound spends preserves the invariant, so the pool's balance is
always exactly the value of its live notes; it can never pay out value that was not deposited. -/
theorem settle_preserves_inv {rootOf : List Nat → Nat} {s s' : State} {p q : Bool} {a b : Spend}
    (inv : Inv s) (sa : Sound s a) (sb : Sound s b) (fr : Fresh s a b)
    (h : settle rootOf s p q a b = .ok s') : Inv s' := by
  obtain ⟨na, nb, ne, sp⟩ := settle_ok_nullifiers h
  obtain ⟨s1, s2, h1, h2, rfl⟩ := settle_ok_split h
  obtain ⟨-, -, b1, le1, l1, -, -⟩ := spendOne_ok h1
  obtain ⟨-, -, b2, le2, l2, -, -⟩ := spendOne_ok h2
  -- the two removed notes
  have hb1 : (b.nullifier, b.withdrawn + b.fee + b.change) ∈ removeN a.nullifier s.live :=
    mem_removeN sb (fun e => ne e.symm)
  have t1 := total_removeN inv.distinctKeys sa
  have t2 := total_removeN (distinct_removeN inv.distinctKeys) hb1
  have hc := inv.conserve
  refine ⟨?_, ?_, ?_⟩
  · -- balance
    simp only [l2, l1, b2, b1, total_append, total]
    rw [b1] at le2
    omega
  · -- distinct keys
    simp only [l2, l1, keys, List.map_append, List.map]
    have dk := distinct_removeN (n := b.nullifier) (distinct_removeN (n := a.nullifier) inv.distinctKeys)
    have sub : ∀ m, m ∈ (removeN b.nullifier (removeN a.nullifier s.live)).map Prod.fst → m ∈ keys s.live :=
      fun m hm => keys_removeN_sub (keys_removeN_sub hm)
    clear t1 t2 hc
    generalize hr : (removeN b.nullifier (removeN a.nullifier s.live)).map Prod.fst = r at dk sub
    simp only [keys] at dk
    rw [hr] at dk
    have ha : a.changeNullifier ∉ r := fun hm => fr.aNew.1 (sub _ hm)
    have hb : b.changeNullifier ∉ r := fun hm => fr.bNew.1 (sub _ hm)
    clear hr sub
    induction r with
    | nil => simp [Distinct, fr.apart]
    | cons x xs ih =>
      simp only [Distinct, List.mem_cons, not_or] at dk ha hb ⊢
      simp only [List.cons_append, Distinct, List.mem_append, List.mem_cons, not_or]
      exact ⟨⟨dk.1, fun e => ha.1 e.symm, fun e => hb.1 e.symm, by simp⟩, ih dk.2 ha.2 hb.2⟩
  · -- live notes are unspent
    intro m hm
    rw [sp]
    simp only [l2, l1, keys, List.map_append, List.map, List.mem_append, List.mem_cons] at hm
    simp only [List.mem_cons, not_or]
    rcases hm with hm | hm | hm | hm
    · have gb : m ≠ b.nullifier := fun e => removeN_gone (distinct_removeN inv.distinctKeys) (e ▸ hm)
      have hm' : m ∈ keys (removeN a.nullifier s.live) := keys_removeN_sub (n := b.nullifier) hm
      have ga : m ≠ a.nullifier := fun e => removeN_gone inv.distinctKeys (e ▸ hm')
      exact ⟨gb, ga, inv.liveUnspent m (keys_removeN_sub hm')⟩
    · subst hm; exact ⟨fr.notSpent.2.1, fr.notSpent.1, fr.aNew.2⟩
    · subst hm; exact ⟨fr.notSpent.2.2.2, fr.notSpent.2.2.1, fr.bNew.2⟩
    · simp at hm

/-- Deposits preserve the invariant (a deposit's note nullifier is fresh). -/
theorem deposit_preserves_inv {rootOf : List Nat → Nat} {assocRootOf : Nat → Nat} {s : State}
    {value leaf n : Nat} (inv : Inv s) (fresh : n ∉ keys s.live ∧ n ∉ s.spent) :
    Inv (deposit rootOf assocRootOf s value leaf n) := by
  refine ⟨?_, ?_, ?_⟩
  · simp [deposit, total, inv.conserve]; omega
  · simp only [deposit, keys, List.map, Distinct]
    exact ⟨fresh.1, inv.distinctKeys⟩
  · intro m hm
    simp only [deposit, keys, List.map, List.mem_cons] at hm
    rcases hm with hm | hm
    · subst hm; exact fresh.2
    · exact inv.liveUnspent m hm

end PoolModel
