# Metrics

Every `topi-node` serves Prometheus metrics over HTTP on `--metrics-port` (default 47400) in the text
exposition format, and the same counters as JSON on `--stats-port` (default 47300) for the console. Every
sample carries a `role` label: `gateway`, `mix`, `exit` or `reply-relay`.

| Metric | Type | Labels | Meaning |
|---|---|---|---|
| `mixnet_packets_received_total` | counter | `role` | Packets received by the node |
| `mixnet_packets_forwarded_total` | counter | `role` | Packets sent on to the next hop, or into the reply store |
| `mixnet_batches_total` | counter | `role` | Batches the mix released |
| `mixnet_batched_packets_total` | counter | `role` | Packets released in batches |
| `mixnet_last_batch_size` | gauge | `role` | Packets in the most recent batch |
| `mixnet_cover_packets_total` | counter | `role` | Cover packets that reached the exit |
| `mixnet_spends_queued_total` | counter | `role` | Pool spends the exit queued for the prover |
| `mixnet_epoch_tickets_total` | counter | `role`, `result` (`verified`, `refused`) | Rate-limit epoch tickets the gateway checked |
| `mixnet_rejected_total` | counter | `role`, `reason` | Packets or messages dropped, by reason (below) |
| `mixnet_uptime_seconds` | gauge | `role` | Seconds since the node started |

Rejection reasons: `bad_size` (not a fixed-size packet), `bad_mac` (forged, corrupted or not for this
node), `replay` (already processed), `invalid_token` (no valid rate-limit token), `no_continuation_budget`
(a message's packets beyond its token's budget), `integrity` (the payload's end-to-end tag failed at the
exit).

## Useful queries

```promql
# mean batch size at the mix: the anonymity set each released packet hides in
rate(mixnet_batched_packets_total{role="mix"}[5m]) / rate(mixnet_batches_total{role="mix"}[5m])

# share of exit traffic that is cover
rate(mixnet_cover_packets_total{role="exit"}[5m]) / rate(mixnet_packets_received_total{role="exit"}[5m])

# replays and forgeries, per node
sum by (role, reason) (rate(mixnet_rejected_total{reason=~"replay|bad_mac"}[5m]))
```

## Scraping

```yaml
scrape_configs:
  - job_name: mixnet
    static_configs:
      - targets: ["GATEWAY:47400", "MIX:47400", "EXIT:47400", "REPLY_RELAY:47400"]
```

On AWS the metrics and stats ports accept only `OPERATOR_CIDR` (see `infra/aws`).
