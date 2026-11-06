# topi-sphinx

KEM-Sphinx: fixed-size onion packets with a post-quantum key encapsulation per hop.

Each hop gets its own X-Wing ciphertext (ML-KEM-768 combined with X25519), so the routing layers stay
confidential against a quantum adversary while keeping classical security. The header is built with
the Sphinx filler technique, so every hop sees a header of the same size and every packet is exactly
8192 bytes, cover traffic included.

| Item | Value |
|---|---|
| Hops | 3 |
| Packet | 8192 bytes |
| Per-hop ciphertext | 1120 bytes (X-Wing) |
| Per-hop MAC | 16 bytes (BLAKE3 keyed hash) |
| Header | 3,504 bytes |
| Payload | 4,688 bytes, with an end-to-end integrity tag |
| Replay protection | per-hop tag set |

## API

- `keygen`, `keypair_from_seed`, `sk_bytes`, `pk_bytes`, `pk_from_bytes`: node keys.
- `build_packet(route, routing, message)`: a forward packet over three hops.
- `process(sk, packet, seen)`: one hop: authenticate, peel a layer, read the routing for the next hop.
- `build_surb`, `reply_packet`, `open_reply`: single-use reply blocks, built by the sender, used by the
  last hop to answer, opened by the sender.
- `cover_packet`: an empty packet indistinguishable from a real one until the last hop.

## Simplifications

- Layer encryption uses the BLAKE3 XOF as a stream cipher.
- The payload uses a plain stream layer per hop rather than a wide-block cipher (Lioness); integrity is
  checked end to end with a tag at the final hop.
- The replay set grows with the node's lifetime; key rotation bounds it in a production deployment.

## Tests and benchmarks

```bash
cargo test --release -p topi-sphinx
cargo run --release -p topi-sphinx --example bench
```

Building a packet takes about 0.4 ms and processing a hop about 0.18 ms on a laptop core.
