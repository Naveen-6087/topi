/-!
# `ShieldedPool.settle` and `deposit`, as a state machine

`spendOne` and `settle` follow `ShieldedPool.sol` check by check (terminal proof, statement binding, then
per spend: pool and scope, known roots, unspent nullifier, payout, then the paired change insert). The
proof system itself is abstracted: `proofOk` is the terminal verifier's answer and `bindOk` is the
application-root comparison. Merkle roots are abstracted by `rootOf : List Nat → Nat`.

`live` is ghost state (not in the contract): the (nullifier, value) of every note that exists and is not
spent. It is what "no inflation" is about. A transaction's effect is `step`: a reverted `settle` leaves the
state exactly as it was (EVM revert semantics of the SENDER frame).
-/

namespace PoolModel

/-- One spend's fields as `settle` uses them (ghost fields are marked). -/
structure Spend where
  /-- pool and scope fields equal this pool's identity -/
  poolOk : Bool
  poolRoot : Nat
  assocRoot : Nat
  nullifier : Nat
  withdrawn : Nat
  fee : Nat
  changeLeaf : Nat
  /-- ghost: value of the change note -/
  change : Nat
  /-- ghost: the nullifier the change note will have -/
  changeNullifier : Nat
  deriving Repr

structure State where
  spent : List Nat
  knownPool : List Nat
  knownAssoc : List Nat
  balance : Nat
  leaves : List Nat
  /-- ghost: live notes as (nullifier, value) -/
  live : List (Nat × Nat)

inductive Err where
  | proof
  | binding
  | wrongPool
  | unknownRoot
  | alreadySpent
  | payout
  deriving DecidableEq, Repr

/-- `_spend` in ShieldedPool: checks, nullifier, payout of withdrawn + fee. -/
def spendOne (s : State) (x : Spend) : Except Err State :=
  if x.poolOk = false then .error .wrongPool else
  if x.poolRoot ∉ s.knownPool ∨ x.assocRoot ∉ s.knownAssoc then .error .unknownRoot else
  if x.nullifier ∈ s.spent then .error .alreadySpent else
  if s.balance < x.withdrawn + x.fee then .error .payout else
  .ok { s with spent := x.nullifier :: s.spent, balance := s.balance - (x.withdrawn + x.fee) }

/-- Remove the first live note with nullifier `n`. -/
def removeN (n : Nat) : List (Nat × Nat) → List (Nat × Nat)
  | [] => []
  | p :: ps => if p.1 = n then ps else p :: removeN n ps

/-- `settle(proof, A, B)`. -/
def settle (rootOf : List Nat → Nat) (s : State) (proofOk bindOk : Bool) (a b : Spend) : Except Err State :=
  if proofOk = false then .error .proof else
  if bindOk = false then .error .binding else
  match spendOne s a with
  | .error e => .error e
  | .ok s1 =>
    match spendOne s1 b with
    | .error e => .error e
    | .ok s2 =>
      let leaves := s2.leaves ++ [a.changeLeaf, b.changeLeaf]
      .ok { s2 with
        leaves := leaves
        knownPool := rootOf leaves :: s2.knownPool
        live := removeN b.nullifier (removeN a.nullifier s2.live) ++
          [(a.changeNullifier, a.change), (b.changeNullifier, b.change)] }

/-- A settle transaction's effect: revert means no change at all. -/
def step (rootOf : List Nat → Nat) (s : State) (proofOk bindOk : Bool) (a b : Spend) : State :=
  match settle rootOf s proofOk bindOk a b with
  | .ok s' => s'
  | .error _ => s

/-- `deposit`: value in, note leaf + zero leaf, label leaf; ghost: a new live note. -/
def deposit (rootOf : List Nat → Nat) (assocRootOf : Nat → Nat) (s : State)
    (value noteLeaf nullifier : Nat) : State :=
  let leaves := s.leaves ++ [noteLeaf, 0]
  { s with
    balance := s.balance + value
    leaves := leaves
    knownPool := rootOf leaves :: s.knownPool
    knownAssoc := assocRootOf leaves.length :: s.knownAssoc
    live := (nullifier, value) :: s.live }

end PoolModel
