//! Native Rust exit prover and settlement manager.
//! Scans the spend queue, validates client proofs, invokes leanVM recursion,
//! and broadcasts the settlement transaction to Ethrex chain 8141. Zero Node.js or TypeScript.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::crypto::{self};
use crate::paths::Paths;
use crate::wallet_ops::{pipeline_log, rpc_call};
use rand_core::RngCore;

#[derive(Clone, Debug)]
pub struct Spend {
    pub id: String,
    pub fields_file: PathBuf,
    pub proof_file: PathBuf,
    pub fields: Vec<u32>,
}

/// Encode ABI calldata for settle(bytes proofCall, uint256[] a, uint256[] b).
pub fn encode_settle_calldata(proof_call: &[u8], a: &[u32], b: &[u32]) -> Vec<u8> {
    // Selector for settle(bytes,uint256[],uint256[]) = 0xf4b589c7
    let mut out = Vec::new();
    out.extend_from_slice(&hex::decode("f4b589c7").unwrap());

    // Head: 3 offsets (head size = 3 * 32 = 96)
    let head_offset: usize = 96;
    let proof_padded_len = proof_call.len().div_ceil(32) * 32;
    let bytes_part_len = 32 + proof_padded_len;
    let offset_a = head_offset + bytes_part_len;
    let offset_b = offset_a + 32 + a.len() * 32;

    let write_word = |buf: &mut Vec<u8>, val: usize| {
        let mut w = [0u8; 32];
        w[24..32].copy_from_slice(&(val as u64).to_be_bytes());
        buf.extend_from_slice(&w);
    };

    write_word(&mut out, head_offset);
    write_word(&mut out, offset_a);
    write_word(&mut out, offset_b);

    // Tail: bytes proofCall
    write_word(&mut out, proof_call.len());
    out.extend_from_slice(proof_call);
    let pad = proof_padded_len - proof_call.len();
    out.extend(std::iter::repeat_n(0u8, pad));

    // Tail: uint256[] a
    write_word(&mut out, a.len());
    for &elem in a {
        let mut w = [0u8; 32];
        w[28..32].copy_from_slice(&elem.to_be_bytes());
        out.extend_from_slice(&w);
    }

    // Tail: uint256[] b
    write_word(&mut out, b.len());
    for &elem in b {
        let mut w = [0u8; 32];
        w[28..32].copy_from_slice(&elem.to_be_bytes());
        out.extend_from_slice(&w);
    }

    out
}

/// Check if a spend's public fields match Spartan-WHIR proof statement.
pub fn check_spend_proof(paths: &Paths, fields_file: &Path, proof_file: &Path) -> bool {
    let script = r#"dir=$(mktemp -d) && trap 'rm -rf "$dir"' EXIT && tar -xzf "$2" -C "$dir" && "$WALLET_BIN" check "$1" "$dir""#;
    paths
        .prover_run("pool-wallet check", script, &[paths.linux(fields_file), paths.linux(proof_file)])
        .is_ok_and(|out| out.contains("\"ok\":true"))
}

// Offsets into a spend's 82 public settle fields (sdk SPEND_FIELDS), and the pool's view selectors.
const POOL_ROOT: usize = 50;
const ASSOCIATION_ROOT: usize = 58;
const NULLIFIER: usize = 66;
const SPENT: &str = "d5b5cc23"; // spent(uint256)
const KNOWN_POOL_ROOT: &str = "0a00c08f"; // knownPoolRoot(uint256)
const KNOWN_ASSOCIATION_ROOT: &str = "0850ebcc"; // knownAssociationRoot(uint256)

/// Eight 32-bit limbs from `start` packed into one uint256 (limb i at bits 32i, as the sdk's packDigest),
/// as 64 hex characters, big-endian, ready for calldata.
pub fn packed_word_hex(fields: &[u32], start: usize) -> String {
    let mut word = [0u8; 32];
    for (i, limb) in fields[start..start + 8].iter().enumerate() {
        word[28 - 4 * i..32 - 4 * i].copy_from_slice(&limb.to_be_bytes());
    }
    hex::encode(word)
}

fn read_pool_address(paths: &Paths) -> String {
    fs::read_to_string(paths.pool_deployment())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["pool"].as_str().map(String::from))
        .unwrap_or_default()
}

/// A pool view `selector(uint256 word)` that returns a bool.
fn pool_flag(rpc_url: &str, pool: &str, selector: &str, word: &str) -> Result<bool, String> {
    let out = rpc_call(rpc_url, "eth_call", json!([{ "to": pool, "data": format!("0x{selector}{word}") }, "latest"]))?;
    let s = out.as_str().ok_or("eth_call returned no data")?.trim_start_matches("0x");
    if s.is_empty() {
        return Err("eth_call returned empty data".into());
    }
    Ok(s.chars().any(|c| c != '0'))
}

