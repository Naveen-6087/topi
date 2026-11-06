# Post-Quantum Key Encapsulation (KEM) & Hop-by-Hop Sphinx Routing

This document explains in detail how **Post-Quantum Key Encapsulation (X-Wing KEM)** and **Sphinx Onion Routing** interact across every hop of the Topi mixnet (`Client -> Gateway -> Mix -> Exit`).

---

## 1. High-Level Concept: The 3-Layer Onion

A mixnet packet does not use a single shared key. Instead, the Client creates **three separate cryptographic sessions**—one for each node along the path:

```text
CLIENT
  │
  ├── 1. Gateway Session ──► Shared Secret 1 (ss1)  +  Capsule 1 (1,120 B)
  ├── 2. Mix Session     ──► Shared Secret 2 (ss2)  +  Capsule 2 (1,120 B)
  └── 3. Exit Session    ──► Shared Secret 3 (ss3)  +  Capsule 3 (1,120 B)
```

### Critical Rules:
1. **Shared Secrets ($ss_1, ss_2, ss_3$) are NEVER transmitted over the network.** They are derived independently inside the RAM of the Client and each respective node.
2. **What travels on the wire are Capsules.** A Capsule is a 1,120-byte mathematical puzzle built with lattice cryptography. Only the node holding the matching private key can solve it to recover the exact same 32-byte shared secret.
3. **The packet size never changes.** Every packet is strictly **8,192 bytes** at every point in the network.

---

## 2. Phase 1: Client Packet Construction (Inside Out)

Before sending anything over the network, the Client builds the packet in memory from the **innermost layer to the outermost layer** (like Russian nesting dolls).

```text
Step 1: Wrap Exit Layer
┌────────────────────────────────────────────────────────┐
│ [Capsule 3] + [Routing 3: "is_exit: true, Deliver"]    │
└────────────────────────────────────────────────────────┘
                           │
                           ▼ Encrypt under ss2
Step 2: Wrap Mix Layer
┌────────────────────────────────────────────────────────┐
│ [Capsule 2] + [Routing 2: "Go to Exit"]                │
│             + [Encrypted Hop 3 Layer]                  │
└────────────────────────────────────────────────────────┘
                           │
                           ▼ Encrypt under ss1
Step 3: Wrap Gateway Layer
┌────────────────────────────────────────────────────────┐
│ [Capsule 1] + [Routing 1: "Go to Mix"]                 │
│             + [Encrypted Hop 2 Layer (with Capsule 2)] │
└────────────────────────────────────────────────────────┘
```

### Notice Where Capsule 2 and Capsule 3 Come From:
- **Capsule 1** sits openly at the very front of the packet.
- **Capsule 2** is hidden inside the encrypted body of layer 1 (encrypted under $ss_1$).
- **Capsule 3** is hidden inside the encrypted body of layer 2 (encrypted under $ss_2$).

---

## 3. Phase 2: What Happens at Each Hop

### Hop 1: Gateway Node

```text
Incoming Wire: [Capsule 1 (1,120 B)] | [MAC 1 (16 B)] | [Encrypted Data (7,056 B)]
```

1. **Read Capsule 1**: The Gateway extracts the front 1,120 bytes.
2. **Decapsulate**:
   $$\text{Decapsulate}(\text{gateway\_private\_key}, \text{Capsule 1}) \longrightarrow \mathbf{ss_1} \text{ (32 bytes)}$$
   *(Takes $\approx 0.18$ ms on a single CPU core).*
3. **Verify Integrity**: Gateway derives the `MAC Key 1` from $ss_1$ and verifies `MAC 1`.
4. **Peel Layer 1**: Gateway uses $ss_1$ with BLAKE3 XOF stream cipher to decrypt the remaining data.
5. **The Reveal**:
   - Routing: *"Forward to Mix Node at 1.2.3.4 after delay $D$"*.
   - **Capsule 2 & MAC 2 are now decrypted and uncovered!**
6. **Shift & Pad**: Gateway strips Capsule 1 and shifts Capsule 2 to the front. To prevent the packet from shrinking, it appends pseudo-random filler padding to the end.
7. **Forward**: Sends the new 8,192-byte packet to the Mix.

---

### Hop 2: Mix Node

