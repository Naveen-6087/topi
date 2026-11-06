//! Test vectors for the shielded pool's Solidity hashing (test/fixtures/vectors.json). Copy into a spartan-whir
//! checkout as examples/vectors.rs and run:
//!   cargo run --release --features parallel,poseidon1 --example vectors -- FIXTURE_A FIXTURE_B > vectors.json
//! Prints JSON with:
//! - a Poseidon1-16 permutation vector (input 0..15),
//! - each fixture's public inputs and statement digest (spartan-whir's own function),
//! - the binary application root (leanVM's formula: overwrite sponge over [NOD1, 2, 0x6, A, B]),
//! - a Poseidon2-24 `hash_fields` vector (the withdrawal tree's node hash).
use std::{env, fs};

use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_koala_bear::{default_koalabear_poseidon1_16, KoalaBear};
use p3_symmetric::Permutation;
use serde_json::{json, Value};
use spartan_whir::{engine::Poseidon1QuinticEngine, full_zk_statement_digest, withdrawal};

type F = KoalaBear;

const NOD1: u32 = 1_313_817_649;

fn u32s(values: &[F]) -> Vec<u32> {
    values.iter().map(|v| v.as_canonical_u32()).collect()
}

fn hash_slice(data: &[F]) -> [F; 8] {
    // leanVM symetric::poseidon_hash_slice: state[0] = len; overwrite state[8..16] per 8-chunk; permute.
    assert!(!data.is_empty() && data.len() % 8 == 0);
    let permutation = default_koalabear_poseidon1_16();
    let mut state = [F::ZERO; 16];
    state[0] = F::from_usize(data.len());
    for chunk in data.chunks(8) {
        state[8..].copy_from_slice(chunk);
        permutation.permute_mut(&mut state);
    }
    state[8..].try_into().unwrap()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dirs: Vec<String> = env::args().skip(1).collect();
    assert_eq!(dirs.len(), 2, "usage: vectors FIXTURE_A FIXTURE_B");

    let permutation = default_koalabear_poseidon1_16();
    let mut state: [F; 16] = std::array::from_fn(|i| F::from_usize(i));
    permutation.permute_mut(&mut state);

    let mut statements = Vec::new();
    let mut digests = Vec::new();
    for dir in &dirs {
        let file: Value = serde_json::from_str(&fs::read_to_string(format!("{dir}/full_zk_spark_statement.json"))?)?;
        let inputs: Vec<F> = file["inner_public_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| F::from_u32(v.as_u64().unwrap() as u32))
            .collect();
        let digest = full_zk_statement_digest::<Poseidon1QuinticEngine>(&inputs);
        let recorded: Vec<u32> = file["digest"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
        assert_eq!(u32s(&digest), recorded, "statement digest disagrees with the fixture");
        statements.push(json!({ "dir": dir, "public_inputs": u32s(&inputs), "digest": u32s(&digest) }));
        digests.push(digest);
    }

    let mut preimage = vec![F::from_u32(NOD1), F::from_u32(2)];
    preimage.resize(8, F::ZERO);
    preimage.extend_from_slice(&digests[0]);
    preimage.extend_from_slice(&digests[1]);
    let root = hash_slice(&preimage);

    let node_input: Vec<F> = (1..=16).map(|i| F::from_u32(i)).collect();
    let node = withdrawal::hash_fields(4, &node_input);

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "poseidon1_16_permutation": { "input": (0..16).collect::<Vec<u32>>(), "output": u32s(&state) },
            "statements": statements,
            "application_root": u32s(&root),
            "poseidon2_24_hash_fields": { "domain": 4, "input": u32s(&node_input), "output": u32s(&node) },
        }))?
    );
    Ok(())
}
