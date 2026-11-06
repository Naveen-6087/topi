# topi-mixnet

Mixnet nodes and client for carrying private transactions to a chain: KEM-Sphinx routing, a batching
mix, single-use reply blocks, anonymous rate limiting at the gateway.

```text
client ─► gateway ─► mix ─► exit             forward: rate-limited, batched, shuffled
  ▲          ▲        │      │
  └─ reply store ◄────┴── reply relay ◄─┘    reply: on the sender's single-use reply block
```

| Role | Does |
|---|---|
| gateway | admits a packet only with a valid rate-limit token, peels the first layer, holds replies in the reply store, sends cover traffic |
| mix | collects packets for a batch interval, releases each batch shuffled |
| exit | reassembles messages, answers pings, queues pool spends for the exit prover |
| reply relay | first hop of the reply path |

A client never accepts connections: replies wait in the gateway's reply store under a slot only the
sender knows, sealed to the sender's reply keys, and the client polls for them.

## Binaries

```bash
# keys and a topology (hosts in role order: gateway, mix, exit, reply relay)
topi-node keygen keys/gateway.key            # likewise mix.key, exit.key, reply-relay.key
topi-node topology topology.json --members members.txt --keys keys --hosts 127.0.0.1 127.0.0.1 127.0.0.1 127.0.0.1

# one process per node
topi-node run gateway --topology topology.json --key keys/gateway.key --bind 127.0.0.1 --stats-port 47300
topi-node run exit --topology topology.json --key keys/exit.key --spend-queue spend-queue

# a member identity, a round trip, a spend submission
topi-client register --identity member.id --members members.txt
topi-client ping --topology topology.json --identity member.id
topi-client spend --topology topology.json --identity member.id --proof client.tgz --public public.json
```

Each node serves its counters and recent log as JSON on its stats port (`NodeStats`), and Prometheus metrics
over HTTP on its metrics port (default 47400); see [METRICS.md](../../METRICS.md).

## Library

`Client` (connect, ping, submit a spend), `node::run` (a node from a `NodeConfig`), `Topology`
(nodes, addresses, keys, rate-limit membership), `message` (fragment framing), `exit` (the spend
envelope `pq-spend/v1`), `replies` (the reply store), `ratelimit` (the gateway's admission and the
client's tokens), `events` (counters and log).

## Wire formats

- Fragment: `kind u8 | message id u64 | index u16 | count u16 | has_surb u8 | [SURB header] | data`.
- Spend: `pq-spend/v1 | header length u32 BE | {"fields": [82 field elements]} | client proof (.tar.gz)`.
- Reply store: `P | slot u16 | packet` stores, `G | slot u16` fetches and removes.
- Rate limiter: ticket `1 | len u32 | ticket`, packet `2 | token root | len u16 | token | packet`,
  continuation `3 | token value | packet`.

## Tests

```bash
cargo test --release -p topi-mixnet   # includes a four-node network on localhost: ping and spend
```
