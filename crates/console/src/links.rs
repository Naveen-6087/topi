//! Where the project lives on the web, and opening those pages in the system browser.

use std::process::{Command, Stdio};

pub const SITE: &str = "https://topi.topi-website.workers.dev";
pub const REPO: &str = "https://github.com/Naveen-6087/topi";
/// The chain 8141 block explorer (Dora).
pub const EXPLORER: &str = "https://dora.privacy.ethrex.xyz";
/// The ERC-8421 alt mempool explorer: admission reports of any sidecar it is pointed at.
pub const MEMPOOL_EXPLORER: &str = "https://explorer.altmempool.wtf";

/// The block explorer of a chain: Dora for 8141, the alt mempool explorer for the ERC-8421 devnet (1337).
pub fn explorer_for(chain_id: u64) -> String {
    match chain_id {
        1337 => format!("{MEMPOOL_EXPLORER}/?node=https://altmempool.forshtat.com/rpc"),
        _ => EXPLORER.to_string(),
    }
}

/// The alt mempool explorer pointed at a sidecar's JSON-RPC (its admissions, rule by rule).
pub fn mempool_explorer(node_rpc: &str) -> String {
    format!("{MEMPOOL_EXPLORER}/?node={node_rpc}")
}

/// The address without its scheme, for display.
pub fn short(url: &str) -> &str {
    url.trim_start_matches("https://")
}

/// Opens `url` in the default browser, without waiting for it.
pub fn open(url: &str) -> std::io::Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ())
}
