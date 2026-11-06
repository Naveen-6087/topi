//! Pure Rust Ethereum crypto helpers: secp256k1 keys, addresses, and EIP-1559 transaction signing.

use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

pub fn random_private_key() -> [u8; 32] {
    let signing_key = SigningKey::random(&mut rand_core::OsRng);
    signing_key.to_bytes().into()
}

pub fn private_key_from_hex(hex_str: &str) -> Result<[u8; 32], String> {
    let clean = hex_str.trim().trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| format!("invalid hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes for private key, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Derives the 20-byte Ethereum address from a 32-byte secp256k1 private key.
pub fn address_from_key(key_bytes: &[u8; 32]) -> Result<[u8; 20], String> {
    let signing_key = SigningKey::from_slice(key_bytes).map_err(|e| format!("invalid key: {e}"))?;
    let verifying_key = signing_key.verifying_key();
    let encoded_point = verifying_key.to_encoded_point(false);
    let pubkey_bytes = encoded_point.as_bytes();
    // pubkey_bytes is 65 bytes: 0x04 || X (32 bytes) || Y (32 bytes)
    if pubkey_bytes.len() != 65 || pubkey_bytes[0] != 0x04 {
        return Err("invalid uncompressed public key".into());
    }
    let mut hasher = Keccak256::new();
    hasher.update(&pubkey_bytes[1..]); // hash the 64 bytes of (X, Y)
    let hash = hasher.finalize();
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..32]);
    Ok(addr)
}

