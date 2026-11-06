/-!
# EIP-8141 frame transactions, as the post-quantum pool's validation code sees them

A model of exactly the transaction parameters that `PoolAccount.yul` and `ClassicPaymaster.yul` read
(TXPARAM sender, frame count, current frame index, blob count, nonce-key count, nonce sequence; per frame
FRAMEPARAM target, mode, flags, value, data length and the first 4 data bytes; SIGPARAM signer, scheme,
msg). Each policy below is a transcription of one Yul program, check by check, with the same failure
codes, so a Lean/chain disagreement points at a specific line.

Execution semantics used by the theorems (EIP-8141): after the validation prefix approves, SENDER frames
call their target with `caller = tx.sender`; DEFAULT frames call with the protocol entry point as caller;
the payer is the account whose VERIFY frame approved PAYMENT.
-/

namespace PoolModel

inductive Mode where
  | default
  | verify
  | sender
  deriving DecidableEq, Repr

def Mode.code : Mode → Nat
  | .default => 0
  | .verify => 1
  | .sender => 2

structure Frame where
  target : Nat
  mode : Mode
  flags : Nat
  value : Nat
  dataLen : Nat
  /-- The first four data bytes as a big-endian number (only meaningful when `dataLen ≥ 4`). -/
  selector : Nat
  /-- EIP-8037 state-gas budget of the frame. -/
  stateLimit : Nat
  deriving DecidableEq, Repr

structure Sig where
  signer : Nat
  scheme : Nat
  msgEmpty : Bool
  /-- len(signature) (SIGPARAM 0x03, meaningful for ARBITRARY entries) -/
  len : Nat := 0
  /-- Abstract cryptography: the deployment's leanSPHINCS verifier accepts this entry's bytes under the
  key the validating account is configured with, over compute_sig_hash(tx). Supplied by whoever builds
  the transaction (the differential test knows which witnesses it corrupted); never computed here. -/
  verifies : Bool := false
  deriving DecidableEq, Repr

structure Tx where
  sender : Nat
  frames : List Frame
  nonceKeys : Nat
  nonceSeq : Nat
  blobs : Nat
  sigs : List Sig
  deriving Repr

/-- `settle(bytes,uint256[],uint256[])` -/
def SETTLE : Nat := 0xf4b589c7

/-- APPROVE scopes (frame flags bits 0..1). -/
def SCOPE_PAYMENT : Nat := 1
def SCOPE_EXECUTION : Nat := 2

/-- `PoolAccount.yul`, `verifyFrameApprove()`: `.ok` means it reaches `APPROVE(EXECUTION)`. -/
def poolPolicy (pool : Nat) (tx : Tx) (current : Nat) : Except Nat Unit :=
  if tx.sender ≠ pool then .error 1 else
  if tx.frames.length ≠ 3 then .error 2 else
  if current ≠ 0 then .error 3 else
  if tx.blobs ≠ 0 then .error 4 else
  if tx.nonceKeys = 0 then .error 5 else
  if tx.nonceSeq ≠ 0 then .error 6 else
  match tx.frames with
  | [f0, f1, f2] =>
    if f0.target ≠ pool then .error 10 else
    if f0.mode ≠ .verify then .error 11 else
    if f0.flags ≠ SCOPE_EXECUTION then .error 12 else
    if f0.value ≠ 0 then .error 13 else
    if f1.target = 0 ∨ f1.target = pool then .error 20 else
    if f1.mode ≠ .verify then .error 21 else
    if f1.flags ≠ SCOPE_PAYMENT then .error 22 else
    if f1.value ≠ 0 then .error 23 else
    if f2.target ≠ pool then .error 30 else
    if f2.mode ≠ .sender then .error 31 else
    if f2.flags ≠ 0 then .error 32 else
    if f2.value ≠ 0 then .error 33 else
    if f2.dataLen < 4 then .error 34 else
    if f2.selector ≠ SETTLE then .error 35 else
    .ok ()
  | _ => .error 2

/-- `ClassicPaymaster.yul`, VERIFY entry: `.ok` means it reaches `APPROVE(PAYMENT)`. -/
def paymasterPolicy (paymaster pool operator : Nat) (tx : Tx) (current : Nat) : Except Nat Unit :=
  if tx.sender ≠ pool then .error 1 else
  if current ≠ 1 then .error 2 else
  match tx.frames with
  | _ :: f1 :: _ =>
    if f1.target ≠ paymaster then .error 3 else
    if f1.mode ≠ .verify then .error 4 else
    if f1.flags ≠ SCOPE_PAYMENT then .error 5 else
    match tx.sigs with
    | [s] =>
      if s.signer ≠ operator then .error 7 else
      if s.scheme ≠ 1 then .error 8 else
      if !s.msgEmpty then .error 9 else
      .ok ()
    | _ => .error 6
  | _ => .error 3

