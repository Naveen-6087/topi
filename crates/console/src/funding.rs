//! Keeps the accounts a demo spends from funded (Wallet tab, `F`, and the first step of `D`).
//!
//! The money circulates: a deposit moves ETH from the depositing key into the pool, and a private
//! withdrawal pays it out to the note's recipient (the PQ account, or a console account). So funding is
//! mostly moving it back. Targets: the depositing key (`.secrets/deployer.key`) and the pool's
//! paymaster each hold at least TARGET. Sources, in order: console accounts (ECDSA keys under the
//! accounts dir), the leanSPHINCS PQ account (a frame transaction it signs with `lsk`), and the deployer
//! itself for the paymaster. Each source keeps KEEP for its own gas.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::crypto::{self, Eip1559Tx};
use crate::paths::Paths;
use crate::wallet_ops::rpc_call;

/// What the depositing key and the paymaster should each hold: two 0.01 ETH deposits plus gas, or
/// two settles' maximum cost.
pub const TARGET: u128 = 40_000_000_000_000_000;
/// What a source keeps for its own gas.
const KEEP: u128 = 5_000_000_000_000_000;
const ETH: f64 = 1e18;

struct Ctx<'a> {
    paths: &'a Paths,
    rpc: String,
    chain_id: u64,
    log: &'a dyn Fn(&str),
}

/// A key that can pay: an ECDSA key, or the PQ account (leanSPHINCS).
enum Source {
    Ecdsa { name: String, key: [u8; 32], addr: String },
    Pq { addr: String, verify_selector: String, key_file: std::path::PathBuf },
}

impl Source {
    fn addr(&self) -> &str {
        match self {
            Source::Ecdsa { addr, .. } | Source::Pq { addr, .. } => addr,
        }
    }
    fn name(&self) -> String {
        match self {
            Source::Ecdsa { name, .. } => name.clone(),
            Source::Pq { .. } => "PQ account".into(),
        }
    }
}