/// Converts a 20-byte address to an EIP-55 checksummed hex string (`0x...`).
pub fn checksum_address(addr: &[u8; 20]) -> String {
    let hex_addr = hex::encode(addr);
    let mut hasher = Keccak256::new();
    hasher.update(hex_addr.as_bytes());
    let hash = hasher.finalize();

    let mut result = String::with_capacity(42);
    result.push_str("0x");
    for (i, c) in hex_addr.chars().enumerate() {
        let hash_byte = hash[i / 2];
        let nibble = if i % 2 == 0 { hash_byte >> 4 } else { hash_byte & 0x0f };
        if nibble >= 8 {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

// ---------------- RLP Encoding Helpers ----------------

pub fn rlp_encode_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() == 1 && bytes[0] < 0x80 {
        vec![bytes[0]]
    } else if bytes.len() <= 55 {
        let mut out = Vec::with_capacity(1 + bytes.len());
        out.push(0x80 + bytes.len() as u8);
        out.extend_from_slice(bytes);
        out
    } else {
        let len_bytes = to_min_be_u64(bytes.len() as u64);
        let mut out = Vec::with_capacity(1 + len_bytes.len() + bytes.len());
        out.push(0xb7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(bytes);
        out
    }
}

pub fn rlp_encode_u128(val: u128) -> Vec<u8> {
    if val == 0 {
        vec![0x80] // RLP empty string represents 0 in Ethereum
    } else {
        let bytes = to_min_be_u128(val);
        rlp_encode_bytes(&bytes)
    }
}

pub fn rlp_encode_u64(val: u64) -> Vec<u8> {
    rlp_encode_u128(val as u128)
}

pub fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload_len: usize = items.iter().map(|it| it.len()).sum();
    let mut out = Vec::new();
    if payload_len <= 55 {
        out.push(0xc0 + payload_len as u8);
    } else {
        let len_bytes = to_min_be_u64(payload_len as u64);
        out.push(0xf7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

fn to_min_be_u64(v: u64) -> Vec<u8> {
    let be = v.to_be_bytes();
    let skip = be.iter().take_while(|&&b| b == 0).count();
    if skip == 8 { vec![0] } else { be[skip..].to_vec() }
}

fn to_min_be_u128(v: u128) -> Vec<u8> {
    let be = v.to_be_bytes();
    let skip = be.iter().take_while(|&&b| b == 0).count();
    if skip == 16 { vec![0] } else { be[skip..].to_vec() }
}

// ---------------- EIP-1559 Signing ----------------

#[derive(Clone, Debug)]
pub struct Eip1559Tx {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub value: u128,
    pub data: Vec<u8>,
}

impl Eip1559Tx {
    /// Produces the unsigned fields payload for RLP encoding
    fn fields_rlp(&self) -> Vec<Vec<u8>> {
        vec![
            rlp_encode_u64(self.chain_id),
            rlp_encode_u64(self.nonce),
            rlp_encode_u128(self.max_priority_fee_per_gas),
            rlp_encode_u128(self.max_fee_per_gas),
            rlp_encode_u64(self.gas_limit),
            rlp_encode_bytes(&self.to),
            rlp_encode_u128(self.value),
            rlp_encode_bytes(&self.data),
            rlp_encode_list(&[]), // empty access list
        ]
    }

    /// Computes keccak256(0x02 || rlp([fields...]))
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut payload = vec![0x02];
        payload.extend(rlp_encode_list(&self.fields_rlp()));
        let mut hasher = Keccak256::new();
        hasher.update(&payload);
        hasher.finalize().into()
    }

    /// Signs the transaction using secp256k1 and returns the raw hex transaction (`0x02...`).
    pub fn sign(&self, key_bytes: &[u8; 32]) -> Result<Vec<u8>, String> {
        let signing_key = SigningKey::from_slice(key_bytes).map_err(|e| format!("invalid key: {e}"))?;
        let hash = self.signing_hash();

        // Sign with recoverable signature (RFC6979)
        let (signature, recid) = signing_key.sign_prehash_recoverable(&hash).map_err(|e| format!("failed to sign prehash: {e}"))?;

        let r_bytes = signature.r().to_bytes();
        let s_bytes = signature.s().to_bytes();
        let y_parity = recid.to_byte() as u64;

        let mut all_fields = self.fields_rlp();
        all_fields.push(rlp_encode_u64(y_parity));
        // r and s are integers: RLP takes them without leading zero bytes (a 32-byte r that starts with
        // 0x00, about one signature in 256, is otherwise rejected as malformed).
        all_fields.push(rlp_encode_bytes(trim_leading_zeros(r_bytes.as_ref())));
        all_fields.push(rlp_encode_bytes(trim_leading_zeros(s_bytes.as_ref())));

        let mut raw = vec![0x02];
        raw.extend(rlp_encode_list(&all_fields));
        Ok(raw)
    }
}

// ---------------- EIP-8141 Frame Transaction (Type 0x06) ----------------

pub const FRAME_TX_TYPE: u8 = 0x06;

#[derive(Clone, Debug)]
pub struct Frame {
    pub mode: u64,  // 0: DEFAULT, 1: VERIFY, 2: SENDER
    pub flags: u64, // 1: PAYMENT, 2: EXECUTION
    pub target: Option<[u8; 20]>,
    pub gas_limit: u64,
    pub state_limit: u64,
    pub value: u128,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct FrameSignature {
    pub scheme: u64, // 0: ARBITRARY, 1: SECP256K1, 2: P256
    pub signer: Option<[u8; 20]>,
    pub msg: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct FrameTransaction {
    pub chain_id: u64,
    pub nonce_keys: Vec<u128>,
    pub nonce_seq: u64,
    pub sender: [u8; 20],
    pub frames: Vec<Frame>,
    pub signatures: Vec<FrameSignature>,
    pub max_priority_fee: u128,
    pub max_fee: u128,
    pub max_blob_fee: u128,
    pub blob_hashes: Vec<Vec<u8>>,
}

impl Frame {
    pub fn encode_rlp(&self) -> Vec<u8> {
        let target_bytes = self.target.map(|t| t.to_vec()).unwrap_or_default();
        rlp_encode_list(&[
            rlp_encode_u64(self.mode),
            rlp_encode_u64(self.flags),
            rlp_encode_bytes(&target_bytes),
            rlp_encode_list(&[rlp_encode_u64(self.gas_limit), rlp_encode_u64(self.state_limit)]),
            rlp_encode_u128(self.value),
            rlp_encode_bytes(&self.data),
        ])
    }
}

impl FrameSignature {
    pub fn encode_rlp(&self, elide_empty_msg_sig: bool) -> Vec<u8> {
        let signer_bytes = if self.scheme == 0 { vec![] } else { self.signer.map(|s| s.to_vec()).unwrap_or_default() };
        let sig_bytes = if elide_empty_msg_sig && self.msg.is_empty() { vec![] } else { self.signature.clone() };
        rlp_encode_list(&[
            rlp_encode_u64(self.scheme),
            rlp_encode_bytes(&signer_bytes),
            rlp_encode_bytes(&self.msg),
            rlp_encode_bytes(&sig_bytes),
        ])
    }
}

impl FrameTransaction {
    fn envelope(&self, elide_empty_msg_sig: bool) -> Vec<u8> {
        let nonce_keys_rlp: Vec<Vec<u8>> = self.nonce_keys.iter().map(|&k| rlp_encode_u128(k)).collect();
        let frames_rlp: Vec<Vec<u8>> = self.frames.iter().map(|f| f.encode_rlp()).collect();
        let sigs_rlp: Vec<Vec<u8>> = self.signatures.iter().map(|s| s.encode_rlp(elide_empty_msg_sig)).collect();
        let fees_rlp = vec![rlp_encode_u128(self.max_priority_fee), rlp_encode_u128(self.max_fee), rlp_encode_u128(self.max_blob_fee)];
        let blobs_rlp: Vec<Vec<u8>> = self.blob_hashes.iter().map(|b| rlp_encode_bytes(b)).collect();

        rlp_encode_list(&[
            rlp_encode_u64(self.chain_id),
            rlp_encode_list(&nonce_keys_rlp),
            rlp_encode_u64(self.nonce_seq),
            rlp_encode_bytes(&self.sender),
            rlp_encode_list(&frames_rlp),
            rlp_encode_list(&sigs_rlp),
            rlp_encode_list(&fees_rlp),
            rlp_encode_list(&blobs_rlp),
        ])
    }

    /// Computes keccak256(0x06 || envelope with empty-msg signatures' bytes elided)
    pub fn signature_hash(&self) -> [u8; 32] {
        let mut payload = vec![FRAME_TX_TYPE];
        payload.extend(self.envelope(true));
        let mut hasher = Keccak256::new();
        hasher.update(&payload);
        hasher.finalize().into()
    }

    /// The bytes eth_sendRawTransaction takes: 0x06 || envelope (full, non-elided).
    pub fn encode_raw(&self) -> Vec<u8> {
        let mut out = vec![FRAME_TX_TYPE];
        out.extend(self.envelope(false));
        out
    }

    /// Signs the transaction with a leanSPHINCS PQ key via `<lsk> sign-key` (the LSK_BIN binary) and fills
    /// the ARBITRARY witness.
    pub fn sign_with_lsk(&mut self, lsk_bin: &str, lsk_key_file: &std::path::Path) -> Result<Vec<u8>, String> {
        // Append placeholder if needed
        let need_placeholder = match self.signatures.last() {
            Some(last) => last.scheme != 0 || !last.msg.is_empty() || !last.signature.is_empty(),
            None => true,
        };
        if need_placeholder {
            self.signatures.push(FrameSignature {
                scheme: 0, // ARBITRARY
                signer: None,
                msg: vec![],
                signature: vec![],
            });
        }

        let sig_hash = self.signature_hash();
        let sig_hash_hex = hex::encode(sig_hash);

        let out = std::process::Command::new(lsk_bin)
            .args(["sign-key", &lsk_key_file.display().to_string(), &sig_hash_hex])
            .output()
            .map_err(|e| format!("failed to invoke lsk ({lsk_bin}; set LSK_BIN): {e}"))?;

        if !out.status.success() {
            return Err(format!("lsk sign-key failed: {}", String::from_utf8_lossy(&out.stderr)));
        }

        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let hex_str = stdout.trim_start_matches("0x");
        if hex_str.len() < 192 {
            return Err("lsk output too short".into());
        }

        // Output format is abi.encode(bytes32 pkSeed, bytes32 pkRoot, bytes sig)
        // Offset of bytes sig is at hex chars 128..192
        let offset = usize::from_str_radix(&hex_str[128..192], 16).map_err(|e| format!("invalid sig offset: {e}"))? * 2;
        if hex_str.len() < offset + 64 {
            return Err("lsk output corrupted at offset".into());
        }
        let length = usize::from_str_radix(&hex_str[offset..offset + 64], 16).map_err(|e| format!("invalid sig length: {e}"))?;
        let sig_hex = &hex_str[offset + 64..offset + 64 + 2 * length];
        let signature_bytes = hex::decode(sig_hex).map_err(|e| format!("invalid hex in signature: {e}"))?;

        let last_idx = self.signatures.len() - 1;
        self.signatures[last_idx].signature = signature_bytes;

        Ok(self.encode_raw())
    }
}

/// An integer's big-endian bytes without leading zeros, as RLP encodes integers.
fn trim_leading_zeros(bytes: &[u8]) -> &[u8] {
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[first..]
}

impl FrameTransaction {
    /// This transaction as `eth_sendFrameTransaction` takes it (ERC-8421 alternative mempools):
    /// quantities as hex, mode/flags/scheme as numbers, an ARBITRARY signer and an empty msg as "0x".
    /// The mempool re-encodes it to the same raw bytes as `encode_raw` (no EIP-8272 recent roots).
    pub fn to_request_json(&self) -> serde_json::Value {
        let q = |v: u128| format!("0x{v:x}");
        let addr = |a: &[u8; 20]| format!("0x{}", hex::encode(a));
        let bytes = |b: &[u8]| format!("0x{}", hex::encode(b));
        serde_json::json!({
            "chainId": q(self.chain_id as u128),
            "nonceKeys": self.nonce_keys.iter().map(|k| q(*k)).collect::<Vec<_>>(),
            "nonceSeq": q(self.nonce_seq as u128),
            "sender": addr(&self.sender),
            "frames": self.frames.iter().map(|f| serde_json::json!({
                "mode": f.mode,
                "flags": f.flags,
                "target": f.target.as_ref().map(addr),
                "limits": { "execution": q(f.gas_limit as u128), "state": q(f.state_limit as u128) },
                "value": q(f.value),
                "data": bytes(&f.data),
            })).collect::<Vec<_>>(),
            "signatures": self.signatures.iter().map(|s| serde_json::json!({
                "scheme": s.scheme,
                "signer": match (&s.signer, s.scheme) { (Some(a), sc) if sc != 0 => addr(a), _ => "0x".to_string() },
                "msg": bytes(&s.msg),
                "signature": bytes(&s.signature),
            })).collect::<Vec<_>>(),
            "fees": {
                "maxPriorityFeePerGas": q(self.max_priority_fee),
                "maxFeePerGas": q(self.max_fee),
                "maxFeePerBlobGas": q(self.max_blob_fee),
            },
            "blobVersionedHashes": self.blob_hashes.iter().map(|h| bytes(h)).collect::<Vec<_>>(),
            "recentRootReferences": [],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_derivation_known_vector() {
        // Known private key and expected Ethereum checksum address
        let priv_hex = "4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f360319";
        let key = private_key_from_hex(priv_hex).unwrap();
        let addr = address_from_key(&key).unwrap();
        let checksum = checksum_address(&addr);
        assert_eq!(checksum, "0xD75A5b50b9c65435e967bc83cCFee3a867a9071a");
    }

    #[test]
    fn test_eip1559_signing() {
        let key = random_private_key();
        let addr = address_from_key(&key).unwrap();
        let tx = Eip1559Tx {
            chain_id: 8141,
            nonce: 0,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 2_000_000_000,
            gas_limit: 21_000,
            to: addr,
            value: 10_000_000_000_000_000,
            data: vec![],
        };
        let raw = tx.sign(&key).unwrap();
        assert_eq!(raw[0], 0x02);
        assert!(raw.len() > 50);
    }

    #[test]
    fn test_frame_tx_vector() {
        let pool = hex::decode("66268c9e7735f331c48c30eecf2fe2e76b80a9d7").unwrap();
        let paymaster = hex::decode("98f4e0c284fbb848adccb4470f74d08054569ea4").unwrap();
        let mut pool_addr = [0u8; 20];
        pool_addr.copy_from_slice(&pool);
        let mut paymaster_addr = [0u8; 20];
        paymaster_addr.copy_from_slice(&paymaster);

        let fields = vec![0u32; 82];
        let settle_data = crate::prover_ops::encode_settle_calldata(&[1, 2, 3], &fields, &fields);

        let frames = vec![
            Frame { mode: 1, flags: 2, target: Some(pool_addr), gas_limit: 10_000, state_limit: 0, value: 0, data: vec![0u8; 4] },
            Frame {
                mode: 1,
                flags: 1,
                target: Some(paymaster_addr),
                gas_limit: 150_000,
                state_limit: 110_000,
                value: 0,
                data: vec![0u8; 4],
            },
            Frame {
                mode: 2,
                flags: 0,
                target: Some(pool_addr),
                gas_limit: 14_600_000,
                state_limit: 5_000_000,
                value: 0,
                data: settle_data,
            },
        ];

        let tx = FrameTransaction {
            chain_id: 8141,
            nonce_keys: vec![123456789],
            nonce_seq: 0,
            sender: pool_addr,
            frames: frames.clone(),
            signatures: vec![FrameSignature { scheme: 0, signer: None, msg: vec![], signature: vec![] }],
            max_priority_fee: 1_000_000_000,
            max_fee: 2_000_000_000,
            max_blob_fee: 0,
            blob_hashes: vec![],
        };

        let sig_hash = tx.signature_hash();
        let sig_hash_hex = hex::encode(sig_hash);
        let raw = tx.encode_raw();
        let raw_hex = hex::encode(&raw[..50]);

        println!("RUST SIG HASH: {sig_hash_hex}");
        println!("RUST RAW PREFIX: 0x{raw_hex}");

        let f0 = frames[0].encode_rlp();
        let f1 = frames[1].encode_rlp();
        let f2 = frames[2].encode_rlp();
        println!("Rust f0 len: {}, f1 len: {}, f2 len: {}", f0.len(), f1.len(), f2.len());
        println!("Rust f0 hex: {}", hex::encode(&f0));
        println!("Rust f1 hex: {}", hex::encode(&f1));
        println!("Rust f2 prefix: {}", hex::encode(&f2[..50]));

        assert_eq!(sig_hash_hex, "36d6f0af82bb288afee1310a2e8772597a2bdcf0ab2e35f61f4d27f210033774");
        assert_eq!(raw_hex, "06f9160a821fcdc584075bcd15809466268c9e7735f331c48c30eecf2fe2e76b80a9d7f915d5e201029466268c9e7735f331");
    }
}

#[cfg(test)]
mod request_json_tests {
    use super::*;

    #[test]
    fn rlp_integers_drop_leading_zeros() {
        assert_eq!(trim_leading_zeros(&[0, 0, 1, 2]), &[1, 2]);
        assert_eq!(trim_leading_zeros(&[0, 0]), &[] as &[u8]);
        assert_eq!(trim_leading_zeros(&[7, 0]), &[7, 0]);
    }

    /// A settle-shaped transaction as JSON and raw hex, for the SDK cross-check (TOPI_JSON_OUT=file).
    #[test]
    fn request_json_matches_raw() {
        let pool = [0x66u8; 20];
        let tx = FrameTransaction {
            chain_id: 8141,
            nonce_keys: vec![0x296d72e41d840fcf609249aa3ec8ac04],
            nonce_seq: 0,
            sender: pool,
            frames: vec![
                Frame { mode: 1, flags: 2, target: Some(pool), gas_limit: 10_000, state_limit: 0, value: 0, data: vec![0; 4] },
                Frame { mode: 1, flags: 1, target: Some([0x4d; 20]), gas_limit: 150_000, state_limit: 110_000, value: 0, data: vec![0; 4] },
                Frame {
                    mode: 2,
                    flags: 0,
                    target: Some(pool),
                    gas_limit: 14_600_000,
                    state_limit: 5_000_000,
                    value: 0,
                    data: vec![0xf4, 0xb5, 0x89, 0xc7, 1, 2, 3],
                },
            ],
            signatures: vec![FrameSignature { scheme: 0, signer: None, msg: vec![], signature: vec![7; 5684] }],
            max_priority_fee: 1_000_000_000,
            max_fee: 1_000_000_014,
            max_blob_fee: 0,
            blob_hashes: vec![],
        };
        let json = tx.to_request_json();
        assert_eq!(json["signatures"][0]["signer"], "0x");
        assert_eq!(json["frames"][1]["limits"]["state"], "0x1adb0");
        if let Ok(out) = std::env::var("TOPI_JSON_OUT") {
            let both = serde_json::json!({ "request": json, "raw": format!("0x{}", hex::encode(tx.encode_raw())) });
            std::fs::write(out, both.to_string()).unwrap();
        }
    }
}
