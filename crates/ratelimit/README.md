# topi-ratelimit

Post-quantum anonymous rate limiting (RLN-style).

This crate rate-limits mixnet packets per member and epoch without revealing the member. Its epoch
proofs are made with **Binius64** in **zero-knowledge mode**. Binius64 is a hash-based proof
system: binary-tower sumcheck plus BaseFold/FRI, with SHA-256 Merkle commitments. There are no
pairings, no elliptic curves and no trusted setup.

## Design

```
Registration (stake-gated, out of scope):  member publishes  C = SHA256("TPID" ‖ sk)
                                            entry nodes agree on membership root M (depth 16)

Once per epoch (client):
  t_i = SHA256("TPTK" ‖ sk ‖ epoch:u64be ‖ i:u32be)      for i in 0..L   (L = 16)
  R   = MerkleRoot(t_0 … t_{L-1})                       complete binary tree, node = SHA256(l ‖ r)
  π   = ZK proof of:  ∃ sk, path :  MerkleRoot_path(SHA256("TPID"‖sk)) = M
                                   ∧  R = MerkleRoot(t_i(sk, epoch))
        public inputs (M, epoch, R)
  EpochTicket = (R, π)

Per packet:   Token = (t_i, i, path_i → R)        32 + 4 + 32·log2 L bytes  (164 B for L = 16)

Entry node:   verify π once per R  →  accept each distinct t_i under R once.
              Only L leaves exist under R, so at most L packets per (member, epoch).
```

All hashes are standard FIPS 180-4 SHA-256 with padding. A leaf (36 B) and a token (48 B) each
fit in one compression block. A node (64 B) takes two blocks, and the second is a constant
padding block. The 4-byte tags and the different message lengths keep the three kinds of hash
apart. The circuit (`src/circuit.rs`) uses the `sha256_compress` gadget from
`binius-circuits` and builds each padded block from constant words. It is the exact in-circuit
twin of `src/hash.rs`. The valid-flow test checks that the two agree: the root computed natively
must satisfy the circuit.

For default parameters the circuit has 1 + 16·2 + 16 + 15·2 = 79 SHA-256 compressions, which
come to 28,757 inner AND constraints.

### How zero-knowledge (hiding) is actually used

- Proving goes through `binius_prover::zk_config::ZKProver`, and verification through
  `binius_verifier::zk_config::ZKVerifier`. The crate never uses the transparent
  `Prover`/`Verifier`.
- `ZKVerifier::setup` runs the inner Binius64 verifier symbolically and turns it into an outer
  Spartan constraint system.
- That outer system is padded with blinding rows (`BlindingInfo::for_fri_queries`) sized to the
  number of FRI queries.
- The inner proof's messages are never sent in the clear. They become private witness of the
  outer proof, and all oracles are committed through a BaseFold **ZK** compiler with random masks.
- `RlnSystem::prove_epoch` passes a fresh CSPRNG (`rand::rng()`) into `ZKProver::prove`.
- The test `zk_proofs_are_randomized` shows that two proofs of the same statement are different
  byte strings and that both verify.
- The only public values are `M`, `epoch` and `R`. `sk`, the leaf, the member index, the
  membership path and the individual `t_i` stay hidden.

### Fiat–Shamir and statement binding

The transcript is `HasherChallenger<Sha256>`. The ZK verifier observes the public words
`(M, epoch, R)` before anything else. So a proof cannot be moved to another `M`, `epoch` or `R`;
the tests check all three. `VerifierTranscript::finalize` rejects trailing bytes, and any panic
inside verification is caught and counted as a rejection.

## API (`src/lib.rs`)

```rust
let id = Identity::random();
let tree = MembershipTree::new(16, &[id.commitment(), /* … */]);
let (m, path) = (tree.root(), tree.path(0).unwrap());

let ticket = prove_epoch(&id, &path, &m, epoch, 16)?;        // EpochTicket { token_root, proof }
assert!(verify_epoch(&m, epoch, &ticket));                    // default params (16, 16)

let tok = token(&id, epoch, i, 16).unwrap();                  // None for i >= L
assert!(verify_token(&ticket.token_root, &tok));

let mut rl = RateLimiter::new(system(Params::default())?, m, epoch);
rl.accept(&ticket, &tok);            // first time: verifies π, registers R
rl.accept(&ticket.token_root, &tok2) // later: bare R is enough
// -> AcceptResult::{Accepted, Duplicate, OverLimit, Invalid}
```

- **`RlnSystem` / `system(params)`:** the compiled circuit plus the ZK prover and verifier keys,
  cached for each `Params { depth, l }`. Building one costs about 0.4 s, so reuse it.
- **Configurable parameters:** `depth` can be 1–40 and `L` can be any power of two from 2 to
  2^20. `verify_epoch` always uses the default parameters. Use `verify_epoch_with` or
  `RlnSystem::verify_epoch` for others. A verifier deliberately does not take `L` from the
  ticket, so a client cannot choose its own limit.
- **Wire formats:** `EpochTicket::{to,from}_bytes` is `R ‖ proof`.
  `Token::{to,from}_bytes` is `t ‖ i ‖ path`.

## How to run

Binius64 is a git dependency pinned at commit `4428e759dd27956bd5b9ae5025d67eb8ccc1accf`.
Build with `RUSTFLAGS="-C target-cpu=native"` for the fastest proving.

