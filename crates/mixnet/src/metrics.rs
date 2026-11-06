//! Prometheus metrics: a node's counters in the text exposition format, over plain HTTP.
//! Every metric is described in METRICS.md at the repository root.

use std::fmt::Write;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::events::{NodeStats, now_secs};

pub const METRICS_PORT: u16 = 47400;

/// The node's metrics in the Prometheus text format (version 0.0.4).
pub fn render(s: &NodeStats) -> String {
    let role = &s.role;
    let mut out = String::new();
    let mut metric = |name: &str, kind: &str, help: &str, samples: &[(String, f64)]| {
        let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
        for (labels, value) in samples {
            let _ = writeln!(out, "{name}{{role=\"{role}\"{labels}}} {value}");
        }
    };
    let one = |v: u64| vec![(String::new(), v as f64)];
    metric("mixnet_packets_received_total", "counter", "Packets received by this node.", &one(s.received));
    metric("mixnet_packets_forwarded_total", "counter", "Packets this node sent on to the next hop or the reply store.", &one(s.forwarded));
    metric("mixnet_batches_total", "counter", "Batches the mix released.", &one(s.batches));
    metric(
        "mixnet_batched_packets_total",
        "counter",
        "Packets released in batches; divided by batches, the mean batch size.",
        &one(s.batched),
    );
    metric("mixnet_last_batch_size", "gauge", "Packets in the most recent batch.", &one(s.last_batch as u64));
    metric("mixnet_cover_packets_total", "counter", "Cover packets that reached the exit.", &one(s.cover));
    metric("mixnet_spends_queued_total", "counter", "Pool spends the exit queued for the prover.", &one(s.spends));
    let tickets =
        vec![(",result=\"verified\"".into(), s.tickets_verified as f64), (",result=\"refused\"".into(), s.tickets_refused as f64)];
    metric("mixnet_epoch_tickets_total", "counter", "Rate-limit epoch tickets the gateway checked.", &tickets);
    let rejected: Vec<(String, f64)> = s.rejected_by.iter().map(|(reason, n)| (format!(",reason=\"{reason}\""), *n as f64)).collect();
    metric("mixnet_rejected_total", "counter", "Packets or messages dropped, by reason.", &rejected);
    metric("mixnet_uptime_seconds", "gauge", "Seconds since the node started.", &one(now_secs().saturating_sub(s.started)));
    out
}

/// Answer every HTTP request with the current metrics.
pub async fn serve(port: u16, stats: Arc<Mutex<NodeStats>>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    loop {
        let Ok((mut s, _)) = listener.accept().await else { continue };
        let stats = stats.clone();
        tokio::spawn(async move {
            let mut request = [0u8; 1024];
            let _ = s.read(&mut request).await;
            let body = render(&stats.lock().unwrap());
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes()).await;
            let _ = s.write_all(body.as_bytes()).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, Reject};

    #[test]
    fn renders_counters_with_labels() {
        let mut s = NodeStats::new("mix");
        s.record(Event::Received);
        s.record(Event::Batch(5));
        s.record(Event::Rejected(Reject::Replay));
        s.record(Event::Rejected(Reject::Replay));
        let text = render(&s);
        assert!(text.contains("mixnet_packets_received_total{role=\"mix\"} 1"));
        assert!(text.contains("mixnet_batched_packets_total{role=\"mix\"} 5"));
        assert!(text.contains("mixnet_rejected_total{role=\"mix\",reason=\"replay\"} 2"));
        assert!(text.contains("# TYPE mixnet_batches_total counter"));
    }
}