/// Tops up the depositing key and the paymaster to TARGET from the other accounts. Logs every move.
pub fn ensure_funded(paths: &Paths, log: &dyn Fn(&str)) -> Result<(), String> {
    let pool: Value =
        serde_json::from_str(&fs::read_to_string(paths.pool_deployment()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let ctx = Ctx {
        paths,
        rpc: paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz"),
        chain_id: pool["chainId"].as_u64().unwrap_or(8141),
        log,
    };
    let deployer =
        ecdsa_from_file(&paths.p(".secrets/deployer.key"), "deployer").ok_or("no .secrets/deployer.key: the key deposits are paid from")?;
    let paymaster = pool["paymaster"].as_str().ok_or("deployment has no paymaster")?.to_string();

    let mut sources: Vec<Source> = console_accounts(paths);
    if let Some(pq) = pq_account(paths) {
        sources.push(pq);
    }

    // 1. The depositing key, from the accounts the withdrawals paid.
    top_up(&ctx, deployer.addr(), "the depositing key", &sources)?;
    // 2. The paymaster, from the same accounts, then the depositing key's surplus.
    sources.push(deployer);
    top_up(&ctx, &paymaster, "the paymaster", &sources)?;
    Ok(())
}

fn top_up(ctx: &Ctx, target: &str, what: &str, sources: &[Source]) -> Result<(), String> {
    let mut have = balance(ctx, target)?;
    if have >= TARGET {
        (ctx.log)(&format!("{what} {target} holds {:.4} ETH: enough", have as f64 / ETH));
        return Ok(());
    }
    for src in sources {
        if src.addr().eq_ignore_ascii_case(target) {
            continue;
        }
        let need = TARGET - have;
        let spare = balance(ctx, src.addr())?
            .saturating_sub(KEEP + if matches!(src, Source::Ecdsa { name, .. } if name == "deployer") { TARGET } else { 0 });
        if spare < 1_000_000_000_000_000 {
            continue;
        }
        let amount = need.min(spare);
        (ctx.log)(&format!("funding {what} with {:.4} ETH from {} {}", amount as f64 / ETH, src.name(), src.addr()));
        send(ctx, src, target, amount)?;
        have = balance(ctx, target)?;
        if have >= TARGET {
            (ctx.log)(&format!("{what} now holds {:.4} ETH", have as f64 / ETH));
            return Ok(());
        }
    }
    if have < TARGET {
        return Err(format!(
            "{what} {target} holds {:.4} ETH and no account has more to spare: send testnet ETH to it (or to the PQ account)",
            have as f64 / ETH
        ));
    }
    Ok(())
}

fn send(ctx: &Ctx, src: &Source, to: &str, value: u128) -> Result<(), String> {
    let to_bytes = addr20(to)?;
    let hash = match src {
        Source::Ecdsa { key, addr, .. } => {
            let nonce = hex_u128(&rpc_call(&ctx.rpc, "eth_getTransactionCount", json!([addr, "pending"]))?) as u64;
            // A transfer to a new account also pays state gas for it; 250,000 covers both.
            let tx = Eip1559Tx {
                chain_id: ctx.chain_id,
                nonce,
                max_priority_fee_per_gas: 1_000_000_000,
                max_fee_per_gas: 2_000_000_000,
                gas_limit: 250_000,
                to: to_bytes,
                value,
                data: vec![],
            };
            let raw = tx.sign(key)?;
            rpc_call(&ctx.rpc, "eth_sendRawTransaction", json!([format!("0x{}", hex::encode(raw))]))?
        }
        Source::Pq { addr, verify_selector, key_file } => {
            let mut nonce_key = [0u8; 16];
            getrandom::fill(&mut nonce_key).map_err(|e| e.to_string())?;
            let account = addr20(addr)?;
            let mut tx = crypto::FrameTransaction {
                chain_id: ctx.chain_id,
                nonce_keys: vec![u128::from_be_bytes(nonce_key)],
                nonce_seq: 0,
                sender: account,
                frames: vec![
                    crypto::Frame {
                        mode: 1,
                        flags: 3,
                        target: Some(account),
                        gas_limit: 120_000,
                        state_limit: 110_000,
                        value: 0,
                        data: hex::decode(verify_selector.trim_start_matches("0x")).map_err(|e| e.to_string())?,
                    },
                    crypto::Frame {
                        mode: 2,
                        flags: 0,
                        target: Some(to_bytes),
                        gas_limit: 50_000,
                        state_limit: 200_000,
                        value,
                        data: vec![],
                    },
                ],
                signatures: vec![],
                max_priority_fee: 1_000_000_000,
                max_fee: 2_000_000_000,
                max_blob_fee: 0,
                blob_hashes: vec![],
            };
            let raw = tx.sign_with_lsk(&ctx.paths.setting("LSK_BIN", "lsk"), key_file)?;
            rpc_call(&ctx.rpc, "eth_sendRawTransaction", json!([format!("0x{}", hex::encode(raw))]))?
        }
    };
    let hash = hash.as_str().ok_or("no transaction hash")?.to_string();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(90) {
        std::thread::sleep(Duration::from_millis(1500));
        if let Ok(rc) = rpc_call(&ctx.rpc, "eth_getTransactionReceipt", json!([hash]))
            && !rc.is_null()
        {
            return if rc["status"].as_str() == Some("0x1") { Ok(()) } else { Err(format!("funding transfer {hash} reverted")) };
        }
    }
    Err(format!("funding transfer {hash} not mined within 90 s"))
}

fn balance(ctx: &Ctx, addr: &str) -> Result<u128, String> {
    Ok(hex_u128(&rpc_call(&ctx.rpc, "eth_getBalance", json!([addr, "latest"]))?))
}

fn hex_u128(v: &Value) -> u128 {
    v.as_str().and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0)
}

fn addr20(a: &str) -> Result<[u8; 20], String> {
    let bytes = hex::decode(a.trim_start_matches("0x")).map_err(|e| format!("bad address {a}: {e}"))?;
    bytes.try_into().map_err(|_| format!("bad address {a}"))
}

fn ecdsa_from_file(file: &Path, name: &str) -> Option<Source> {
    let key = crypto::private_key_from_hex(&fs::read_to_string(file).ok()?).ok()?;
    let addr = crypto::checksum_address(&crypto::address_from_key(&key).ok()?);
    Some(Source::Ecdsa { name: name.into(), key, addr })
}

/// The console's own accounts (their account.key files): demo withdrawals pay them.
fn console_accounts(paths: &Paths) -> Vec<Source> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(paths.accounts_dir()) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(src) = ecdsa_from_file(&e.path().join("account.key"), &name) {
                out.push(src);
            }
        }
    }
    out
}

/// The leanSPHINCS PQ account, when its record and key are here.
fn pq_account(paths: &Paths) -> Option<Source> {
    let file = paths.p(&format!("deployments/{}/account.json", 8141));
    let rec: Value = serde_json::from_str(&fs::read_to_string(file).ok()?).ok()?;
    let key_file = paths.p(&paths.setting("ACCOUNT_PQ_KEY", ".secrets/pq/user.key"));
    key_file.exists().then(|| Source::Pq {
        addr: rec["account"].as_str().unwrap_or_default().to_string(),
        verify_selector: rec["verifySelector"].as_str().unwrap_or_default().to_string(),
        key_file,
    })
}
