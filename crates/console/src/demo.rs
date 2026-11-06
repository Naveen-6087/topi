//! The whole private payment in one go, for demos (Wallet tab, `D`): the accounts it spends from are
//! topped up (crate::funding), then two fresh notes are deposited,
//! witnessed, proven and sent through the mixnet, then the exit prover pairs them and settles on chain
//! (through the alt mempool when it runs, recursing on AWS when EXIT_PROVER=aws).

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::paths::Paths;
use crate::{commands, prover_ops, wallet_ops};

/// Each note's deposit, in wei (0.01 ETH).
const AMOUNT_WEI: u128 = 10_000_000_000_000_000;

pub struct Demo<'a> {
    pub notes_dir: &'a Path,
    pub account_dir: &'a Path,
    pub recipient: &'a str,
    pub topology: &'a str,
    pub queue: &'a str,
    pub mempool: Option<&'a str>,
}

pub fn run(paths: &Paths, d: &Demo, log: &dyn Fn(&str)) -> Result<(), String> {
    let started = Instant::now();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|t| t.as_secs()).unwrap_or(0);
    let notes = [format!("demo{stamp}a"), format!("demo{stamp}b")];
    let step = |what: &str| log(&format!("[{:>4}s] {what}", started.elapsed().as_secs()));

    step("funding: the depositing key and the paymaster");
    crate::funding::ensure_funded(paths, log)?;

    for n in &notes {
        step(&format!("note {n}: new note and deposit"));
        wallet_ops::new_note(paths, n, d.notes_dir, log)?;
        wallet_ops::deposit(paths, n, AMOUNT_WEI, d.notes_dir, Some(d.account_dir), log)?;
    }
    for n in &notes {
        step(&format!("note {n}: witness and client proof"));
        wallet_ops::witness(paths, n, d.recipient, d.notes_dir, log)?;
        wallet_ops::prove_note(paths, n, d.notes_dir, log)?;
        step(&format!("note {n}: sending the proof through the mixnet"));
        send(paths, n, d, log)?;
    }
    step("exit prover: pairing the two spends, recursion, settle");
    prover_ops::run_exit_prover(paths, d.queue, d.mempool, log)?;
    step(&format!("demo done in {} s", started.elapsed().as_secs()));
    Ok(())
}

/// `topi-client spend` through the network; retried while a just-started network warms up.
fn send(paths: &Paths, note: &str, d: &Demo, log: &dyn Fn(&str)) -> Result<(), String> {
    let spec = commands::send(paths, note, Some(d.topology), Some(d.notes_dir));
    let mut last = String::new();
    for attempt in 1..=3 {
        let out = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .output()
            .map_err(|e| format!("cannot run topi-client: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if out.status.success() && stdout.contains("\"ok\":true") {
            log(&format!("queued at the exit: {}", stdout.trim()));
            return Ok(());
        }
        last = format!("{} {}", stdout.trim(), String::from_utf8_lossy(&out.stderr).trim());
        log(&format!("send attempt {attempt} failed: {last}"));
        std::thread::sleep(Duration::from_secs(5));
    }
    Err(format!("could not send {note} through the mixnet: {last}"))
}
