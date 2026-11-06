//! Wallet support for the privacy-pools withdrawal relation (lean statement).
//!
//! Everything here mirrors the circuit and the on-chain pool exactly:
//! - precommitment = hash_fields(PRECOMMIT = 1, nullifier || secret)
//! - trees: depth 32, zero leaf = [0; 8], node = hash_fields(domain, left || right) with the left/right
//!   order given by the index bit (pool tree domain 4, association tree domain 5), as ShieldedPool inserts
//! - JSON numbers are canonical KoalaBear values (u32 < p); amounts are decimal strings.
use std::fs;

use p3_field::{PrimeCharacteristicRing, PrimeField32};
use serde_json::{json, Value};

use crate::engine::F;
use crate::withdrawal::{hash_fields, WithdrawalDigest, WithdrawalStatement, WithdrawalWitness, WITHDRAWAL_TREE_DEPTH};

const PRECOMMIT: u32 = 1;
const POOL_NODE: u32 = 4;
const ASSOCIATION_NODE: u32 = 5;
const MODULUS: u32 = 2_130_706_433;

pub type Error = Box<dyn std::error::Error>;

/// A uniformly random canonical digest from the OS (rejection sampling below the modulus).
pub fn random_digest() -> Result<WithdrawalDigest, Error> {
    let mut out = [F::ZERO; 8];
    let mut file = fs::File::open("/dev/urandom")?;
    let mut filled = 0;
    while filled < 8 {
        let mut b = [0u8; 4];
        std::io::Read::read_exact(&mut file, &mut b)?;
        let v = u32::from_le_bytes(b) & 0x7fff_ffff;
        if v < MODULUS {
            out[filled] = F::from_u32(v);
            filled += 1;
        }
    }
    Ok(out)
}

pub fn precommitment(nullifier: &WithdrawalDigest, secret: &WithdrawalDigest) -> WithdrawalDigest {
    hash_fields(PRECOMMIT, &[*nullifier, *secret].concat())
}

pub fn digest_json(d: &WithdrawalDigest) -> Value {
    json!(d.iter().map(|x| x.as_canonical_u32()).collect::<Vec<_>>())
}

pub fn digest_from(v: &Value) -> Result<WithdrawalDigest, Error> {
    let a = v.as_array().ok_or("digest must be an array")?;
    if a.len() != 8 {
        return Err("digest must have 8 elements".into());
    }
    let mut d = [F::ZERO; 8];
    for (i, x) in a.iter().enumerate() {
        let x = x.as_u64().ok_or("digest element must be a number")?;
        if x >= MODULUS as u64 {
            return Err("non-canonical field element".into());
        }
        d[i] = F::from_u32(x as u32);
    }
    Ok(d)
}

/// The pool packs a digest into one word, element i at bits 32i (PackedTree.pack).
pub fn digest_from_packed(hex_word: &str) -> Result<WithdrawalDigest, Error> {
    let s = hex_word.trim_start_matches("0x");
    let s = format!("{:0>64}", s);
    let mut d = [F::ZERO; 8];
    for i in 0..8 {
        // element i occupies bits 32i..32i+32, i.e. hex chars from the right
        let end = 64 - 8 * i;
        let v = u32::from_str_radix(&s[end - 8..end], 16)?;
        if v >= MODULUS {
            return Err("non-canonical packed element".into());
        }
        d[i] = F::from_u32(v);
    }
    Ok(d)
}

pub fn u128_from(v: &Value) -> Result<u128, Error> {
    Ok(v.as_str().ok_or("amount must be a decimal string")?.parse::<u128>()?)
}

/// Sibling path and root for `index` in a depth-32 tree whose first leaves are `leaves`.
pub fn path(domain: u32, leaves: &[WithdrawalDigest], index: usize) -> ([WithdrawalDigest; WITHDRAWAL_TREE_DEPTH], WithdrawalDigest) {
    let zero = [F::ZERO; 8];
    let mut zeros = [zero; WITHDRAWAL_TREE_DEPTH + 1];
    for level in 0..WITHDRAWAL_TREE_DEPTH {
        zeros[level + 1] = hash_fields(domain, &[zeros[level], zeros[level]].concat());
    }
    let mut nodes: Vec<WithdrawalDigest> = leaves.to_vec();
    let mut at = index;
    let mut siblings = [zero; WITHDRAWAL_TREE_DEPTH];
    for level in 0..WITHDRAWAL_TREE_DEPTH {
        siblings[level] = nodes.get(at ^ 1).copied().unwrap_or(zeros[level]);
        let mut next = Vec::with_capacity(nodes.len().div_ceil(2));
        for pair in nodes.chunks(2) {
            let right = pair.get(1).copied().unwrap_or(zeros[level]);
            next.push(hash_fields(domain, &[pair[0], right].concat()));
        }
        if next.is_empty() {
            next.push(zeros[level + 1]);
        }
        nodes = next;
        at >>= 1;
    }
    (siblings, nodes[0])
}

