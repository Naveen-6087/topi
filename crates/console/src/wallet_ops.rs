//! Native Rust implementation of wallet operations: note generation, on-chain deposit,
//! Merkle tree witnessing, and request building. Zero Node.js or TypeScript dependencies.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::crypto::{self, Eip1559Tx};
use crate::paths::Paths;

/// Append a line to logs/pipeline.log
pub fn pipeline_log(paths: &Paths, component: &str, message: &str) {
    let log_file = paths.p("logs/pipeline.log");
    if let Some(parent) = log_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = format!("{now} {component} {message}\n");
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&log_file);
    if let Ok(ref mut f) = file {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

/// Unpack a 256-bit uint256 word into 8 32-bit field elements (little-endian: element i at bits 32i).
pub fn unpack_digest(packed: u128, packed_hi: u128) -> [u32; 8] {
    std::array::from_fn(|i| {
        let word = if i < 4 { packed } else { packed_hi };
        ((word >> (32 * (i % 4))) & 0xffff_ffff) as u32
    })
}

pub fn unpack_word_hex(hex_word: &str) -> [u32; 8] {
    let s = hex_word.trim_start_matches("0x");
    let s = format!("{:0>64}", s);
    std::array::from_fn(|i| {
        let end = 64 - 8 * i;
        u32::from_str_radix(&s[end - 8..end], 16).unwrap_or(0)
    })
}

/// Helper for JSON-RPC requests via ureq with automatic retry and backoff on transient DNS/network drops
pub fn rpc_call(url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let mut last_err = String::new();
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(500 * (1 << (attempt - 1)).min(4000)));
        }
        let res = ureq::post(url).timeout(Duration::from_secs(30)).send_json(&body);
        match res {
            Ok(resp) => match resp.into_json::<Value>() {
                Ok(val) => {
                    if let Some(err) = val.get("error") {
                        return Err(err.to_string());
                    }
                    return Ok(val["result"].clone());
                }
                Err(e) => last_err = e.to_string(),
            },
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    Err(format!("{url}: {last_err}"))
}

