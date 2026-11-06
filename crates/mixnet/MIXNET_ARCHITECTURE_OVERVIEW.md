# Topi Mixnet: Architecture, Node Roles & System Overview

This document provides a complete high-level and structural overview of the **Topi Post-Quantum Mixnet** (`crates/mixnet`), explaining the problem it solves, how many node types exist, how they operate together, and the end-to-end anonymity guarantees provided to users.

---

## 1. What is a Mixnet as a Whole?

In standard blockchain networks, privacy breaks at the **network transport layer**:
- Even if a transaction payload is zero-knowledge encrypted, an ISP or eavesdropper sees the sender's **IP address**, **exact packet timestamp**, and **packet size**.
- Timing and packet correlation easily deanonymize users without needing to break cryptography.

### The Mixnet Solution: The "Cryptographic Blender"
A **Mixnet** (Mixing Network) is a multi-hop overlay network that unlinks sender identity from message destinations:
1. **Cryptographic Unlinking**: Layered onion encryption (KEM-Sphinx). No single node knows both the origin and the destination.
2. **Timing Unlinking (Batch & Shuffle)**: Relay nodes do not forward packets immediately upon arrival. They accumulate packets over a batch window, **shuffle the batch in random order** using a cryptographically secure random number generator (CSPRNG), and flush them out together.
3. **Traffic Invariance**: Every packet is padded to the exact same size (**8,192 bytes**). Real transactions, return receipts, pings, and dummy cover traffic are completely indistinguishable on the wire.

---

## 2. The 4 Specialized Node Roles

Topi stratifies its network into **4 specialized roles**:

```text
FORWARD PATH (Sending a Transaction):
[Client] ──(1)──► [ GATEWAY ] ──(2)──► [ MIX ] ──(3)──► [ EXIT ] ──► [Blockchain Prover]
                     ▲                   │                 │
                     │                   │                 │
REVERSE PATH (Receiving the Receipt):    │                 │
[Client Polls] ──────┴─ [Reply Store] ◄──┴── [REPLY RELAY] ◄┘
```

| # | Role Name | Quantity (Minimal / Scaled) | Primary Function | Wire Ports |
| :---: | :--- | :---: | :--- | :--- |
| **1** | **Gateway** | 1 / Multiple (e.g. 2–10) | **Front Door & Anti-Spam Barrier**.<br>• Verifies Binius64 zero-knowledge membership tickets before admitting packets.<br>• Peels Onion Layer 1.<br>• Hosts the in-memory **Reply Store** where replies wait for clients.<br>• Injects dummy cover traffic when the network is quiet. | • Relay: `47200`<br>• Rate Limiter: `47201`<br>• Reply Store: `47250` |
| **2** | **Mix** | 1 / Multiple (e.g. 2–10) | **Batching Blender & Timing Decorrelator**.<br>• Collects incoming packets over a configurable interval (default: `1.5s`).<br>• Peels Onion Layer 2.<br>• Shuffles the entire batch randomly via CSPRNG to destroy timing correlations before forwarding. | • Relay: `47210` |
| **3** | **Exit** | 1 / Multiple (e.g. 2–10) | **Reassembler & Settlement Queue**.<br>• Peels the final Onion Layer 3.<br>• Reassembles out-of-order chunks into the original multi-kilobyte spend proof.<br>• Dumps transactions into `spend-queue/` for the recursive ZK prover.<br>• Formulates return receipts using the sender's single-use reply block (SURB). | • Relay: `47220` |
| **4** | **Reply Relay** | 1 / Multiple (e.g. 2–10) | **Reverse Path Privacy Shield**.<br>• Prevents the Exit node from communicating directly with the client's Gateway.<br>• Peels the first layer of the return onion and routes the receipt back through the Mix to the Gateway's reply store. | • Relay: `47230` |

---

## 3. How Many Nodes Operate in the Network?

- **Minimal Network (Local / Development)**:
  - Exactly **4 nodes** (1 Gateway, 1 Mix, 1 Exit, 1 Reply Relay).
- **Production Scaled Cluster (Multi-Node Stratified Routing)**:
  - Runs **8, 16, or more nodes** deployed across diverse geographic regions (e.g., 2 Gateways, 2 Mixes, 2 Exits, 2 Reply Relays in US, EU, and Asia).
  - The client dynamically chooses an independent, random 3-hop forward path (`[Gateway_i, Mix_j, Exit_k]`) and an independent random return path (`[ReplyRelay_x, Mix_y, Gateway_z]`) using client-side CSPRNG.

---

## 4. End-to-End System Guarantees

1. **Complete Origin Privacy (IP Unlinkability)**:
   - The Gateway knows the client's IP, but cannot read the transaction payload or see where it terminates.
   - The Mix node only sees the Gateway's IP and the Exit's IP.
   - The Exit node knows the transaction content, but has zero knowledge of who sent it or what IP it came from.
2. **Resistance to Global Passive Adversaries**:
   - An eavesdropper observing every network router simultaneously cannot correlate incoming packets with outgoing packets due to the Mix batching and shuffling.
3. **No Ingress Ports Required for Clients**:
   - Mobile and desktop clients never run listening ports or accept incoming connections. Replies are deposited anonymously into a secret mailbox (`slot`) at the Gateway's reply store, which the client polls over outbound TCP.
4. **Post-Quantum Cryptographic Security**:
   - Every hop is secured by hybrid lattice encapsulation (**X-Wing: ML-KEM-768 + X25519**). Even if an adversary records all traffic today and acquires a quantum supercomputer in the future, the recorded packets cannot be retroactively decrypted.