/// Recipient address -> ten little-endian 16-bit limbs.
pub fn address_limbs(addr: &str) -> Result<[u16; 10], Error> {
    let s = addr.trim_start_matches("0x");
    if s.len() != 40 {
        return Err("address must be 20 bytes".into());
    }
    let v = u128::from_str_radix(&s[8..], 16)?; // low 128 bits
    let hi = u64::from_str_radix(&s[..8], 16)?; // high 32 bits
    let mut limbs = [0u16; 10];
    for i in 0..8 {
        limbs[i] = (v >> (16 * i)) as u16;
    }
    limbs[8] = hi as u16;
    limbs[9] = (hi >> 16) as u16;
    Ok(limbs)
}

/// Fee recipient address -> context: six little-endian 28-bit limbs, then two zeros (ShieldedPool._contextAddress).
pub fn context_for(addr: &str) -> Result<WithdrawalDigest, Error> {
    let s = addr.trim_start_matches("0x");
    if s.len() != 40 {
        return Err("address must be 20 bytes".into());
    }
    let mut bits = [false; 168];
    for (nibble, c) in s.chars().rev().enumerate() {
        let n = c.to_digit(16).ok_or("bad hex")?;
        for b in 0..4 {
            bits[4 * nibble + b] = (n >> b) & 1 == 1;
        }
    }
    let mut d = [F::ZERO; 8];
    for i in 0..6 {
        let mut limb = 0u32;
        for b in 0..28 {
            if bits[28 * i + b] {
                limb |= 1 << b;
            }
        }
        d[i] = F::from_u32(limb);
    }
    Ok(d)
}

/// Build the (statement, witness) for one withdrawal from wallet and chain data, check the derived roots
/// against the chain's, and return them with the 82 transaction fields the pool's settle expects.
pub fn build(note: &Value, chain: &Value, request: &Value) -> Result<(WithdrawalStatement, WithdrawalWitness, Value), Error> {
    let pool = digest_from(&chain["pool"])?;
    let scope = digest_from(&chain["scope"])?;
    let pool_leaves: Vec<WithdrawalDigest> =
        chain["pool_leaves"].as_array().ok_or("pool_leaves")?.iter().map(digest_from).collect::<Result<_, _>>()?;
    let association_leaves: Vec<WithdrawalDigest> = chain["association_leaves"]
        .as_array()
        .ok_or("association_leaves")?
        .iter()
        .map(digest_from)
        .collect::<Result<_, _>>()?;
    let leaf_index = note["leaf_index"].as_u64().ok_or("leaf_index")? as usize;
    let label_index = note["label_index"].as_u64().ok_or("label_index")? as usize;
    let (pool_siblings, pool_root) = path(POOL_NODE, &pool_leaves, leaf_index);
    let (association_siblings, association_root) = path(ASSOCIATION_NODE, &association_leaves, label_index);
    if pool_root != digest_from(&chain["pool_root"])? {
        return Err("rebuilt pool root differs from the chain's".into());
    }
    if association_root != digest_from(&chain["association_root"])? {
        return Err("rebuilt association root differs from the chain's".into());
    }

    let balance = u128_from(&note["amount"])?;
    let withdrawn = u128_from(&request["withdrawn"])?;
    let fee = u128_from(&request["fee"])?;
    let change = balance.checked_sub(withdrawn).and_then(|x| x.checked_sub(fee)).ok_or("withdrawn + fee exceeds the note")?;
    let witness = WithdrawalWitness {
        balance,
        change,
        label: digest_from(&note["label"])?,
        nullifier: digest_from(&note["nullifier"])?,
        secret: digest_from(&note["secret"])?,
        new_nullifier: digest_from(&request["change_note"]["nullifier"])?,
        new_secret: digest_from(&request["change_note"]["secret"])?,
        pool_siblings,
        pool_index: leaf_index as u32,
        association_siblings,
        association_index: label_index as u32,
    };
    let mut statement = WithdrawalStatement {
        pool,
        scope,
        context: context_for(request["fee_recipient"].as_str().ok_or("fee_recipient")?)?,
        recipient: address_limbs(request["recipient"].as_str().ok_or("recipient")?)?,
        withdrawn,
        fee,
        pool_root: [F::ZERO; 8],
        association_root: [F::ZERO; 8],
        nullifier_hash: [F::ZERO; 8],
        change_commitment: [F::ZERO; 8],
        transaction_digest: [F::ZERO; 8],
    };
    statement.derive(&witness);
    if statement.pool_root != pool_root || statement.association_root != association_root {
        return Err("note is not the leaf at leaf_index (wrong secrets, label or amount)".into());
    }
    let out = json!({
        "fields": statement.lean_transaction_fields().iter().map(|x| x.as_canonical_u32()).collect::<Vec<_>>(),
        "tx_digest": digest_json(&statement.transaction_digest),
        "nullifier_hash": digest_json(&statement.nullifier_hash),
        "change_commitment": digest_json(&statement.change_commitment),
        "change": change.to_string(),
    });
    Ok((statement, witness, out))
}

/// The prover input file: wallet note + chain view + request, as written by `pool-wallet witness`.
pub fn load_input(path: &str) -> Result<(WithdrawalStatement, WithdrawalWitness), Error> {
    let v: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let (statement, witness, _) = build(&v["note"], &v["chain"], &v["request"])?;
    Ok((statement, witness))
}
