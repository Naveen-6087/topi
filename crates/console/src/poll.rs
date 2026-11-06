//! Background pollers: node counters over the stats port, and the chain over JSON-RPC.

use std::collections::HashMap;
use std::io::Read;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::mixnet_client::NodeStats;
use serde_json::{Value, json};

/// A node to watch.
#[derive(Clone, Debug)]
pub struct Target {
    pub role: String,
    pub host: String,
    pub stats_port: u16,
    pub region: String,
    pub instance: String,
}

/// Latest counters per role; `None` when the node did not answer.
pub type Stats = Arc<Mutex<HashMap<String, Option<NodeStats>>>>;

pub fn watch_nodes(targets: Arc<Mutex<Vec<Target>>>, out: Stats) {
    std::thread::spawn(move || {
        loop {
            let list = targets.lock().unwrap().clone();
            let answers: Vec<_> = list.into_iter().map(|t| std::thread::spawn(move || (t.role.clone(), fetch(&t)))).collect();
            let fresh = answers.into_iter().filter_map(|h| h.join().ok()).collect();
            *out.lock().unwrap() = fresh;
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn fetch(t: &Target) -> Option<NodeStats> {
    let addr = format!("{}:{}", t.host, t.stats_port).parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let mut body = String::new();
    s.read_to_string(&mut body).ok()?;
    serde_json::from_str(&body).ok()
}

#[derive(Clone, Debug, Default)]
pub struct ChainEvent {
    pub block: u64,
    pub kind: &'static str,
    pub tx: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub block: u64,
    pub pool_wei: u128,
    pub account_wei: u128,
    pub paymaster_wei: u128,
    pub events: Vec<ChainEvent>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ChainConfig {
    pub rpc: String,
    pub pool: String,
    pub account: String,
    pub paymaster: String,
    pub from_block: u64,
}

const DEPOSITED: &str = "0x0a944b8f49444f906e0a3f95a3c09508d3697509680d81709d25c327ab8cf651";
const SETTLED: &str = "0xd6bc57af9bdeff672c39eb7ac8725e83883c78f01ea44f6ce017a99e3ef5bb3c";

pub fn watch_chain(cfg: Arc<Mutex<ChainConfig>>, out: Arc<Mutex<Chain>>) {
    std::thread::spawn(move || {
        loop {
            let current_cfg = cfg.lock().unwrap().clone();
            match read_chain(&current_cfg) {
                Ok(c) => *out.lock().unwrap() = c,
                Err(e) => out.lock().unwrap().error = Some(e.chars().take(120).collect()),
            }
            std::thread::sleep(Duration::from_secs(4));
        }
    });
}

fn read_chain(cfg: &ChainConfig) -> Result<Chain, String> {
    let rpc = |method: &str, params: Value| rpc(&cfg.rpc, method, params);
    let balance = |a: &str| rpc("eth_getBalance", json!([a, "latest"])).map(|v| hex_u128(&v)).unwrap_or(0);
    let mut c = Chain {
        block: hex_u128(&rpc("eth_blockNumber", json!([]))?) as u64,
        pool_wei: balance(&cfg.pool),
        account_wei: balance(&cfg.account),
        paymaster_wei: balance(&cfg.paymaster),
        ..Default::default()
    };
    let from = format!("0x{:x}", cfg.from_block);
    match rpc("eth_getLogs", json!([{ "address": cfg.pool, "fromBlock": from, "toBlock": "latest" }])) {
        Ok(logs) => {
            for l in logs.as_array().cloned().unwrap_or_default().iter().rev().take(60) {
                let data = l["data"].as_str().unwrap_or("0x").trim_start_matches("0x").to_string();
                let word = |i: usize| data.get(i * 64 + 32..i * 64 + 64).and_then(|h| u128::from_str_radix(h, 16).ok()).unwrap_or(0);
                let (kind, detail) = match l["topics"][0].as_str().unwrap_or("") {
                    DEPOSITED => ("deposit", format!("{} into leaf {}", crate::theme::eth(word(3)), word(0))),
                    SETTLED => ("settle", format!("two nullifiers spent, change notes from leaf {}", word(0))),
                    _ => ("event", String::new()),
                };
                c.events.push(ChainEvent {
                    block: hex_u128(&l["blockNumber"]) as u64,
                    kind,
                    tx: l["transactionHash"].as_str().unwrap_or("").into(),
                    detail,
                });
            }
        }
        Err(e) => c.error = Some(format!("logs: {e}")),
    }
    Ok(c)
}

/// The alternative mempool's status, or None if it does not answer within a moment.
pub fn mempool_status(url: &str) -> Option<Value> {
    let call = |method: &str, params: Value| -> Option<Value> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let resp: Value = ureq::post(url).timeout(Duration::from_millis(800)).send_json(body).ok()?.into_json().ok()?;
        resp.get("result").cloned()
    };
    // The ERC-8421 sidecar: counters from its explorer's admissions and transactions.
    let admissions = call("skandha_explorer_listAdmissions", json!([{}]))?;
    let txs = call("skandha_explorer_listTransactions", json!([{}])).unwrap_or(Value::Null);
    let items = |v: &Value| v["items"].as_array().cloned().unwrap_or_default();
    let (mut admitted, mut rejected) = (0u64, 0u64);
    for a in items(&admissions) {
        match a["outcome"].as_str() {
            Some("rejected") => rejected += 1,
            Some(_) => admitted += 1,
            None => {}
        }
    }
    let status = |s: &str| items(&txs).iter().filter(|t| t["status"].as_str() == Some(s)).count() as u64;
    let mined = status("OnChain") + status("Finalized");
    Some(json!({
        "pending": status("New"),
        "inflight": status("Submitted") + status("Pending"),
        "counters": { "admitted": admitted, "rejected": { "rules": rejected }, "released": status("Submitted") + mined, "mined": mined },
        "peers": [],
    }))
}

fn rpc(url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp: Value = ureq::post(url)
        .timeout(Duration::from_secs(30))
        .send_json(body)
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    if let Some(err) = resp.get("error") {
        return Err(err.to_string());
    }
    Ok(resp["result"].clone())
}

fn hex_u128(v: &Value) -> u128 {
    v.as_str().and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0)
}
