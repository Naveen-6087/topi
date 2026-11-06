//! The console's whole wallet flow on chain 8141, exactly as its keys run it: two fresh notes are made,
//! deposited, witnessed, proven, sent through a running mixnet into the exit's queue, then the exit
//! prover pairs them, recurses and settles, optionally through the alternative mempool.
//!
//! It spends testnet ETH (two deposits from the deployer key) and takes about 25 minutes, so it runs
//! only when asked:
//!
//!   TOPI_LIVE=1 [TOPI_TOPOLOGY=data/local/topology.json] [MEMPOOL_URL=http://127.0.0.1:47500] \
//!     cargo test --release -p topi-tui --test live_wallet -- --nocapture
//!
//! Needs the prover installed (bash prover/setup.sh), a mixnet whose exit queues into
//! data/local/spend-queue, and `cargo build --release` so topi-client is in target/release.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use topi_tui::paths::Paths;
use topi_tui::{commands, prover_ops, wallet_ops};

const AMOUNT_WEI: u128 = 10_000_000_000_000_000;

#[test]
fn notes_deposit_prove_send_and_settle() {
    if std::env::var("TOPI_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set TOPI_LIVE=1 (spends testnet ETH, about 25 minutes)");
        return;
    }
    let mut paths = Paths::discover();
    paths.bin = paths.root.join("target/release");
    let log = |line: &str| eprintln!("  {line}");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let notes_dir = paths.p(&format!("data/wallet/live-{stamp}"));
    let topology = std::env::var("TOPI_TOPOLOGY").unwrap_or_else(|_| paths.s("data/local/topology.json"));
    let pool = topi_tui::app::read_json(&paths.pool_deployment());
    let recipient = topi_tui::app::read_json(&paths.p("deployments/8141/account.json"))["account"]
        .as_str()
        .unwrap_or(pool["operator"].as_str().unwrap())
        .to_string();

    for name in ["a", "b"] {
        eprintln!("note {name}");
        wallet_ops::new_note(&paths, name, &notes_dir, &log).expect("new note");
        wallet_ops::deposit(&paths, name, AMOUNT_WEI, &notes_dir, None, &log).expect("deposit");
    }
    for name in ["a", "b"] {
        wallet_ops::witness(&paths, name, &recipient, &notes_dir, &log).expect("witness");
        wallet_ops::prove_note(&paths, name, &notes_dir, &log).expect("prove");
        let spec = commands::send(&paths, name, Some(&topology), Some(&notes_dir));
        let out = Command::new(&spec.program).args(&spec.args).current_dir(&spec.cwd).output().expect("topi-client");
        let stdout = String::from_utf8_lossy(&out.stdout);
        eprintln!("  send {name}: {}", stdout.trim());
        assert!(out.status.success() && stdout.contains("\"ok\":true"), "send failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    let mempool = std::env::var("MEMPOOL_URL").ok();
    let queue = paths.s("data/local/spend-queue");
    prover_ops::run_exit_prover(&paths, &queue, mempool.as_deref(), &log).expect("exit prover");
}

/// Only the exit side: pair what is queued in data/local/spend-queue, recurse and settle.
///
///   TOPI_LIVE_SETTLE=1 [MEMPOOL_URL=...] cargo test --release -p topi-tui --test live_wallet settle_queued -- --nocapture
#[test]
fn settle_queued() {
    if std::env::var("TOPI_LIVE_SETTLE").as_deref() != Ok("1") {
        eprintln!("skipped: set TOPI_LIVE_SETTLE=1 (about 20 minutes of recursion, then a settle on chain 8141)");
        return;
    }
    let paths = Paths::discover();
    let log = |line: &str| eprintln!("  {line}");
    let mempool = std::env::var("MEMPOOL_URL").ok();
    prover_ops::run_exit_prover(&paths, &paths.s("data/local/spend-queue"), mempool.as_deref(), &log).expect("exit prover");
}

/// The console's `D` (crate::demo) as the key runs it, against a running local mixnet.
///
///   TOPI_LIVE_DEMO=1 [EXIT_PROVER=aws] [MEMPOOL_URL=...] cargo test --release -p topi-tui --test live_wallet full_demo -- --nocapture
#[test]
fn full_demo() {
    if std::env::var("TOPI_LIVE_DEMO").as_deref() != Ok("1") {
        eprintln!("skipped: set TOPI_LIVE_DEMO=1 (spends testnet ETH; needs the local mixnet up)");
        return;
    }
    let mut paths = Paths::discover();
    paths.bin = paths.root.join("target/release");
    let log = |line: &str| eprintln!("  {line}");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let notes_dir = paths.p(&format!("data/wallet/demo-run-{stamp}"));
    let recipient = topi_tui::app::read_json(&paths.p("deployments/8141/account.json"))["account"].as_str().unwrap().to_string();
    let mempool = std::env::var("MEMPOOL_URL").ok();
    let d = topi_tui::demo::Demo {
        notes_dir: &notes_dir,
        account_dir: &notes_dir,
        recipient: &recipient,
        topology: &paths.s("data/local/topology.json"),
        queue: &paths.s("data/local/spend-queue"),
        mempool: mempool.as_deref(),
    };
    topi_tui::demo::run(&paths, &d, &log).expect("demo");
}

/// The console's `F` (crate::funding): tops up the depositing key and the paymaster.
///
///   TOPI_LIVE_FUND=1 cargo test --release -p topi-tui --test live_wallet fund_accounts -- --nocapture
#[test]
fn fund_accounts() {
    if std::env::var("TOPI_LIVE_FUND").as_deref() != Ok("1") {
        eprintln!("skipped: set TOPI_LIVE_FUND=1 (moves testnet ETH between our accounts)");
        return;
    }
    let paths = Paths::discover();
    topi_tui::funding::ensure_funded(&paths, &|line: &str| eprintln!("  {line}")).expect("funding");
}
