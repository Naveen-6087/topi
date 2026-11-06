# Exact Packet State at Every Hop in the Mixnet

This guide presents the **exact byte-by-byte visual layout** of a Topi mixnet packet as it travels across each hop. 

At **every stage**, the packet length is strictly invariant: **exactly 8,192 bytes (65,536 bits)**.

---

## High-Level Size Overview

```text
◄────────────────────────────── Total: 8,192 bytes ────────────────────────────────────────►
┌────────────────────────────────────────────────────────┬─────────────────────────────────┐
│                 Header (3,504 bytes)                   │      Payload (4,688 bytes)      │
└────────────────────────────────────────────────────────┴─────────────────────────────────┘
```

- **Header Size**: `3 hops × 1,168 bytes per layer = 3,504 bytes`.
- **Payload Size**: `8,192 - 3,504 = 4,688 bytes`.

---

## State 1: Leaving Client ➔ Arriving at Gateway (Hop 1)

The Client generated 3 shared secrets ($ss_1, ss_2, ss_3$) and 3 capsules ($C_1, C_2, C_3$).
Layer 1 is wrapped around Layer 2, which is wrapped around Layer 3.

```text
◄────────────────────────────── Total: 8,192 bytes ────────────────────────────────────────►
┌────────────────────────────────────────────────────────┬─────────────────────────────────┐
│                 Header (3,504 bytes)                   │      Payload (4,688 bytes)      │
├──────────────┬──────────┬──────────────────────────────┼─────────────────────────────────┤
│  Capsule 1   │  MAC 1   │ Encrypted under ss1:         │ Encrypted under:                │
│ (1,120 bytes)│ (16 bytes│  • Routing 1: "Go to Mix"    │  ss1 ⊕ ss2 ⊕ ss3                │
│              │          │  • Capsule 2 (1,120 bytes)   │                                 │
│              │          │  • MAC 2 (16 bytes)          │ [Plaintext transaction data     │
│              │          │  • Encrypted under ss2:      │  locked under 3 layers of       │
│              │          │    [Routing 2, Capsule 3...] │  stream cipher encryption]      │
│              │          │  • Filler Padding 1          │                                 │
└──────────────┴──────────┴──────────────────────────────┴─────────────────────────────────┘
```

### What Gateway (Hop 1) does:
1. **Reads Capsule 1 (1,120 B)** from the front.
2. Runs `decapsulate(SK_gateway, Capsule 1)` $\to$ calculates **$ss_1$**.
3. Verifies **`MAC 1`** using the key derived from $ss_1$.
4. Decrypts with $ss_1$:
   - Reads `Routing 1`: *"Wait 10ms, then forward to Mix node IP"*.
   - **Capsule 2 and MAC 2 are now unlocked!**
5. Strips `Capsule 1` and `MAC 1`, shifts the data left by 1,136 bytes, and appends 1,136 bytes of pseudo-random filler padding generated via BLAKE3 XOF.
6. Decrypts one layer off the payload using $ss_1$.

---

## State 2: Leaving Gateway ➔ Arriving at Mix (Hop 2)

Notice: **Capsule 1 is gone forever.** Capsule 2 is now sitting openly at the front.

```text
◄────────────────────────────── Total: 8,192 bytes ────────────────────────────────────────►
┌────────────────────────────────────────────────────────┬─────────────────────────────────┐
│                 Header (3,504 bytes)                   │      Payload (4,688 bytes)      │
├──────────────┬──────────┬──────────────────────────────┼─────────────────────────────────┤
│  Capsule 2   │  MAC 2   │ Encrypted under ss2:         │ Encrypted under:                │
│ (1,120 bytes)│ (16 bytes│  • Routing 2: "Go to Exit"   │  ss2 ⊕ ss3                      │
│              │          │  • Capsule 3 (1,120 bytes)   │                                 │
│              │          │  • MAC 3 (16 bytes)          │ [1 layer peeled; still locked   │
│              │          │  • Routing 3: "Deliver"      │  under 2 layers of stream       │
│              │          │  • Filler Padding 2          │  cipher encryption]             │
└──────────────┴──────────┴──────────────────────────────┴─────────────────────────────────┘
```

