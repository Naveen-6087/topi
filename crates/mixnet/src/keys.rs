//! Node key files: the 32-byte X-Wing seed, hex.

use std::path::Path;

use topi_sphinx::{DecapsulationKey, EncapsulationKey, keygen, keypair_from_seed, sk_bytes};

use crate::{Error, Result};

/// Write a fresh key to `path` and return its public key.
pub fn generate(path: impl AsRef<Path>) -> Result<EncapsulationKey> {
    let (sk, pk) = keygen();
    std::fs::write(path, hex::encode(sk_bytes(&sk)))?;
    Ok(pk)
}

pub fn load(path: impl AsRef<Path>) -> Result<(DecapsulationKey, EncapsulationKey)> {
    let text = std::fs::read_to_string(path)?;
    let seed: [u8; 32] = hex::decode(text.trim())
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Error::Topology("a node key file holds a 32-byte hex seed".into()))?;
    Ok(keypair_from_seed(seed))
}