```text
Incoming Wire: [Capsule 2 (1,120 B)] | [MAC 2 (16 B)] | [Encrypted Data (7,056 B)]
```

1. **Anonymity Boundary**: The Mix node **never saw Capsule 1** and does not know the Client's IP address. It only knows the packet arrived from the Gateway.
2. **Read Capsule 2**: The Mix extracts the front 1,120 bytes.
3. **Decapsulate**:
   $$\text{Decapsulate}(\text{mix\_private\_key}, \text{Capsule 2}) \longrightarrow \mathbf{ss_2} \text{ (32 bytes)}$$
4. **Verify Integrity**: Verifies `MAC 2` using `MAC Key 2` derived from $ss_2$.
5. **Peel Layer 2**: Uses $ss_2$ to decrypt the next layer:
   - Routing: *"Batch shuffle, then forward to Exit Node at 5.6.7.8"*.
   - **Capsule 3 & MAC 3 are now decrypted and uncovered!**
6. **Batch & Shuffle**: The Mix puts the packet into a batch pool. After the batch interval (e.g., 1.5s), it shuffles the batch with CSPRNG and forwards the packet to Exit.

---

### Hop 3: Exit Node

```text
Incoming Wire: [Capsule 3 (1,120 B)] | [MAC 3 (16 B)] | [Encrypted Payload (7,056 B)]
```

1. **Read Capsule 3**: Exit extracts the front 1,120 bytes.
2. **Decapsulate**:
   $$\text{Decapsulate}(\text{exit\_private\_key}, \text{Capsule 3}) \longrightarrow \mathbf{ss_3} \text{ (32 bytes)}$$
3. **Verify Integrity**: Verifies `MAC 3` using `MAC Key 3`.
4. **Peel Layer 3**:
   - Routing reveals: `is_exit: true` (*"You are the final destination"*).
5. **Decrypt Payload**:
   - Exit decrypts the inner 4,688-byte payload using $ss_3$.
   - Verifies the end-to-end BLAKE3 integrity tag.
   - Extracts the original transaction / spend proof.
6. **Execution**: Places the spend proof into the zero-knowledge prover queue (`spend-queue`) and routes the receipt back via the sender's SURB.

---

## 4. Packet Transformation State Diagram

```text
[CLIENT]
   │
   ▼ Sends 8,192 B Packet:
   [Capsule 1] [MAC 1] [Encrypted: Routing 1, Capsule 2, MAC 2, Layer 3, Payload]
   │
   ▼
[GATEWAY]
   ├── Opens Capsule 1 using private key ──► Gets ss1
   ├── Peels Layer 1 with ss1
   ├── Uncovers Capsule 2 & MAC 2
   └── Appends filler padding
   │
   ▼ Sends 8,192 B Packet:
   [Capsule 2] [MAC 2] [Encrypted: Routing 2, Capsule 3, MAC 3, Payload]
   │
   ▼
[MIX]
   ├── Opens Capsule 2 using private key ──► Gets ss2
   ├── Peels Layer 2 with ss2
   ├── Uncovers Capsule 3 & MAC 3
   └── Appends filler padding
   │
   ▼ Sends 8,192 B Packet:
   [Capsule 3] [MAC 3] [Encrypted: Routing 3, Payload]
   │
   ▼
[EXIT]
   ├── Opens Capsule 3 using private key ──► Gets ss3
   ├── Peels Layer 3 with ss3
   └── Recovers Plaintext Spend Transaction!
```

---

## 5. Cryptographic Specifications

| Component | Mechanism | Specification |
| :--- | :--- | :--- |
| **KEM Algorithm** | **X-Wing** | Hybrid: `ML-KEM-768` (Post-Quantum Lattice) + `X25519` (Classical EC) |
| **Ciphertext (Capsule)** | `CIPHERTEXT_SIZE` | Exactly **1,120 bytes** ($1,088\text{ B ML-KEM} + 32\text{ B X25519}$) |
| **Shared Secret ($ss$)** | Derived Key | Exactly **32 bytes** (256 bits) |
| **Layer Stream Cipher** | **BLAKE3 XOF** | Fast, constant-time pseudorandom keystream generator |
| **Packet Size** | Fixed Buffer | Exactly **8,192 bytes** across all hops |
| **Replay Protection** | Hash Set | 32-byte tag cached per hop; duplicate capsules are rejected immediately |
