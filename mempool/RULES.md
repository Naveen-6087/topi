# topi privacy-pool mempool

An ERC-8421 alternative mempool for the topi shielded pool's settle transactions on EIP-8141 chains.
This document defines it; its mempool id is this file's IPFS CIDv0 (`node mempool/src/mempool-id.ts`).

## Why it exists

A settle cannot enter the public mempool:

- every settle has the same sender (the pool) and the same paymaster, while the public mempool keeps
  one pending transaction per sender and per non-canonical paymaster;
- its validation prefix verifies a 5,684-byte leanSPHINCS post-quantum witness, which with the pool's
  own check needs more than the public 100,000 validation gas.

## Transactions it admits

A settle of a pool this mempool serves:

1. Three frames: VERIFY the pool with execution approval, VERIFY an accepted paymaster with payment
   approval, then one SENDER call to the pool's `settle` (selector `0xf4b589c7`). No value, no blobs,
   the pool as sender, a fresh EIP-8250 nonce key at sequence 0.
2. One signature entry: ARBITRARY, empty msg, 5,684 bytes (a leanSPHINCS witness over the signature
   hash), which verifies under the exit's leanSPHINCS key on the shared verifier.
3. Validation budget: frame 0 plus frame 1 execution limits plus signature cost at most 200,000.
4. Both spends name this pool and scope and pay their fee context to the transaction's own fee
   recipient; their two nullifiers differ, are unspent on chain and not claimed by another pending
   settle; both pool roots and both association roots are known to the pool.
5. The pool and every accepted paymaster still run the code this node recorded at start.
6. The node's simulation (`ethrex_simulateFrameTransaction`) approves the validation prefix.

## Entities

The pool (sender) and the accepted paymasters are whitelisted entities: the rules above replace the
ERC-7562 opcode and storage rules for them. A node that can trace validation (ethrex's
eip8141-tracer) still applies those rules to every other entity; a node that cannot admits only
whitelisted entities.

## Pending set

At most 256 pending settles, 64 per paymaster; a pending settle expires after 30 minutes. Every new
block re-checks the pending settles, so one whose spend settled through another route drops out.
Settles share one sender, so they reach the chain one after another, each with its own nonce key.
