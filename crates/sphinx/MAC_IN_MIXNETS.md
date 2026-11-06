# Message Authentication Codes (MAC) in Mixnets: Architecture & Security Deep Dive

This document explains the role, mechanics, and necessity of **Message Authentication Codes (MAC)** and **MAC Keys** within post-quantum mixnets (specifically Topi / KEM-Sphinx).

---

## 1. What is a MAC vs. a MAC Key?

| Term | What It Is | Size | Visibility | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **`MAC Key`** | A shared secret key | 32 bytes (256 bits) | **Strictly Private** (RAM only) | Secret input used to compute and verify the authentication tag. |
| **`MAC (Tag)`** | Cryptographic digest / stamp | 16 bytes (128 bits) | **Public** (Inside packet header) | Verification tag attached to each layer of the Sphinx onion. |

### The Core Formula

```text
MAC = BLAKE3_Keyed_Hash(MAC_Key, Routing_Data)
```

- **Only someone who knows the `MAC Key`** can calculate a valid `MAC`.
- **Anyone holding the `MAC Key`** can independently calculate the hash and verify whether the message was tampered with.

---

## 2. Why Encryption Alone is NOT Enough

A common misconception in network security is: *"If the packet is encrypted, isn't it already secure?"*

> [!WARNING]
> **Encryption provides confidentiality, but NOT integrity or authenticity.**

In stream ciphers (such as AES-CTR, ChaCha20, or BLAKE3 XOF used in mixnets), ciphertext is created by XORing plaintext with a pseudorandom keystream:

$$\text{Ciphertext} = \text{Plaintext} \oplus \text{Keystream}$$

### The Bit-Flipping Vulnerability

If an active attacker intercepts a packet on the network, they do not need to know the decryption key to alter data. If they flip bit $k$ of the ciphertext:

$$\text{Ciphertext}' = \text{Ciphertext} \oplus \Delta$$

When the mix node decrypts it:

$$\text{Plaintext}' = \text{Ciphertext}' \oplus \text{Keystream} = \text{Plaintext} \oplus \Delta$$

**Without a MAC:**
1. The mix node decrypts the flipped bits into corrupted routing data.
2. The packet may be misrouted, crash the node, or trigger distinguishable error messages that break user anonymity.

---

## 3. Threat Models: What MACs Defend Against in Mixnets

### A. Tagging Attacks (The #1 Mixnet Threat)

In a mixnet without per-hop MACs:
1. A malicious ISP or eavesdropper modifies ("tags") a few bits of a packet entering **Gateway (Hop 1)**.
2. The packet travels through the **Mix (Hop 2)** and gets shuffled in a batch.
3. The adversary monitors traffic leaving the mixnet at **Exit (Hop 3)** or the blockchain.
4. When a packet fails with an error or shows the predictable corrupted bit pattern at Hop 3, the attacker correlates the input with the output: **the user's identity is completely deanonymized!**

> [!IMPORTANT]
> **Defense with MAC:**
> If any bit is modified, the MAC check at the very next hop fails instantly. The packet is dropped immediately, stopping tagging attacks in their tracks.

### B. Routing Hijacking & Splicing
An attacker could cut the header from Packet A and splice it onto Packet B. Because the MAC binds the specific cryptographic shared secret to that exact routing payload, spliced packets are rejected.

### C. Replay Attacks
In KEM-Sphinx, nodes maintain a set of seen cryptographic tags (`seen: HashSet<[u8; 32]>`). The MAC tag uniquely identifies the valid layer. If an attacker replays the exact same packet, the node detects the duplicate MAC/tag and discards it without re-processing.

---

## 4. How MACs Work Across the 3 Hops

In Topi's 3-hop Sphinx design (`Client -> Gateway -> Mix -> Exit`):

```text
CLIENT (Builds Onion Backwards)
  │
  ├── 1. Derives Shared Secrets:
  │      ss3 (Exit), ss2 (Mix), ss1 (Gateway)
  │
  ├── 2. Derives Authentication Keys:
  │      ss3 ──► MAC Key 3
  │      ss2 ──► MAC Key 2
  │      ss1 ──► MAC Key 1
  │
  └── 3. Generates Authentication Tags:
         MAC 3 = BLAKE3(MAC Key 3, Routing_Hop_3)
         MAC 2 = BLAKE3(MAC Key 2, Routing_Hop_2 || Encrypted_Hop_3)
         MAC 1 = BLAKE3(MAC Key 1, Routing_Hop_1 || Encrypted_Hop_2)
```

### On the Wire (Packet Structure)

```text
┌──────────────┬──────────┬────────────────────────────────────────────────────────┐
│  Capsule 1   │  MAC 1   │ Encrypted Routing Bytes (contains Capsule 2, MAC 2...) │
│ (1,120 B)    │ (16 B)   │ (2,368 B)                                              │
└──────────────┴──────────┴────────────────────────────────────────────────────────┘
```

### Step-by-Step Execution at Each Node

```text
Incoming Packet
       │
       ▼
[Decapsulate Capsule] ──► Recovers 32-byte Shared Secret (ss)
       │
       ▼
[KDF] ──────────────────► Computes MAC Key
       │
       ▼
[Calculate MAC] ────────► BLAKE3(MAC Key, Encrypted Routing Bytes)
       │
       ▼
[Compare with Packet MAC]
       │
       ├─── MISMATCH ──► Drop packet immediately (Silent drop, no error leaked)
       │
       └─── MATCH ─────► Verify OK! Peel onion layer and forward
```

---

## 5. Security Properties Summary

1. **Hop-by-Hop Authentication**: Each node only possesses its own MAC key ($ss_i$). Hop 1 cannot verify or forge Hop 2's MAC.
2. **Timing-Attack Resistance**: MAC verification always uses constant-time equality checks (`subtle::ConstantTimeEq`) so verification time never leaks information about matching bytes.
3. **Silent Drop Policy**: When a MAC fails, nodes log an internal counter (`Event::Rejected`) and drop the packet without returning error packets over the network, preventing oracle attacks.
4. **Post-Quantum Resilience**: BLAKE3 uses a 256-bit internal state with tree-hashing properties. Grover's algorithm only reduces pre-image security to 128 bits, well beyond quantum cryptanalysis thresholds.

---

## Related Documentation

- [**Packet States by Hop**](PACKET_STATES_BY_HOP.md) - Exact byte layout at Gateway, Mix, and Exit.
- [**Post-Quantum KEM & Hop-by-Hop Routing**](POST_QUANTUM_KEM_HOP_ROUTING.md) - Deep dive on X-Wing KEM derivation and onion unpeeling.
- [**Topi Mixnet Overview**](../mixnet/README.md) - High-level network architecture and roles.
