# Post-Quantum Cryptography & Key Encapsulation (KEM): Concepts, Mechanics & Architecture

This guide explains **Post-Quantum Cryptography**, **Key Encapsulation Mechanisms (KEM)**, **Lattice Mathematics (Learning With Errors)**, and **Hybrid X-Wing** defense as implemented in Topi's KEM-Sphinx mixnet.

---

## 1. The Core Problem: Why Do We Need "Post-Quantum"?

Today, almost every secure connection on the internet (HTTPS, SSH, Bitcoin, Signal, PGP) relies on **Classical Public Key Cryptography** (such as RSA, Diffie-Hellman, and Elliptic Curves like X25519 / secp256k1).

### How Classical Cryptography Works

Classical algorithms rely on "trapdoor" mathematical problems that are simple to compute in one direction, but practically impossible to reverse on standard binary supercomputers:
- **Integer Factorization (RSA)**: Multiplying two 2,048-bit prime numbers $p \times q = N$ takes a fraction of a millisecond. Finding $p$ and $q$ given only $N$ takes billions of years on modern supercomputer clusters.
- **Discrete Logarithms (Diffie-Hellman / ECC)**: Computing $g^x \pmod p$ is trivial. Finding the private exponent $x$ given only $g^x$ and $p$ is computationally infeasible.

### The Threat: Shor's Algorithm (1994)

In 1994, mathematician Peter Shor proved that a sufficiently large **Quantum Computer** running **Shor's Algorithm** solves both discrete logarithms and prime factorization in polynomial time—**in a matter of minutes**:

