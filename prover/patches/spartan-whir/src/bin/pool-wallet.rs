//! pool-wallet: note keys and withdrawal inputs for the post-quantum pool (privacy-pools relation, lean statement).
//!
//!   pool-wallet keygen OUT.json
//!       fresh note secrets: {"nullifier", "secret", "precommitment"} (precommitment goes into deposit()).
//!   pool-wallet witness NOTE.json CHAIN.json REQUEST.json OUT.json
//!       NOTE    = keygen output + {"amount", "label", "leaf_index", "label_index"} from the Deposited event
//!       CHAIN   = {"pool", "scope", "pool_leaves", "association_leaves", "pool_root", "association_root"}
//!       REQUEST = {"recipient", "withdrawn", "fee", "fee_recipient", "change_note": keygen output}
//!       OUT     = the prover input ({"note","chain","request"}, for leanvm-full-zk-spark-fixture --input)
//!                 plus "public" = the 82 settle fields, transaction digest, nullifier hash, change commitment.
use std::{env, fs};

use serde_json::{json, Value};
use spartan_whir::pool_wallet::{build, digest_json, precommitment, random_digest};

fn read(path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 2 => {
            let nullifier = random_digest()?;
            let secret = random_digest()?;
            let out = json!({
                "nullifier": digest_json(&nullifier),
                "secret": digest_json(&secret),
                "precommitment": digest_json(&precommitment(&nullifier, &secret)),
            });
            fs::write(&args[1], serde_json::to_string_pretty(&out)?)?;
            println!("{}", out["precommitment"]);
        }
        Some("witness") if args.len() == 5 => {
            let (note, chain, request) = (read(&args[1])?, read(&args[2])?, read(&args[3])?);
            let (_, _, public) = build(&note, &chain, &request)?;
            let out = json!({ "note": note, "chain": chain, "request": request, "public": public });
            fs::write(&args[4], serde_json::to_string_pretty(&out)?)?;
            println!("witness ok: nullifier {} change {}", public["nullifier_hash"], public["change"]);
        }
        Some("check") if args.len() == 3 => {
            // check FIELDS.json FIXTURE_DIR: the 82 fields hash (TRANSACTION domain) to the lean statement's
            // only public input recorded in the client's proof fixture. The exit runs this before proving.
            let fields: Vec<u32> = serde_json::from_value(read(&args[1])?["fields"].clone())?;
            if fields.len() != 82 {
                return Err("need 82 fields".into());
            }
            let elems: Vec<spartan_whir::engine::F> =
                fields.iter().map(|&x| <spartan_whir::engine::F as p3_field::PrimeCharacteristicRing>::from_u32(x)).collect();
            let digest = spartan_whir::withdrawal::hash_fields(6, &elems);
            let statement = read(&format!("{}/full_zk_spark_statement.json", args[2]))?;
            let recorded: Vec<u32> = serde_json::from_value(statement["inner_public_inputs"].clone())?;
            let computed: Vec<u32> = digest.iter().map(|x| p3_field::PrimeField32::as_canonical_u32(x)).collect();
            if computed != recorded {
                println!("{}", json!({"ok": false, "reason": "fields do not match the proof's statement"}));
                std::process::exit(1);
            }
            println!("{}", json!({"ok": true, "tx_digest": computed}));
        }
        _ => {
            eprintln!("usage: pool-wallet keygen OUT.json | witness NOTE.json CHAIN.json REQUEST.json OUT.json | check FIELDS.json FIXTURE_DIR");
            std::process::exit(2);
        }
    }
    Ok(())
}