```bash
cargo test --release -p topi-ratelimit                 # 11 integration tests, ~4 s after build
cargo run  --release -p topi-ratelimit --example bench # depth 16, L 16, 5 iterations
cargo run  --release -p topi-ratelimit --example bench -- 20 64 3
```

Use `--release`. In debug builds the prover is very slow, even though dependencies are built
with opt-level 3.

## Measured numbers

These were measured on an Intel Core i5-12500H laptop (16 threads), Windows 11, with `--release`
and `target-cpu=native`. FRI runs at rate 1/4 (`LOG_INV_RATE = 2`), and the verifier targets
96-bit security (Binius `SECURITY_BITS`).

| depth / L | inner AND constraints | prove (median) | verify (median) | epoch proof | token |
|---|---|---|---|---|---|
| **16 / 16** | 28,757 | **221 ms** | **186 ms** | **323,424 B** (≈316 KiB) | **164 B** |
| 20 / 64 | 84,085 | 255 ms | 199 ms | 328,160 B | 228 B |

Other costs for depth 16 / L 16:

| step | time |
|---|---|
| Circuit build plus ZK-verifier setup (once per process) | 343 ms |
| ZK-prover setup (once per process) | 80 ms |
| Native token-path check | 0.39 µs |
| Remaining 15 `RateLimiter::accept` calls, all together | 0.1 ms |

Effect of the FRI rate at depth 16 / L 16:

| rate | proof size | prove | verify |
|---|---|---|---|
| 1/2 | 436,512 B | 250 ms | 235 ms |
| 1/4 (chosen) | 323,424 B | 227 ms | 215 ms |
| 1/8 | 301,728 B | 356 ms | 273 ms |

Prove and verify times change little with circuit size at this scale. Most of the cost is the
fixed overhead of the ZK wrapper (the outer Spartan proof over the inner verifier).

## Security notes

- **What is post-quantum.**
  - Every component rests only on hash functions: SHA-256 in the statement, SHA-256 Merkle
    commitments in BaseFold/FRI, and a SHA-256 Fiat–Shamir challenger.
  - There is no discrete-log, pairing or lattice assumption.
  - Soundness in the quantum random-oracle model is the standard assumption for FRI-based SNARKs.
    Grover-type attacks halve the bit security of hash preimages and collisions. 256-bit digests
    keep a comfortable margin, but the 96-bit IOP soundness target should be raised for
    long-term deployment.
- **Anonymity (unlinkability across epochs).** `R` and the `t_i` are PRF outputs of `sk`
  (SHA-256 keyed by a secret prefix). Tickets from different epochs cannot be linked to each other
  or to the commitment `C`, given ZK and the PRF assumption.
- **Linkability within an epoch.** All packets one member sends in an epoch carry tokens under
  the same `R`. Any entry node that sees them can link them to one pseudonymous sender. That is
  inherent to design 1; it is the price of verifying the proof only once. Mitigations: keep epochs
  short, use smaller `L`, or move to a design with a proof per packet.
- **No slashing.** Going over the limit is impossible rather than punishable: a client has only
  `L` valid tokens under any proven `R`. Producing a second valid `R` for the same epoch is
  impossible because `R` is a deterministic function of `(sk, epoch)`. Nothing reveals `sk` on
  misuse, unlike Shamir-share RLN. Stake or registration gates entry into the membership tree,
  and removing a misbehaving member (for example, one reported out of band) means rebuilding `M`.
- **Cross-node double spending.** Each entry node keeps its own `RateLimiter`. A client can spend
  the same `t_i` once at each of several entry nodes. A global limit needs a shared (gossiped)
  spent-token set for each `R`.
- **Domain separation and empty slots.**
  - Tags and message lengths keep leaf, token and node hashes apart.
  - Both trees have a fixed depth enforced in the circuit or by `verify_token_with_l`, which
    rules out confusion between levels.
  - Empty membership slots are `0^256`, and nobody knows a preimage for that value.

## Honest limitations

- **The full statement is proven.** It is the membership path plus all `L` tokens plus the root
  `R`; it is not a subset. The circuit is not audited, though. Binius64 is research-grade,
  pre-1.0 software, and its ZK wrapper is new.
- **The proof is large.** It is about 316 KiB, against about 128–192 B for Groth16-based RLN. It
  is sent once per epoch per entry node, not per packet, but it is still a real bandwidth cost.
  Recursion or proof compression in Binius could reduce it, but this crate does not try.
- **Verification takes about 190 ms.** That is acceptable once per `R` per epoch, but it is a
  denial-of-service surface. An attacker can send junk tickets with fresh `R`s. The `rejected`
  cache is keyed by `R`, so a fresh `R` defeats it. Rate-limit ticket submissions at the network
  layer, for example per connection or with proof-of-work.
- **`OverLimit` is defence in depth.** Valid tokens can never trigger it. The tests show that an
  index ≥ L has no valid path and that the (L+1)-th distinct token cannot exist.
- **Not built here:**
  - Membership management (registration, staking, removal, history of accepted roots).
  - Epoch clock and synchronisation.
  - Persistence of the per-R token sets.
  - Constant-time handling of `sk`.
  - Zeroisation of secrets.
- **Unverified ZK claim.** Hiding relies on Binius' ZK wrapper as implemented. The only
  empirical check here is that proofs are randomized. There is no formal audit of
  zero-knowledge.