/// Reads queued spends from queue_dir, filters valid ones, and moves rejected ones to queue_dir/rejected.
pub fn scan_queue(paths: &Paths, queue_dir: &Path, log: &dyn Fn(&str)) -> Vec<Spend> {
    let mut spends = Vec::new();
    let Ok(entries) = fs::read_dir(queue_dir) else {
        return spends;
    };

    let rejected_dir = queue_dir.join("rejected");
    let _ = fs::create_dir_all(&rejected_dir);

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let proof_file = queue_dir.join(format!("{file_stem}.tgz"));
            if !proof_file.exists() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<Value>(&content)
                && let Some(arr) = val["fields"].as_array()
            {
                let fields: Vec<u32> = arr.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect();
                if fields.len() == 82 {
                    // Ask the pool: nullifier unspent, both roots known (the same reads the alt mempool makes).
                    let rpc_url = paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz");
                    let pool_addr = read_pool_address(paths);
                    let ask = |selector: &str, start: usize| pool_flag(&rpc_url, &pool_addr, selector, &packed_word_hex(&fields, start));
                    let (spent, pool_root, association_root) =
                        match (ask(SPENT, NULLIFIER), ask(KNOWN_POOL_ROOT, POOL_ROOT), ask(KNOWN_ASSOCIATION_ROOT, ASSOCIATION_ROOT)) {
                            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                                log(&format!("skipping {file_stem} this round: cannot read the pool ({e})"));
                                continue;
                            }
                        };
                    let is_spent = spent;
                    if !is_spent && !(pool_root && association_root) {
                        log(&format!("rejected {file_stem}: a root the spend was proven against is unknown to the pool"));
                        let _ = fs::rename(&path, rejected_dir.join(format!("{file_stem}.json")));
                        let _ = fs::rename(&proof_file, rejected_dir.join(format!("{file_stem}.tgz")));
                        continue;
                    }

                    if is_spent {
                        log(&format!("spend {file_stem} already spent on-chain (moving to done/)"));
                        let done_dir = queue_dir.join("done");
                        let _ = fs::create_dir_all(&done_dir);
                        let _ = fs::rename(&path, done_dir.join(format!("{file_stem}.json")));
                        let _ = fs::rename(&proof_file, done_dir.join(format!("{file_stem}.tgz")));
                        continue;
                    }

                    if check_spend_proof(paths, &path, &proof_file) {
                        log(&format!("checked {file_stem}: pool, roots, nullifier and statement ok"));
                        spends.push(Spend { id: file_stem, fields_file: path, proof_file, fields });
                    } else {
                        log(&format!("rejected {file_stem}: fields do not match proof"));
                        let _ = fs::rename(&path, rejected_dir.join(format!("{file_stem}.json")));
                    }
                }
            }
        }
    }
    spends.sort_by(|a, b| a.id.cmp(&b.id));
    spends
}

