# Topi

Post-quantum private payments on Ethereum, built on EIP-8141 frame transactions.

A wallet proves a withdrawal from a shielded pool in zero knowledge and sends the proof through a
post-quantum mixnet. An exit folds spends into one proof the chain verifies and settles them in one frame
transaction authorized by hash-based signatures.

| Crate | What it is |
|---|---|
| crates/sphinx | KEM-Sphinx packets: X-Wing per hop, fixed 8 KB, replies |
| crates/ratelimit | anonymous rate limiting with Binius64 epoch tickets |
| crates/mixnet | mixnet nodes and client: 	opi-node, 	opi-client |

Licensed under either of Apache License 2.0 or MIT, at your option.