import Lean.Data.Json
import PoolModel

/-!
# `pool-policy`: the Lean policy model as a line-delimited JSON service

stdin, one per line: `{"deployment": {"pool","paymaster","operator"}, "tx": {...}}` with addresses and
selectors as 0x-hex strings; stdout, one per line: `{"valid": bool, "pool": "ok"|code, "paymaster": "ok"|code}`.
Every answer is computed by `PoolModel.valid`, `poolPolicy` and `paymasterPolicy`, the functions the
theorems in `PoolModel.PolicyProofs` are about. Parsing is the unverified IO boundary.
-/

open Lean PoolModel

def hexNat (s : String) : Except String Nat := do
  let s := s.toLower
  unless s.startsWith "0x" do throw s!"not hex: {s}"
  (s.drop 2).toString.foldl (fun acc c => do
    let a ← acc
    let d ← if '0' ≤ c && c ≤ '9' then pure (c.toNat - '0'.toNat)
      else if 'a' ≤ c && c ≤ 'f' then pure (c.toNat - 'a'.toNat + 10)
      else throw s!"bad hex digit in {s}"
    pure (a * 16 + d)) (pure 0)

def str (j : Json) (k : String) : Except String String := j.getObjValAs? String k
def nat (j : Json) (k : String) : Except String Nat := j.getObjValAs? Nat k
def addr (j : Json) (k : String) : Except String Nat := do hexNat (← str j k)

def parseMode : Nat → Except String Mode
  | 0 => pure .default
  | 1 => pure .verify
  | 2 => pure .sender
  | m => throw s!"mode {m}"

def parseFrame (j : Json) : Except String Frame := do
  pure { target := ← addr j "target", mode := ← parseMode (← nat j "mode"), flags := ← nat j "flags",
         value := ← nat j "value", dataLen := ← nat j "dataLen", selector := ← addr j "selector",
         stateLimit := ← nat j "stateLimit" }

def optNat (j : Json) (k : String) (d : Nat) : Nat := (j.getObjValAs? Nat k).toOption.getD d
def optBool (j : Json) (k : String) (d : Bool) : Bool := (j.getObjValAs? Bool k).toOption.getD d

def parseSig (j : Json) : Except String Sig := do
  let signer := ((str j "signer").toOption.bind (fun s => (hexNat s).toOption)).getD 0
  pure { signer, scheme := ← nat j "scheme", msgEmpty := ← j.getObjValAs? Bool "msgEmpty",
         len := optNat j "len" 0, verifies := optBool j "verifies" false }

def parseTx (j : Json) : Except String Tx := do
  let frames ← (← j.getObjValAs? (Array Json) "frames").toList.mapM parseFrame
  let sigs ← (← j.getObjValAs? (Array Json) "sigs").toList.mapM parseSig
  pure { sender := ← addr j "sender", frames, nonceKeys := ← nat j "nonceKeys", nonceSeq := ← nat j "nonceSeq",
         blobs := ← nat j "blobs", sigs }

def verdict : Except Nat Unit → Json
  | .ok () => "ok"
  | .error c => toJson c

def handleAccount (j : Json) : Except String Json := do
  let acc ← addr j "account"
  let tx ← parseTx (← j.getObjVal? "tx")
  let scope : Json := match pqAccountPolicy tx 0 with | .ok s => toJson s | .error c => Json.mkObj [("error", toJson c)]
  pure (Json.mkObj [("valid", toJson (validFromAccount acc tx)), ("account", scope)])

def handle (line : String) : Json :=
  match (Json.parse line).toOption.bind (fun j => (j.getObjVal? "account").toOption.map (fun _ => j)) with
  | some j => match handleAccount j with
    | .ok r => r
    | .error e => Json.mkObj [("error", e)]
  | none =>
  match (do
    let j ← Json.parse line
    let dj ← j.getObjVal? "deployment"
    let d : Deployment := { pool := ← addr dj "pool", paymaster := ← addr dj "paymaster", operator := ← addr dj "operator",
                            pqPaymaster := optBool dj "pqPaymaster" false }
    let tx ← parseTx (← j.getObjVal? "tx")
    pure (d, tx) : Except String (Deployment × Tx)) with
  | .error e => Json.mkObj [("error", e)]
  | .ok (d, tx) =>
    Json.mkObj [("valid", toJson (valid d tx)),
                ("pool", verdict (poolPolicy d.pool tx 0)),
                ("paymaster", verdict (if d.pqPaymaster then paymasterPQPolicy d.paymaster d.pool tx 1
                                       else paymasterPolicy d.paymaster d.pool d.operator tx 1))]

partial def loop (stdin stdout : IO.FS.Stream) : IO Unit := do
  let line ← stdin.getLine
  if line.isEmpty then return
  let l := line.trimAscii.toString
  unless l.isEmpty do
    stdout.putStrLn (handle l).compress
    stdout.flush
  loop stdin stdout

def main : IO Unit := do
  loop (← IO.getStdin) (← IO.getStdout)
