//! Self-contained mixnet stats and rate-limiting identity definitions for topi-console,
//! removing dependencies on internal workspace path crates so topi can be published to crates.io.

use serde::{Deserialize, Serialize};
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A node's counters and recent log, served as JSON on its stats port.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeStats {
    pub role: String,
    pub started: u64,
    pub received: u64,
    pub forwarded: u64,
    pub batches: u64,
    pub last_batch: usize,
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
    pub log: VecDeque<String>,
}

/// Create a member identity at `path` and return its commitment [u8; 32].
pub fn new_identity(path: impl AsRef<Path>) -> Result<[u8; 32], String> {
    let mut sk = [0u8; 32];
    getrandom::fill(&mut sk).map_err(|e| format!("getrandom failed: {e}"))?;
    std::fs::write(path, hex::encode(sk)).map_err(|e| e.to_string())?;

    // Commitment is H("TPID" || sk)
    let mut h = Sha256::new();
    h.update(b"TPID");
    h.update(sk);
    Ok(h.finalize().into())
}
