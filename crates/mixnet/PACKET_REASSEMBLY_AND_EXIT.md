# Packet Reassembly & Exit Mechanism: Out-of-Order Handling and ZK Spend Queueing

This document explains how large messages (such as 15 KB to 1.3 MB zero-knowledge spend proofs) are split into fixed-size Sphinx packets, travel across the mixnet, and get reassembled out-of-order at the **Exit node** before queueing for recursive zero-knowledge proving.

---

## 1. Why Reassembly is Necessary

In Topi's mixnet, packet size is strictly invariant:
- Every wire packet is **strictly 8,192 bytes**.
- After stripping the 3,504-byte Sphinx onion header, the maximum plaintext fragment capacity is **4,672 bytes**.

Because zero-knowledge transactions range from **15 KB to 1.3 MB**, they cannot fit into one packet. The client fragments the message across multiple Sphinx packets.

```text
[ 15 KB - 1.3 MB ZK Spend ] 
            │
            ▼ Chopped by Client into 4,672-byte Chunks
  [ Fragment 0 ]  [ Fragment 1 ]  [ Fragment 2 ]  [ Fragment 3 ]
```

### The Out-of-Order Delivery Reality
Because packets experience independent exponential delays (`exp_delay_ms`) and batch mixing intervals (`batch_interval = 1.5s`) at the Mix nodes, **packets arrive at the Exit node out of sequence**:

```text
Time t0: Fragment 2 arrives
Time t1: Fragment 0 arrives (contains SURB return header)
Time t2: Fragment 3 arrives
Time t3: Fragment 1 arrives (completes the message!)
```

The Exit node must maintain a robust, timing-leak-free reassembly pipeline that reconstructs the original payload regardless of arrival order.

---

## 2. Fragment Wire Framing (Inside Payload)

Each fragment carries a **14-byte fixed prefix** prepended to its chunk:

```text
┌───────────┬───────────────┬──────────────┬──────────────┬──────────────┬─────────────────┐
│ Kind (u8) │ Message ID    │ Fragment Idx │ Total Count  │ Has SURB     │ Data / SURB     │
│  1 byte   │ 8 bytes (u64) │ 2 bytes (u16)│ 2 bytes (u16)│ 1 byte (u8)  │ Variable        │
└───────────┴───────────────┴──────────────┴──────────────┴──────────────┴─────────────────┘
◄─────────────────────────── Fixed: 14 bytes ─────────────────────────────►
```

- **`Kind` (1 byte)**: `0` = Cover, `2` = Data (Spend), `3` = Ping.
- **`Message ID` (8 bytes, `u64`)**: Random identifier uniquely identifying this message.
- **`Index` (2 bytes, `u16`)**: Zero-based sequence number (`0, 1, 2, ...`).
- **`Count` (2 bytes, `u16`)**: Total number of fragments required.
- **`has_surb` (1 byte)**: `1` or `2` indicates the presence of a 3,504-byte return header (attached only to Fragment 0).

---

## 3. The In-Memory Reassembly Engine (`Partial`)

In [`crates/mixnet/src/exit.rs`](file:///home/vishal/Projects/EGM/crates/mixnet/src/exit.rs), the Exit node maintains an in-memory thread-safe buffer:

```rust
pub struct Exit {
    queue: PathBuf,
    partial: Mutex<HashMap<u64, Partial>>,
}

struct Partial {
    kind: u8,
    parts: Vec<Option<Vec<u8>>>,
    surb: Option<Vec<u8>>,
}
```

- Keyed by `u64` (`Message ID`).
- `parts`: A vector of option slots pre-allocated to the exact size of `count`.

---

## 4. Step-by-Step Packet Ingestion State Machine

When a raw 8,192-byte packet reaches the Exit node (`Exit::accept`):

```text
Incoming 8,192-byte Packet
            │
            ▼
[Decapsulate Sphinx Onion] ──► Recovers ss3
            │
            ▼
[Verify BLAKE3 Integrity Tag] ──► Fails? Drop silently!
            │
            ▼
[Decode Fragment Header] ──► Extracts ID, Index, Count, SURB
            │
            ▼
    Lock `partial` Map
            │
            ├── ID unseen? ──► Allocate: `parts = vec![None; count]`
            │
            ├── Store chunk: `parts[index] = Some(data)`
            │
            ├── Store SURB if present: `p.surb = frag.surb`
            │
            └── Check completion:
                All slots != None  AND  SURB != None ?
                     │
                     ├── NO  ──► Release lock, wait for remaining packets
                     │
                     └── YES ──► Pop from map and REASSEMBLE!
```

### Visual Example of Slot Filling (`Count = 3`):

1. **Fragment 1 arrives first**:
   ```text
   parts: [ None, Some(Chunk 1), None ]
   surb:  None
   Status: Incomplete
   ```
2. **Fragment 0 arrives second** (Index 0 brings the SURB):
   ```text
   parts: [ Some(Chunk 0), Some(Chunk 1), None ]
   surb:  Some(SURB_Header)
   Status: Incomplete
   ```
3. **Fragment 2 arrives last**:
   ```text
   parts: [ Some(Chunk 0), Some(Chunk 1), Some(Chunk 2) ]
   surb:  Some(SURB_Header)
   Status: COMPLETE!
   ```

---

## 5. Post-Reassembly Pipeline

Once all slots are verified full:

### 1. Atomic Extraction & Memory Cleanup
The Exit atomically removes the `Message ID` entry from `self.partial` to free memory and prevent state exhaustion.

### 2. Flattening & Concatenation
The Exit concatenates the chunks in exact index order:
```rust
let message: Vec<u8> = parts.into_iter().flatten().flatten().collect();
```
The original multi-kilobyte plaintext transaction is now fully reconstructed!

### 3. ZK Spend Queueing (`queue_spend`)
If the message begins with the prefix `pq-spend/v1`:
- Reads the 4-byte header length.
- Extracts the JSON header containing the **82 public settlement fields** (`{"fields": [...]}`).
- Generates a unique 64-bit spend ID.
- Writes two files to the local disk queue (`spend-queue/`):
  1. `{id}.tgz`: The raw client zero-knowledge proof archive.
  2. `{id}.json`: The 82 public settle fields.
- Emits an event: `Event::SpendQueued`.

### 4. Answering on the SURB
The Exit generates an execution receipt:
```json
{ "ok": true, "queued": "4a7f9b01c3e8...", "bytes": 15420 }
```
It wraps this receipt using the sender's saved single-use reply block (**SURB**) via `reply_packet(&surb, &response)` and transmits it back through the mixnet. The sender later polls its Gateway reply store to collect the confirmation receipt anonymously.

---

## 6. What Happens to the 1.3 MB Proof After the Exit?

> [!NOTE]
> **The 1.3 MB client proof is NEVER submitted directly to Ethereum!**

Direct on-chain submission of 1.3 MB proofs would exceed Ethereum block gas limits. Instead:
1. The **Exit node** pairs two queued client proofs ($A$ and $B$).
2. The Exit runs the **`prove-exit.sh` recursive prover** (`leanVM`).
3. `leanVM` verifies both client proofs off-chain and compresses them into a single **Terminal Keccak-WHIR Proof** ($\approx 250\text{ KB}$).
4. The smart contract on Ethereum (`ShieldedPool.sol`) only verifies that single compressed terminal proof, reducing on-chain gas costs by over **95%**.