/// Runs the full exit prover pipeline: checks queue, pairs two spends, runs leanVM recursion,
/// and broadcasts the settle transaction.
/// With `mempool`, the settle goes to that alternative mempool (eth_sendRawTransaction) instead of the node.
pub fn run_exit_prover(paths: &Paths, queue_path: &str, mempool: Option<&str>, log: &dyn Fn(&str)) -> Result<(), String> {
    let queue_dir = PathBuf::from(queue_path);
    log(&format!("checking spend queue in {}...", queue_dir.display()));

    let ready = scan_queue(paths, &queue_dir, log);
    if ready.len() < 2 {
        log(&format!("{} spend(s) ready; waiting for a pair (the terminal proves two statements)", ready.len()));
        return Ok(());
    }

    let a = &ready[0];
    let b = &ready[1];
    let out_dir = queue_dir.join(format!("settle-{}-{}", a.id, b.id));
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let calldata_file = out_dir.join("calldata.bin");
    let calldata_bytes = if calldata_file.exists() && fs::metadata(&calldata_file).map(|m| m.len()).unwrap_or(0) > 0 {
        log(&format!("reusing existing terminal proof for pair {} + {}...", a.id, b.id));
        fs::read(&calldata_file).map_err(|e| format!("missing calldata.bin: {e}"))?
    } else {
        log(&format!("proving pair {} + {} (leanVM recursion)...", a.id, b.id));
        pipeline_log(paths, "exit-prover", &format!("proving pair {} + {} (leanVM recursion)", a.id, b.id));

        let started = Instant::now();
        // prove-exit.sh also checks that the proof is for the guest programs of the deployed terminal verifier.
        if paths.setting("EXIT_PROVER", "local") == "aws" {
            // On the AWS prover machine (infra/aws/prover.sh up and setup): many more cores.
            log("recursion on the AWS prover machine...");
            let out = std::process::Command::new(&paths.bash)
                .arg(paths.p("infra/aws/prover.sh"))
                .args(["prove"])
                .arg(&a.proof_file)
                .arg(&b.proof_file)
                .arg(&out_dir)
                .current_dir(&paths.root)
                .output()
                .map_err(|e| format!("cannot run infra/aws/prover.sh: {e}"))?;
            if !out.status.success() {
                return Err(format!("AWS recursion failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
        } else {
            let script = paths.linux(&paths.p("prover/prove-exit.sh"));
            paths.prover_run(
                "leanVM recursion (prove-exit.sh)",
                r#"bash "$1" "$2" "$3" "$4""#,
                &[script, paths.linux(&a.proof_file), paths.linux(&b.proof_file), paths.linux(&out_dir)],
            )?;
        }

        log(&format!("terminal proof ready in {}s", started.elapsed().as_secs()));
        pipeline_log(paths, "exit-prover", "terminal proof ready");

        fs::read(&calldata_file).map_err(|e| format!("missing calldata.bin: {e}"))?
    };

    // Encode settle calldata
    let tx_data = encode_settle_calldata(&calldata_bytes, &a.fields, &b.fields);

    let pool_file = paths.pool_deployment();
    let pool_json: Value =
        serde_json::from_str(&fs::read_to_string(&pool_file).map_err(|e| format!("cannot read {}: {e}", pool_file.display()))?)
            .map_err(|e| e.to_string())?;

    let pool_addr_str = pool_json["pool"].as_str().ok_or("missing pool address")?;
    let pool_addr_clean = pool_addr_str.trim().trim_start_matches("0x");
    let pool_addr_bytes = hex::decode(pool_addr_clean).map_err(|e| format!("invalid pool hex: {e}"))?;
    let mut pool_addr = [0u8; 20];
    pool_addr.copy_from_slice(&pool_addr_bytes);

    let paymaster_str = pool_json["paymaster"].as_str().ok_or("missing paymaster address")?;
    let paymaster_clean = paymaster_str.trim().trim_start_matches("0x");
    let paymaster_bytes = hex::decode(paymaster_clean).map_err(|e| format!("invalid paymaster hex: {e}"))?;
    let mut paymaster_addr = [0u8; 20];
    paymaster_addr.copy_from_slice(&paymaster_bytes);

    let rpc_url = paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz");
    let pq_key_file = paths.p(&paths.setting("EXIT_PQ_KEY", ".secrets/pq/exit.key"));
    if !pq_key_file.exists() {
        return Err(format!("missing PQ exit key at {}", pq_key_file.display()));
    }

    log("constructing EIP-8141 3-frame settle transaction [VERIFY pool, VERIFY paymaster, SENDER settle]...");

    // Generate random 128-bit nonce key
    let mut nonce_bytes = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce_key = u128::from_be_bytes(nonce_bytes);

    let zero4 = vec![0u8; 4];
    let frames = vec![
        // Frame 0: VERIFY pool (EXECUTION approval = 2)
        crypto::Frame {
            mode: 1,  // VERIFY
            flags: 2, // APPROVE.EXECUTION
            target: Some(pool_addr),
            gas_limit: 10_000,
            state_limit: 0,
            value: 0,
            data: zero4.clone(),
        },
        // Frame 1: VERIFY paymaster (PAYMENT approval = 1)
        crypto::Frame {
            mode: 1,  // VERIFY
            flags: 1, // APPROVE.PAYMENT
            target: Some(paymaster_addr),
            gas_limit: 150_000,
            state_limit: 110_000,
            value: 0,
            data: zero4,
        },
        // Frame 2: SENDER pool settle
        crypto::Frame {
            mode: 2, // SENDER
            flags: 0,
            target: Some(pool_addr),
            gas_limit: 14_600_000,
            state_limit: 5_000_000,
            value: 0,
            data: tx_data,
        },
    ];

    // Query base fee from latest block to set exact fee caps matching paymaster budget
    let block_val = rpc_call(&rpc_url, "eth_getBlockByNumber", json!(["latest", false]))?;
    let base_fee_hex = block_val["baseFeePerGas"].as_str().unwrap_or("0x7").trim_start_matches("0x");
    let base_fee = u128::from_str_radix(base_fee_hex, 16).unwrap_or(7);
    let max_priority_fee: u128 = 1_000_000_000;
    let max_fee: u128 = 2 * base_fee + max_priority_fee;

    let mut frame_tx = crypto::FrameTransaction {
        chain_id: pool_json["chainId"].as_u64().unwrap_or(8141),
        nonce_keys: vec![nonce_key],
        nonce_seq: 0,
        sender: pool_addr,
        frames,
        signatures: vec![],
        max_priority_fee,
        max_fee,
        max_blob_fee: 0,
        blob_hashes: vec![],
    };

    log("signing frame transaction with leanSPHINCS PQ key via lsk...");
    let raw_tx = frame_tx.sign_with_lsk(&paths.setting("LSK_BIN", "lsk"), &pq_key_file)?;
    let raw_hex = format!("0x{}", hex::encode(&raw_tx));

    // Dry run on the node first: a settle the chain would refuse never leaves the exit.
    let sim = rpc_call(&rpc_url, "ethrex_simulateFrameTransaction", json!([raw_hex]))?;
    if sim["valid"] != json!(true) || sim["executionError"].is_string() {
        // The commonest cause: the paymaster cannot cover the settle's maximum cost.
        let wei = |v: &Value| v.as_str().and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok());
        let max_cost = wei(&sim["maxCost"]).unwrap_or(0);
        let balance =
            rpc_call(&rpc_url, "eth_getBalance", json!([paymaster_str, "latest"])).ok().and_then(|v| wei(&v)).unwrap_or(u128::MAX);
        if balance < max_cost {
            return Err(format!(
                "the paymaster {paymaster_str} holds {:.4} ETH but a settle may cost up to {:.4} ETH: send it some ETH",
                balance as f64 / 1e18,
                max_cost as f64 / 1e18
            ));
        }
        return Err(format!("settle refused in simulation: {}", sim));
    }
    log(&format!("simulation ok: {} gas", sim["gasUsed"].as_str().unwrap_or("?")));

    // An ERC-8421 alternative mempool takes the transaction as JSON (eth_sendFrameTransaction) and runs
    // its own admission rules; raw bytes sent to it would be redirected to its node.
    let tx_hash_val = match mempool {
        Some(url) => rpc_call(url, "eth_sendFrameTransaction", json!([frame_tx.to_request_json()]))?,
        None => rpc_call(&rpc_url, "eth_sendRawTransaction", json!([raw_hex]))?,
    };
    let tx_hash = tx_hash_val.as_str().ok_or("no tx hash returned")?;
    match mempool {
        Some(url) => log(&format!("settle {tx_hash} admitted by the alt mempool at {url}; waiting for it to settle...")),
        None => log(&format!("settle frame tx broadcast: {tx_hash}, awaiting receipt...")),
    }

    // A settle in the mempool may wait behind others from the same pool sender.
    let start_wait = Instant::now();
    let mut receipt: Option<Value> = None;
    while start_wait.elapsed() < Duration::from_secs(300) {
        std::thread::sleep(Duration::from_millis(1000));
        if let Ok(rc) = rpc_call(&rpc_url, "eth_getTransactionReceipt", json!([tx_hash]))
            && !rc.is_null()
            && rc.get("blockNumber").is_some()
        {
            receipt = Some(rc);
            break;
        }
    }

    let rc = receipt.ok_or("timeout waiting for settle transaction receipt")?;
    if rc["status"].as_str() != Some("0x1") {
        return Err(format!("settle transaction reverted: {tx_hash}"));
    }

    let block_num = u64::from_str_radix(rc["blockNumber"].as_str().unwrap_or("0x0").trim_start_matches("0x"), 16).unwrap_or(0);

    // Move settled spends to done/
    let done_dir = queue_dir.join("done");
    fs::create_dir_all(&done_dir).map_err(|e| e.to_string())?;

    let _ = fs::rename(&a.fields_file, done_dir.join(format!("{}.json", a.id)));
    let _ = fs::rename(&a.proof_file, done_dir.join(format!("{}.tgz", a.id)));
    let _ = fs::rename(&b.fields_file, done_dir.join(format!("{}.json", b.id)));
    let _ = fs::rename(&b.proof_file, done_dir.join(format!("{}.tgz", b.id)));

    log(&format!("settled pair {} + {} in block {block_num} ({tx_hash})", a.id, b.id));
    pipeline_log(paths, "exit-prover", &format!("settled {} + {} in block {block_num}", a.id, b.id));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_like_the_sdk() {
        // packDigest([1..8]) = sum (i+1) << 32i: limb 0 in the lowest bits, i.e. last in big-endian hex.
        let mut fields = vec![0u32; 82];
        for i in 0..8 {
            fields[NULLIFIER + i] = i as u32 + 1;
        }
        assert_eq!(packed_word_hex(&fields, NULLIFIER), "0000000800000007000000060000000500000004000000030000000200000001");
    }
}