/// Step 1: Create a fresh private payment note and change note.
pub fn new_note(paths: &Paths, name: &str, notes_dir: &Path, log: &dyn Fn(&str)) -> Result<(), String> {
    let note_file = notes_dir.join(format!("note-{name}.json"));
    let change_file = notes_dir.join(format!("change-{name}.json"));

    if note_file.exists() {
        return Err(format!("note '{name}' already exists"));
    }
    fs::create_dir_all(notes_dir).map_err(|e| e.to_string())?;

    log(&format!("generating secrets for note {name}..."));
    for file in [&note_file, &change_file] {
        paths.prover_run("pool-wallet keygen", r#""$WALLET_BIN" keygen "$1""#, &[paths.linux(file)])?;
    }

    log(&format!("note {name}: fresh note and change-note secrets (Poseidon2 precommitments)"));
    pipeline_log(paths, "wallet", &format!("new note {name}"));
    Ok(())
}

/// Step 2: Deposit the note into the Shielded Privacy Pool on chain 8141.
pub fn deposit(
    paths: &Paths,
    name: &str,
    amount_wei: u128,
    notes_dir: &Path,
    account_dir: Option<&Path>,
    log: &dyn Fn(&str),
) -> Result<(), String> {
    let note_file = notes_dir.join(format!("note-{name}.json"));
    if !note_file.exists() {
        return Err(format!("note file {} does not exist", note_file.display()));
    }
    let note_content = fs::read_to_string(&note_file).map_err(|e| e.to_string())?;
    let mut note: Value = serde_json::from_str(&note_content).map_err(|e| e.to_string())?;

    if note.get("deposit_tx").and_then(|v| v.as_str()).is_some() {
        return Err(format!("note '{name}' is already deposited ({})", note["deposit_tx"]));
    }

    let precommit_vals = note["precommitment"].as_array().ok_or_else(|| "precommitment is missing or not an array".to_string())?;
    if precommit_vals.len() != 8 {
        return Err(format!("expected 8 limbs in precommitment, got {}", precommit_vals.len()));
    }

    // Build 260-byte calldata: 4 bytes selector (0x09d009b8) + 8 * 32 bytes (uint256[8])
    let mut calldata = Vec::with_capacity(4 + 8 * 32);
    calldata.extend_from_slice(&hex::decode("09d009b8").unwrap());
    for v in precommit_vals {
        let limb = v.as_u64().ok_or_else(|| "limb not a number".to_string())? as u32;
        let mut word = [0u8; 32];
        word[28..32].copy_from_slice(&limb.to_be_bytes());
        calldata.extend_from_slice(&word);
    }

    let pool_file = paths.pool_deployment();
    let pool_json: Value =
        serde_json::from_str(&fs::read_to_string(&pool_file).map_err(|e| format!("cannot read {}: {e}", pool_file.display()))?)
            .map_err(|e| e.to_string())?;

    let pool_addr_str = pool_json["pool"].as_str().ok_or_else(|| "missing pool address in deployment record".to_string())?;
    let pool_addr_clean = pool_addr_str.trim().trim_start_matches("0x");
    let pool_addr_bytes = hex::decode(pool_addr_clean).map_err(|e| format!("invalid pool address hex: {e}"))?;
    if pool_addr_bytes.len() != 20 {
        return Err("pool address must be 20 bytes".into());
    }
    let mut pool_to = [0u8; 20];
    pool_to.copy_from_slice(&pool_addr_bytes);

    let rpc_url = paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz");

    // Select key: prefer funded account.key or fall back to .secrets/deployer.key
    let mut chosen_key: Option<([u8; 32], String)> = None;
    if let Some(acc_dir) = account_dir {
        let key_file = acc_dir.join("account.key");
        if key_file.exists()
            && let Ok(s) = fs::read_to_string(&key_file)
            && let Ok(key) = crypto::private_key_from_hex(&s)
            && let Ok(addr) = crypto::address_from_key(&key)
        {
            let addr_str = crypto::checksum_address(&addr);
            let bal = rpc_call(&rpc_url, "eth_getBalance", json!([&addr_str, "latest"]))
                .ok()
                .and_then(|v| v.as_str().and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok()))
                .unwrap_or(0);
            if bal >= amount_wei + 20_000_000_000_000 {
                chosen_key = Some((key, addr_str));
            }
        }
    }

    if chosen_key.is_none() {
        let deployer_file = paths.p(".secrets/deployer.key");
        if deployer_file.exists() {
            let s = fs::read_to_string(&deployer_file).map_err(|e| e.to_string())?;
            let key = crypto::private_key_from_hex(&s)?;
            let addr = crypto::address_from_key(&key)?;
            let addr_str = crypto::checksum_address(&addr);
            chosen_key = Some((key, addr_str));
        }
    }

    let (priv_key, sender_addr) =
        chosen_key.ok_or_else(|| "no funded Ethereum key found (checked account.key and .secrets/deployer.key)".to_string())?;

    log(&format!("depositing {} wei from {}...", amount_wei, sender_addr));

    let nonce_hex = rpc_call(&rpc_url, "eth_getTransactionCount", json!([&sender_addr, "pending"]))?;
    let nonce =
        u64::from_str_radix(nonce_hex.as_str().unwrap_or("0x0").trim_start_matches("0x"), 16).map_err(|e| format!("bad nonce: {e}"))?;

    // The node's estimate plus a quarter, so the sender need not hold a 15.5M-gas reserve.
    let estimate = rpc_call(
        &rpc_url,
        "eth_estimateGas",
        json!([{ "from": &sender_addr, "to": pool_addr_str, "value": format!("0x{amount_wei:x}"), "data": format!("0x{}", hex::encode(&calldata)) }]),
    )
    .ok()
    .and_then(|v| v.as_str().and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()));
    let gas_limit = estimate.map(|g| g + g / 4).unwrap_or(15_500_000);

    let tx = Eip1559Tx {
        chain_id: pool_json["chainId"].as_u64().unwrap_or(8141),
        nonce,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 2_000_000_000,
        gas_limit,
        to: pool_to,
        value: amount_wei,
        data: calldata,
    };

    let raw_tx = tx.sign(&priv_key)?;
    let raw_hex = format!("0x{}", hex::encode(&raw_tx));

    log("broadcasting transaction to Ethrex RPC...");
    let tx_hash_val = rpc_call(&rpc_url, "eth_sendRawTransaction", json!([raw_hex]))?;
    let tx_hash = tx_hash_val.as_str().ok_or_else(|| "no tx hash returned".to_string())?;
    log(&format!("tx submitted: {tx_hash}, awaiting receipt..."));

    // Poll for receipt
    let start_wait = Instant::now();
    let mut receipt: Option<Value> = None;
    while start_wait.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(600));
        if let Ok(rc) = rpc_call(&rpc_url, "eth_getTransactionReceipt", json!([tx_hash]))
            && !rc.is_null()
            && rc.get("blockNumber").is_some()
        {
            receipt = Some(rc);
            break;
        }
    }

    let rc = receipt.ok_or_else(|| "timeout waiting for deposit transaction receipt".to_string())?;
    if rc["status"].as_str() != Some("0x1") {
        return Err(format!("deposit transaction reverted: {tx_hash}"));
    }

    let block_num = u64::from_str_radix(rc["blockNumber"].as_str().unwrap_or("0x0").trim_start_matches("0x"), 16).unwrap_or(0);

    // Find Deposited log
    const DEPOSITED_TOPIC: &str = "0x0a944b8f49444f906e0a3f95a3c09508d3697509680d81709d25c327ab8cf651";
    let logs = rc["logs"].as_array().ok_or_else(|| "no logs in receipt".to_string())?;
    let dep_log = logs
        .iter()
        .find(|l| l["topics"].get(0).and_then(|t| t.as_str()) == Some(DEPOSITED_TOPIC))
        .ok_or_else(|| "no Deposited event found in transaction receipt".to_string())?;

    let label_index = u64::from_str_radix(dep_log["topics"][1].as_str().unwrap_or("0x0").trim_start_matches("0x"), 16).unwrap_or(0);

    let data_hex = dep_log["data"].as_str().unwrap_or("0x").trim_start_matches("0x");
    if data_hex.len() < 64 * 3 {
        return Err("insufficient data in Deposited event log".into());
    }

    let leaf_index = u64::from_str_radix(&data_hex[0..64], 16).unwrap_or(0);
    let label_word = &data_hex[128..192];
    let label_digest = unpack_word_hex(label_word);

    // Update note file
    note["amount"] = json!(amount_wei.to_string());
    note["label"] = json!(label_digest);
    note["leaf_index"] = json!(leaf_index);
    note["label_index"] = json!(label_index);
    note["deposit_tx"] = json!(tx_hash);
    note["deposit_block"] = json!(block_num);
    note["depositor"] = json!(sender_addr);

    fs::write(&note_file, serde_json::to_string_pretty(&note).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    log(&format!("note {name}: deposited {amount_wei} wei, leaf {leaf_index}, block {block_num}"));
    pipeline_log(paths, "wallet", &format!("deposited note {name}: leaf {leaf_index}, block {block_num}"));

    Ok(())
}