def isOk {ε α : Type} : Except ε α → Bool
  | .ok _ => true
  | .error _ => false

/-- ARBITRARY signature scheme (EIP-8141): bytes unchecked by the protocol, read with SIGDATACOPY. -/
def ARBITRARY : Nat := 0
def LEANSPHINCS_SIG_BYTES : Nat := 5684

/-- `PQPaymaster.yul`, VERIFY entry: `.ok` means it reaches `APPROVE(PAYMENT)`. -/
def paymasterPQPolicy (paymaster pool : Nat) (tx : Tx) (current : Nat) : Except Nat Unit :=
  if tx.sender ≠ pool then .error 1 else
  if current ≠ 1 then .error 2 else
  match tx.frames with
  | _ :: f1 :: _ =>
    if f1.target ≠ paymaster then .error 3 else
    if f1.mode ≠ .verify then .error 4 else
    if f1.flags ≠ SCOPE_PAYMENT then .error 5 else
    match tx.sigs with
    | [s] =>
      if s.scheme ≠ ARBITRARY then .error 7 else
      if !s.msgEmpty then .error 8 else
      if s.len ≠ LEANSPHINCS_SIG_BYTES then .error 9 else
      if !s.verifies then .error 11 else
      .ok ()
    | _ => .error 6
  | _ => .error 3

/-- The first ARBITRARY entry over compute_sig_hash (empty msg), as SphincsFrameVerify.yul picks it. -/
def firstWitness : List Sig → Option Sig
  | [] => none
  | s :: rest => if s.scheme = ARBITRARY ∧ s.msgEmpty then some s else firstWitness rest

/-- A SphincsFrameAccount's VERIFY frame (verifyFrameAndApprove() → SphincsFrameVerify.yul): the first
ARBITRARY empty-msg witness must verify under the account's key; it then approves exactly the scope its
frame flags allow (which must be non-zero). `.ok scope` = APPROVE(scope). -/
def pqAccountPolicy (tx : Tx) (current : Nat) : Except Nat Nat :=
  match firstWitness tx.sigs with
  | none => .error 25
  | some s =>
    if !s.verifies then .error 26 else
    match tx.frames[current]? with
    | none => .error 27
    | some f => if f.flags % 4 = 0 then .error 21 else .ok (f.flags % 4)

/-- The deployment: pool account, paymaster, and the exit operator the paymaster trusts.
With `pqPaymaster` the paymaster is PQPaymaster (leanSPHINCS) and `operator` is unused. -/
structure Deployment where
  pool : Nat
  paymaster : Nat
  operator : Nat
  pqPaymaster : Bool := false
  deriving Repr

/-- The validation code a VERIFY frame runs: the policy of its target (anything else never approves). -/
def verifyAt (d : Deployment) (tx : Tx) (i : Nat) (f : Frame) : Bool :=
  f.mode = .verify ∧
    ((f.target = d.pool ∧ isOk (poolPolicy d.pool tx i)) ∨
     (f.target = d.paymaster ∧
        isOk (if d.pqPaymaster then paymasterPQPolicy d.paymaster d.pool tx i
              else paymasterPolicy d.paymaster d.pool d.operator tx i)))

/-- EIP-8250 state gas for first use of a nonce key (8141: billed to the payment approver's frame;
found by the differential test, see the policy differential test). -/
def NONCE_KEY_STATE_GAS : Nat := 97920

/-- Validity of a pool transaction under this deployment: both prefix frames (0 and 1) are VERIFY frames
whose validation approves, and the payment approver's state budget covers the fresh nonce keys.
This is what the differential test compares with 8141's simulator. -/
def valid (d : Deployment) (tx : Tx) : Bool :=
  match tx.frames with
  | f0 :: f1 :: _ =>
    verifyAt d tx 0 f0 && verifyAt d tx 1 f1 && decide (NONCE_KEY_STATE_GAS * tx.nonceKeys ≤ f1.stateLimit)
  | _ => false

/-- Validity of a transaction sent by a PQ account `acc`: frame 0 is the account's own VERIFY frame and it
approves execution and payment (scope 3); the account pays for the fresh nonce keys. -/
def validFromAccount (acc : Nat) (tx : Tx) : Bool :=
  match tx.frames with
  | f0 :: _ =>
    decide (tx.sender = acc) && decide (f0.target = acc) && decide (f0.mode = .verify) &&
      (match pqAccountPolicy tx 0 with | .ok 3 => true | _ => false) &&
      decide (NONCE_KEY_STATE_GAS * tx.nonceKeys ≤ f0.stateLimit)
  | _ => false

/-- Frames whose code runs with the pool as `msg.sender` once the transaction executes (EIP-8141:
SENDER frames call as `tx.sender`). -/
def callsAsSender (tx : Tx) : List Frame :=
  tx.frames.filter (fun f => f.mode = .sender)

end PoolModel