$$\text{Classical Supercomputer: } \mathcal{O}\left(e^{(\sqrt[3]{\ln N \cdot (\ln \ln N)^2})}\right) \quad \longrightarrow \quad \text{Quantum Computer (Shor's): } \mathcal{O}\left((\log N)^3\right)$$

When large-scale quantum computers become operational, **every classical RSA and Elliptic Curve key pair will be compromised instantly.**

### The Immediate Danger: "Harvest Now, Decrypt Later" (HNDL)

Even if fault-tolerant quantum computers are years away, adversarial nation-states and automated collectors are **intercepting and recording encrypted internet traffic today**. 

> [!WARNING]
> **Harvest Now, Decrypt Later**: Traffic recorded off the wire today will be retroactively decrypted the moment a quantum computer is switched on. For private payments and financial anonymity, post-quantum security must be applied **now**, before the data ever touches the network.

---

## 2. What is a "Key Encapsulation Mechanism" (KEM)?

In modern high-performance cryptography, we do not encrypt heavy message payloads directly with public keys (which is extremely slow). Instead:
1. Two parties establish a short, symmetric **Shared Secret** (e.g., 32 random bytes).
2. They feed that 32-byte secret into a blazing-fast symmetric stream cipher (such as **BLAKE3 XOF** or ChaCha20) to encrypt the real message.

A **KEM (Key Encapsulation Mechanism)** is a specialized 3-step protocol designed specifically to generate and securely transmit this shared secret:

```text
  ALICE (Mixnet Node)                             BOB (Client)
  ───────────────────                             ────────────
1. Generate Keypair:
   • Public Key (PK)  ────── (Public Registry) ──► 
   • Private Key (SK) (Held secret in RAM)
                                               2. Encapsulate(PK):
                                                  Takes Alice's Public Key.
                                                  Outputs:
                                                  • 32-byte Shared Secret (ss)
                                                  • Ciphertext ("The Capsule", C)
                                                  
3. Decapsulate(SK, C):        ◄── Sends Capsule (C) ─┘
   Uses Private Key on Capsule.
   Recovers exact same 32-byte
   Shared Secret (ss)!
```

### The Real-World Padlock Analogy

1. **Alice** manufactures an **open padlock** (her Public Key) and leaves it on a public table. She keeps the **physical key** (her Private Key) locked in her pocket.
2. **Bob** walks up to the table. He rolls a pair of dice to generate a fresh, random 32-byte password $S$. He places $S$ inside a steel safe, snaps Alice’s padlock shut (**"Encapsulation"**), and mails the locked safe (**"The Capsule / Ciphertext"**) across the world to Alice.
3. Even if eavesdroppers intercept the safe on the road, nobody can open it without Alice's physical key.
4. **Alice** receives the safe, inserts her physical key (**"Decapsulation"**), unlocks it, and reads the exact same 32-byte password $S$.

---

## 3. How Does Post-Quantum Math Work? (Lattice Cryptography)

Post-quantum cryptography avoids prime numbers and elliptic curves completely. Instead, NIST's standardized post-quantum standard (**ML-KEM / CRYSTALS-Kyber**) is built on **Lattice-Based Cryptography**, specifically the **Learning With Errors (LWE)** problem.

### The Intuition: High School Algebra vs. LWE

Consider a system of standard linear equations from basic algebra:

```text
3x + 5y = 26
2x + 7y = 21
```

Any computer (classical or quantum) can solve this in microseconds using Gaussian elimination to find $x=7$ and $y=1$.

Now, introduce a tiny, controlled amount of **random "noise" (error $e$)** to each equation:

```text
3x + 5y ≈ 26.002
2x + 7y ≈ 20.998
4x + 9y ≈ 37.001
```

When expanded to **polynomial rings with matrices in hundreds or thousands of dimensions**:
- **If you hold the private trapdoor** (the exact mathematical structure of the noise), rounding away the error and recovering the secret vector is trivial.
- **If you do NOT hold the trapdoor**, solving for the secret vector requires finding the closest vector in an incomprehensibly vast, high-dimensional lattice grid.
- **Shor's Algorithm cannot solve this.** Lattice problems have no periodic algebraic structure for a quantum Fourier transform to exploit.

---

## 4. What is "X-Wing" in Topi? (Hybrid Defense)

Topi utilizes **X-Wing** (specified in [`crates/sphinx/src/lib.rs`](src/lib.rs)):

```rust
pub const CT: usize = x_wing::CIPHERTEXT_SIZE; // 1,120 bytes
```

### Why not use pure Post-Quantum ML-KEM?

Post-quantum lattice mathematics is relatively young compared to 40-year-old algorithms like Diffie-Hellman. If mathematicians find a structural weakness in lattice reduction tomorrow, pure ML-KEM could become vulnerable.

To guarantee unbreakable security under all circumstances, **X-Wing is a Hybrid KEM**. It executes two independent algorithms in parallel and binds their outputs:

```text
               ┌──────────────────────────────────────────────┐
               │              X-Wing Encapsulate              │
               └──────────────────────┬───────────────────────┘
                                      │
            ┌─────────────────────────┴────────────────────────┐
            ▼                                                  ▼
1. Classical (X25519)                               2. Post-Quantum (ML-KEM-768)
   • 32-byte ephemeral public key                      • 1,088-byte lattice ciphertext
   • Battle-tested classical math                      • Quantum-proof lattice noise
   • Produces: Secret_Classical                        • Produces: Secret_PQ
            │                                                  │
            └─────────────────────────┬────────────────────────┘
                                      ▼
                      Combined via SHA3-256 / HKDF:
         Final Shared Secret = KDF(Secret_Classical ‖ Secret_PQ)
```

> [!IMPORTANT]
> **The Dual-Layer Guarantee**:
> - If an adversary attacks with a **quantum computer**: `ML-KEM-768` holds the line.
> - If an unexpected theoretical flaw is found in **lattice math**: `X25519` holds the line.
> - An adversary must break **BOTH** independent mathematical foundations simultaneously to decrypt even a single hop!

---

## 5. How It Fits Into Every Hop of Topi's Mixnet

When the Client prepares a message, it selects a 3-hop route:
$$\text{Client} \longrightarrow \text{Gateway (Hop 1)} \longrightarrow \text{Mix (Hop 2)} \longrightarrow \text{Exit (Hop 3)}$$

### Step 1: Client-Side Triple Encapsulation
Before sending a single bit, the Client runs X-Wing encapsulation **three separate times**:

```text
Hop 1 (Gateway):  Encapsulate(PK_gateway) ──► ss1 (32 B)  +  Capsule 1 (1,120 B)
Hop 2 (Mix):      Encapsulate(PK_mix)     ──► ss2 (32 B)  +  Capsule 2 (1,120 B)
Hop 3 (Exit):     Encapsulate(PK_exit)    ──► ss3 (32 B)  +  Capsule 3 (1,120 B)
```

- $ss_1, ss_2, ss_3$ stay strictly in the Client's RAM.
- The Client uses $ss_3, ss_2, ss_1$ to nest the routing instructions backwards like Russian nesting dolls.
- **Capsule 1** is placed at the front of the packet.
- **Capsule 2** is encrypted inside Layer 1.
- **Capsule 3** is encrypted inside Layer 2.

---

### Step 2: Hop-by-Hop Decapsulation & Unpeeling

When a packet arrives at each node:

```text
Incoming 8,192-byte Packet
   │
   ├── 1. Node reads its 1,120-byte Capsule from packet header
   │
   ├── 2. Calls Decapsulate(node_private_key, Capsule)
   │      └── Calculates the exact 32-byte shared secret (ss) in ~0.18 ms!
   │
   ├── 3. Derives symmetric stream cipher key (BLAKE3 XOF) from ss
   │
   ├── 4. Peels its onion layer:
   │      • Verifies hop MAC
   │      • Reads its routing instruction (e.g. next IP and delay)
   │      • Uncovers the next hop's Capsule and MAC
   │      • Appends filler padding to maintain exact 8,192-byte invariant
   │
   └── 5. Forwards the new 8,192-byte packet to the next hop!
```

---

## 6. Summary Comparison Matrix

| Stage | Node Role | Capsule Read | Private Key Used | Secret Recovered | What is Learned? |
| :---: | :---: | :---: | :---: | :---: | :---: |
| **Hop 1** | **Gateway** | `Capsule 1` (Bytes `0..1120`) | $SK_{gateway}$ | **$ss_1$** (32 bytes) | Uncovers `Capsule 2` & routing: *"Forward to Mix"* |
| **Hop 2** | **Mix** | `Capsule 2` (Bytes `0..1120`) | $SK_{mix}$ | **$ss_2$** (32 bytes) | Uncovers `Capsule 3` & routing: *"Shuffle & Forward to Exit"* |
| **Hop 3** | **Exit** | `Capsule 3` (Bytes `0..1120`) | $SK_{exit}$ | **$ss_3$** (32 bytes) | Uncovers plaintext transaction & queues for ZK prover |

---

## Related Documentation

- [**Packet States by Hop**](PACKET_STATES_BY_HOP.md) - Byte-by-byte layout of the 8,192-byte packet at each node.
- [**MAC in Mixnets**](MAC_IN_MIXNETS.md) - Authentication tags, BLAKE3 keyed hashing, and tagging attack defenses.
- [**KEM-Sphinx Implementation**](README.md) - Crate API and benchmarking details for `topi-sphinx`.
