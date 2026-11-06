//! Native (out-of-circuit) SHA-256 hashing. Every function here has an exact in-circuit twin in
//! [`crate::circuit`]; the two MUST stay byte-for-byte identical.
//!
//! All hashes are plain FIPS 180-4 SHA-256 (with standard padding):
//!
//! | name                | message (bytes)                                   | len | blocks |
//! |---------------------|---------------------------------------------------|-----|--------|
//! | `leaf(sk)`          | `"TPID" ‖ sk[32]`                                 | 36  | 1      |
//! | `token(sk, e, i)`   | `"TPTK" ‖ sk[32] ‖ e:u64be ‖ i:u32be`             | 48  | 1      |
//! | `node(l, r)`        | `l[32] ‖ r[32]`                                   | 64  | 2      |
//!
//! Domain separation: leaves/tokens have distinct 4-byte tags and lengths 36/48, nodes have
//! length 64, so the three are distinct SHA-256 messages; confusing them needs a SHA-256 collision.

use sha2::{Digest as _, Sha256};

/// A 32-byte SHA-256 digest.
pub type Digest = [u8; 32];

pub(crate) const TAG_LEAF: [u8; 4] = *b"TPID";
pub(crate) const TAG_TOKEN: [u8; 4] = *b"TPTK";

/// Identity commitment `H("TPID" ‖ sk)`.
pub fn leaf(sk: &[u8; 32]) -> Digest {
    let mut h = Sha256::new();
    h.update(TAG_LEAF);
    h.update(sk);
    h.finalize().into()
}

/// Rate-limiting token `t_i = H("TPTK" ‖ sk ‖ epoch ‖ i)`.
pub fn token(sk: &[u8; 32], epoch: u64, i: u32) -> Digest {
    let mut h = Sha256::new();
    h.update(TAG_TOKEN);
    h.update(sk);
    h.update(epoch.to_be_bytes());
    h.update(i.to_be_bytes());
    h.finalize().into()
}

/// Merkle node `H(left ‖ right)`.
pub fn node(left: &Digest, right: &Digest) -> Digest {
    let mut h = Sha256::new();
    h.update(left);
    h.update(right);
    h.finalize().into()
}

/// Big-endian u32 words of a digest (the SHA-256 state words).
pub(crate) fn to_words(d: &Digest) -> [u32; 8] {
    std::array::from_fn(|i| u32::from_be_bytes(d[4 * i..4 * i + 4].try_into().unwrap()))
}
