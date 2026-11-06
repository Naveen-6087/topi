//! What a node reports while it runs, and the counters it serves to monitoring.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;

/// Why a node dropped a packet or message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Not a packet of the fixed size.
    BadSize,
    /// The hop's MAC did not verify: forged, corrupted, or not for this node.
    BadMac,
    /// A packet this node has already processed.
    Replay,
    /// At the gateway: no valid rate-limit token.
    InvalidToken,
    /// At the gateway: a continuation without remaining budget.
    NoBudget,
    /// At the exit: the payload's end-to-end integrity tag did not verify.
    Integrity,
}

impl Reject {
    /// The metric label.
    pub fn label(self) -> &'static str {
        match self {
            Reject::BadSize => "bad_size",
            Reject::BadMac => "bad_mac",
            Reject::Replay => "replay",
            Reject::InvalidToken => "invalid_token",
            Reject::NoBudget => "no_continuation_budget",
            Reject::Integrity => "integrity",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Reject::BadSize => "packet of the wrong size",
            Reject::BadMac => "header MAC did not verify",
            Reject::Replay => "replayed packet",
            Reject::InvalidToken => "packet without a valid rate-limit token",
            Reject::NoBudget => "continuation without budget",
            Reject::Integrity => "payload failed its integrity check",
        }
    }
}

impl From<topi_sphinx::HopError> for Reject {
    fn from(e: topi_sphinx::HopError) -> Reject {
        match e {
            topi_sphinx::HopError::BadSize => Reject::BadSize,
            topi_sphinx::HopError::BadMac => Reject::BadMac,
            topi_sphinx::HopError::Replay => Reject::Replay,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Received,
    Forwarded,
    /// The mix released a batch of this many packets.
    Batch(usize),
    /// A cover packet reached the exit.
    Cover,
    /// A pool spend was queued for the prover.
    SpendQueued(String),
    /// An epoch ticket was verified (true) or refused (false), with its size in bytes.
    Ticket(bool, usize),
    Rejected(Reject),
    Info(String),
}

pub type Events = UnboundedSender<Event>;

const LOG_LINES: usize = 40;

/// A node's counters and recent log, served as JSON on its stats port and as Prometheus metrics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeStats {
    pub role: String,
    pub started: u64,
    pub received: u64,
    pub forwarded: u64,
    pub batches: u64,
    pub last_batch: usize,
    /// Packets released in all batches together (with `batches`, the mean batch size).
    #[serde(default)]
    pub batched: u64,
    pub cover: u64,
    pub spends: u64,
    pub rejected: u64,
    #[serde(default)]
    pub rejected_by: BTreeMap<String, u64>,
    #[serde(default)]
    pub tickets_verified: u64,
    #[serde(default)]
    pub tickets_refused: u64,
    /// Newest first, each line `unix-seconds message`.
    pub log: VecDeque<String>,
}

impl NodeStats {
    pub fn new(role: &str) -> NodeStats {
        NodeStats { role: role.into(), started: now_secs(), ..Default::default() }
    }

    /// Count the event; return a log line when it is worth one.
    pub fn record(&mut self, e: Event) -> Option<String> {
        let line = match e {
            Event::Received => {
                self.received += 1;
                None
            }
            Event::Forwarded => {
                self.forwarded += 1;
                None
            }
            Event::Batch(n) => {
                self.batches += 1;
                self.batched += n as u64;
                self.last_batch = n;
                Some(format!("batch of {n} packets released, shuffled"))
            }
            Event::Cover => {
                self.cover += 1;
                None
            }
            Event::SpendQueued(m) => {
                self.spends += 1;
                Some(m)
            }
            Event::Ticket(ok, bytes) => {
                if ok {
                    self.tickets_verified += 1;
                } else {
                    self.tickets_refused += 1;
                }
                Some(format!("epoch ticket {} ({} KB zero-knowledge proof)", if ok { "verified" } else { "refused" }, bytes / 1024))
            }
            Event::Rejected(r) => {
                self.rejected += 1;
                *self.rejected_by.entry(r.label().into()).or_default() += 1;
                Some(format!("rejected: {}", r.describe()))
            }
            Event::Info(m) => Some(m),
        };
        if let Some(l) = &line {
            self.log.push_front(format!("{} {l}", now_secs()));
            self.log.truncate(LOG_LINES);
        }
        line
    }
}

/// Serve `stats` as one JSON document per connection.
pub async fn serve_stats(port: u16, stats: Arc<Mutex<NodeStats>>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    loop {
        let Ok((mut s, _)) = listener.accept().await else { continue };
        let body = serde_json::to_vec(&*stats.lock().unwrap()).expect("stats serialise");
        let _ = s.write_all(&body).await;
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