### What Mix (Hop 2) does:
1. **Privacy barrier**: Mix does NOT know who the client is (it only sees Gateway's IP).
2. **Reads Capsule 2 (1,120 B)** from the front.
3. Runs `decapsulate(SK_mix, Capsule 2)` $\to$ calculates **$ss_2$**.
4. Verifies **`MAC 2`** using the key derived from $ss_2$.
5. Decrypts with $ss_2$:
   - Reads `Routing 2`: *"Hold for batch interval (1.5s), shuffle, then forward to Exit IP"*.
   - **Capsule 3 and MAC 3 are now unlocked!**
6. Strips `Capsule 2` and `MAC 2`, shifts left, and appends filler padding.
7. Decrypts the second layer off the payload using $ss_2$.
8. **Shuffles the packet** inside a batch with other users' packets to destroy timing correlation.

---

## State 3: Leaving Mix ➔ Arriving at Exit (Hop 3)

Notice: **Capsule 1 and Capsule 2 are gone.** Capsule 3 is now sitting at the front.

```text
◄────────────────────────────── Total: 8,192 bytes ────────────────────────────────────────►
┌────────────────────────────────────────────────────────┬─────────────────────────────────┐
│                 Header (3,504 bytes)                   │      Payload (4,688 bytes)      │
├──────────────┬──────────┬──────────────────────────────┼─────────────────────────────────┤
│  Capsule 3   │  MAC 3   │ Encrypted under ss3:         │ Encrypted under:                │
│ (1,120 bytes)│ (16 bytes│  • Routing 3:                │  ss3                            │
│              │          │    "is_exit: true            │                                 │
│              │          │     Deliver Payload!"        │ [Final layer of stream          │
│              │          │  • Filler Padding 3          │  cipher encryption]             │
└──────────────┴──────────┴──────────────────────────────┴─────────────────────────────────┘
```

### What Exit (Hop 3) does:
1. **Reads Capsule 3 (1,120 B)** from the front.
2. Runs `decapsulate(SK_exit, Capsule 3)` $\to$ calculates **$ss_3$**.
3. Verifies **`MAC 3`** using the key derived from $ss_3$.
4. Decrypts with $ss_3$:
   - Reads `Routing 3`: `is_exit: true` (*"This packet terminates here"*).
5. Discards the entire 3,504-byte header.
6. Decrypts the final layer off the payload using $ss_3$.

---

## State 4: Unpacked Payload at the Exit Node

Once the final layer of encryption ($ss_3$) is peeled, the raw 4,688-byte payload is revealed:

```text
┌────────────────────────┬─────────────────────────────────────────────────────────────────┐
│  BLAKE3 Tag (MAC)      │                  Plaintext Fragment                             │
│       16 bytes         │                     4,672 bytes                                 │
└────────────────────────┴─────────────────────────────────────────────────────────────────┘
```

### Inner Fragment Breakdown:
```text
┌───────────┬───────────────┬──────────────┬──────────────┬──────────────┬─────────────────┐
│ Kind (u8) │ Message ID    │ Fragment Idx │ Total Count  │ Has SURB     │ Data / SURB     │
│  1 byte   │ 8 bytes (u64) │ 2 bytes (u16)│ 2 bytes (u16)│ 1 byte (u8)  │ 4,658 bytes     │
└───────────┴───────────────┴──────────────┴──────────────┴──────────────┴─────────────────┘
```

1. **Verify Integrity**: Exit checks the 16-byte BLAKE3 Tag against the plaintext fragment.
2. **Reassemble**: Collects fragments by `Message ID` and reassembles out-of-order pieces by `Fragment Idx`.
3. **Queue**: Submits the complete private transaction proof to the zero-knowledge prover queue (`spend-queue`).
4. **Answer**: Extracts the return **SURB** and routes the confirmation receipt back through the reply path!

---

## Summary Comparison Matrix

| Hop | Node | What's at Packet Byte `0..1120`? | Secret Derived | Routing Learned | Payload Encrypted By |
| :---: | :---: | :---: | :---: | :---: | :---: |
| **1** | **Gateway** | `Capsule 1` | $ss_1$ | *"Forward to Mix"* | $ss_1 \oplus ss_2 \oplus ss_3$ |
| **2** | **Mix** | `Capsule 2` | $ss_2$ | *"Shuffle & Forward to Exit"* | $ss_2 \oplus ss_3$ |
| **3** | **Exit** | `Capsule 3` | $ss_3$ | *"is_exit: true (Deliver)"* | $ss_3$ |
| **End** | **Prover** | *(Header Discarded)* | — | Transaction Queued | **Plaintext Extracted** |