/// Step 3: Witness Merkle membership and prepare withdrawal inputs.
pub fn witness(paths: &Paths, name: &str, recipient: &str, notes_dir: &Path, log: &dyn Fn(&str)) -> Result<(), String> {
    let note_file = notes_dir.join(format!("note-{name}.json"));
    let change_file = notes_dir.join(format!("change-{name}.json"));
    let request_file = notes_dir.join(format!("request-{name}.json"));
    let chain_file = notes_dir.join("chain.json");
    let input_file = notes_dir.join(format!("input-{name}.json"));
    let public_file = notes_dir.join(format!("public-{name}.json"));

    let note_content = fs::read_to_string(&note_file).map_err(|e| e.to_string())?;
    let note: Value = serde_json::from_str(&note_content).map_err(|e| e.to_string())?;

    let amount_str = note["amount"].as_str().ok_or_else(|| format!("note '{name}' has no deposit yet (run deposit first)"))?;
    let amount: u128 = amount_str.parse().map_err(|e| format!("bad note amount: {e}"))?;

    let change_content = fs::read_to_string(&change_file).map_err(|e| e.to_string())?;
    let change_note: Value = serde_json::from_str(&change_content).map_err(|e| e.to_string())?;

    let pool_file = paths.pool_deployment();
    let pool_json: Value =
        serde_json::from_str(&fs::read_to_string(&pool_file).map_err(|e| format!("cannot read {}: {e}", pool_file.display()))?)
            .map_err(|e| e.to_string())?;

    let pool_addr = pool_json["pool"].as_str().ok_or("missing pool address")?;
    let operator_addr = pool_json["operator"].as_str().ok_or("missing operator address")?;
    let pool_id = &pool_json["poolId"];
    let scope = &pool_json["scope"];

    let rpc_url = paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz");

    log("querying on-chain pool logs to rebuild Merkle trees...");
    let logs_val = rpc_call(&rpc_url, "eth_getLogs", json!([{ "address": pool_addr, "fromBlock": "0x84ef8", "toBlock": "latest" }]))?;
    let logs = logs_val.as_array().ok_or_else(|| "logs is not an array".to_string())?;

    const DEPOSITED_TOPIC: &str = "0x0a944b8f49444f906e0a3f95a3c09508d3697509680d81709d25c327ab8cf651";
    const SETTLED_TOPIC: &str = "0xd6bc57af9bdeff672c39eb7ac8725e83883c78f01ea44f6ce017a99e3ef5bb3c";

    let zero_leaf = [0u32; 8];
    let mut pool_leaves: Vec<[u32; 8]> = Vec::new();
    let mut assoc_leaves: Vec<[u32; 8]> = Vec::new();

    let put_pool = |leaves: &mut Vec<[u32; 8]>, idx: usize, leaf: [u32; 8]| {
        while leaves.len() <= idx {
            leaves.push(zero_leaf);
        }
        leaves[idx] = leaf;
    };

    for l in logs {
        let topic0 = l["topics"].get(0).and_then(|t| t.as_str()).unwrap_or("");
        let data = l["data"].as_str().unwrap_or("0x").trim_start_matches("0x");
        if topic0 == DEPOSITED_TOPIC && data.len() >= 64 * 3 {
            let leaf_idx = usize::from_str_radix(&data[0..64], 16).unwrap_or(0);
            let note_digest = unpack_word_hex(&data[64..128]);
            let label_digest = unpack_word_hex(&data[128..192]);
            let assoc_idx =
                usize::from_str_radix(l["topics"].get(1).and_then(|t| t.as_str()).unwrap_or("0x0").trim_start_matches("0x"), 16)
                    .unwrap_or(0);

            put_pool(&mut pool_leaves, leaf_idx, note_digest);
            put_pool(&mut pool_leaves, leaf_idx + 1, zero_leaf);
            while assoc_leaves.len() <= assoc_idx {
                assoc_leaves.push(zero_leaf);
            }
            assoc_leaves[assoc_idx] = label_digest;
        } else if topic0 == SETTLED_TOPIC && data.len() >= 64 * 5 {
            let leaf_idx = usize::from_str_radix(&data[0..64], 16).unwrap_or(0);
            let change_a = unpack_word_hex(&data[192..256]);
            let change_b = unpack_word_hex(&data[256..320]);
            put_pool(&mut pool_leaves, leaf_idx, change_a);
            put_pool(&mut pool_leaves, leaf_idx + 1, change_b);
        }
    }

    log("fetching current pool roots from contract...");
    // poolRoot() -> 0x969e78f3
    let pr_hex = rpc_call(&rpc_url, "eth_call", json!([{ "to": pool_addr, "data": "0x969e78f3" }, "latest"]))?;
    let pool_root = unpack_word_hex(pr_hex.as_str().unwrap_or("0x0"));

    // associationRoot() -> 0xfab41774
    let ar_hex = rpc_call(&rpc_url, "eth_call", json!([{ "to": pool_addr, "data": "0xfab41774" }, "latest"]))?;
    let assoc_root = unpack_word_hex(ar_hex.as_str().unwrap_or("0x0"));

    // nextPoolIndex() -> 0xf335a06d
    let next_hex = rpc_call(&rpc_url, "eth_call", json!([{ "to": pool_addr, "data": "0xf335a06d" }, "latest"]))?;
    let next_leaf = usize::from_str_radix(next_hex.as_str().unwrap_or("0x0").trim_start_matches("0x"), 16).unwrap_or(0);

    let chain_view = json!({
        "pool": pool_id,
        "scope": scope,
        "pool_leaves": pool_leaves,
        "association_leaves": assoc_leaves,
        "pool_root": pool_root,
        "association_root": assoc_root,
        "next_leaf": next_leaf,
    });

    fs::write(&chain_file, serde_json::to_string_pretty(&chain_view).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    // Send amount = deposited amount (100% full amount, 0 fee, 0 change)
    let request_json = json!({
        "recipient": recipient,
        "withdrawn": amount.to_string(),
        "fee": "0",
        "fee_recipient": operator_addr,
        "change_note": change_note,
    });

    fs::write(&request_file, serde_json::to_string_pretty(&request_json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    log("running pool-wallet witness calculation...");
    paths.prover_run(
        "pool-wallet witness",
        r#""$WALLET_BIN" witness "$1" "$2" "$3" "$4""#,
        &[paths.linux(&note_file), paths.linux(&chain_file), paths.linux(&request_file), paths.linux(&input_file)],
    )?;

    let input_data: Value =
        serde_json::from_str(&fs::read_to_string(&input_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    let public_fields = &input_data["public"]["fields"];
    fs::write(&public_file, json!({ "fields": public_fields }).to_string()).map_err(|e| e.to_string())?;

    log(&format!("withdrawal {name}: {amount} wei to {recipient}, fee 0, change 0"));
    pipeline_log(paths, "wallet", &format!("withdrawal {name} witnessed against the pool's current roots"));

    Ok(())
}

/// Step 4: Generate full zero-knowledge Spartan-WHIR client proof using the compiled prover binary directly.
pub fn prove_note(paths: &Paths, name: &str, notes_dir: &Path, log: &dyn Fn(&str)) -> Result<(), String> {
    let input_file = notes_dir.join(format!("input-{name}.json"));
    if !input_file.exists() {
        return Err(format!("note '{name}' has no witness yet (run witness first)"));
    }
    let send_dir = notes_dir.join("send");
    fs::create_dir_all(&send_dir).map_err(|e| e.to_string())?;
    let out_archive = send_dir.join(format!("{name}.tgz"));

    log(&format!("generating full-ZK Spartan-WHIR client proof for note {name}..."));
    let started = Instant::now();
    let script = paths.linux(&paths.p("prover/prove-client.sh"));
    paths.prover_run("prove-client.sh", r#"bash "$1" "$2" "$3""#, &[script, paths.linux(&input_file), paths.linux(&out_archive)])?;

    let size = fs::metadata(&out_archive).map(|m| m.len()).unwrap_or(0);
    log(&format!("wallet proof ready for note {name}: {} KB in {}s (Spartan-WHIR)", size / 1024, started.elapsed().as_secs()));
    pipeline_log(paths, "wallet", &format!("wallet proof ready for note {name} (full-ZK Spartan-WHIR)"));
    Ok(())
}
