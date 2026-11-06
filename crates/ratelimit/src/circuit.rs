//! The Binius64 epoch circuit.
//!
//! Public (inout): `M[8]` (membership root, 8 BE u32 words), `epoch` (u64), `R[8]` (token root).
//! Private: `sk[8]`, membership siblings `sib[depth][8]`, direction words `dir[depth]`.
//!
//! Constraints (all SHA-256, matching [`crate::hash`] exactly):
//!   1. `leaf = SHA256("TPID" ‖ sk)`
//!   2. fold `leaf` up `depth` levels with `node(l, r) = SHA256(l ‖ r)` (order chosen by `dir`),
//!      assert result == `M`
//!   3. for i in 0..L: `t_i = SHA256("TPTK" ‖ sk ‖ epoch ‖ i)`
//!   4. fold `t_0..t_{L-1}` into a complete tree, assert root == `R`.

use binius_circuits::sha256::{State, sha256_compress};
use binius_core::word::Word;
use binius_frontend::{Circuit, CircuitBuilder, Wire, WitnessFiller};

use crate::hash::{self, Digest, TAG_LEAF, TAG_TOKEN};

pub(crate) struct EpochCircuit {
    pub circuit: Circuit,
    pub depth: usize,
    m: [Wire; 8],
    epoch: Wire,
    r: [Wire; 8],
    sk: [Wire; 8],
    siblings: Vec<([Wire; 8], Wire)>,
}

fn c32(b: &CircuitBuilder, v: u32) -> Wire {
    b.add_constant(Word(v as u64))
}

/// SHA-256 of a message that is exactly 64 bytes `l ‖ r`: compress the data block, then the
/// constant padding block (0x80, zeros, bit length 512).
fn node(b: &CircuitBuilder, l: &[Wire; 8], r: &[Wire; 8]) -> [Wire; 8] {
    let mut m = [l[0]; 16];
    m[..8].copy_from_slice(l);
    m[8..].copy_from_slice(r);
    let s1 = sha256_compress(b, State::iv(b), m);
    let mut pad = [c32(b, 0); 16];
    pad[0] = c32(b, 0x8000_0000);
    pad[15] = c32(b, 512);
    sha256_compress(b, s1, pad).0
}

impl EpochCircuit {
    pub fn build(depth: usize, l: usize) -> Self {
        assert!(l.is_power_of_two() && l >= 2, "L must be a power of two >= 2");
        let b = CircuitBuilder::new();
        // Declaration order of inout wires: M, epoch, R.
        let m: [Wire; 8] = std::array::from_fn(|_| b.add_inout());
        let epoch = b.add_inout();
        let r: [Wire; 8] = std::array::from_fn(|_| b.add_inout());
        let sk: [Wire; 8] = std::array::from_fn(|_| b.add_witness());
        let siblings: Vec<([Wire; 8], Wire)> = (0..depth).map(|_| (std::array::from_fn(|_| b.add_witness()), b.add_witness())).collect();

        let zero = c32(&b, 0);

        // 1. leaf = SHA256("TPID" || sk), 36 bytes -> one padded block.
        let mut blk = [zero; 16];
        blk[0] = c32(&b, u32::from_be_bytes(TAG_LEAF));
        blk[1..9].copy_from_slice(&sk);
        blk[9] = c32(&b, 0x8000_0000);
        blk[15] = c32(&b, 36 * 8);
        let mut cur = sha256_compress(&b, State::iv(&b), blk).0;

        // 2. membership path. `dir` MSB set  <=>  running node is the right child.
        for (sib, dir) in &siblings {
            let left: [Wire; 8] = std::array::from_fn(|j| b.select(*dir, sib[j], cur[j]));
            let right: [Wire; 8] = std::array::from_fn(|j| b.select(*dir, cur[j], sib[j]));
            cur = node(&b, &left, &right);
        }
        for j in 0..8 {
            b.assert_eq(format!("membership_root[{j}]"), cur[j], m[j]);
        }

        // 3. tokens t_i = SHA256("TPTK" || sk || epoch_be64 || i_be32), 48 bytes -> one block.
        let mask32 = b.add_constant(Word::MASK_32);
        let epoch_hi = b.shr(epoch, 32);
        let epoch_lo = b.band(epoch, mask32);
        let mut level: Vec<[Wire; 8]> = (0..l)
            .map(|i| {
                let mut blk = [zero; 16];
                blk[0] = c32(&b, u32::from_be_bytes(TAG_TOKEN));
                blk[1..9].copy_from_slice(&sk);
                blk[9] = epoch_hi;
                blk[10] = epoch_lo;
                blk[11] = c32(&b, i as u32);
                blk[12] = c32(&b, 0x8000_0000);
                blk[15] = c32(&b, 48 * 8);
                sha256_compress(&b, State::iv(&b), blk).0
            })
            .collect();

        // 4. token tree.
        while level.len() > 1 {
            level = level.chunks(2).map(|p| node(&b, &p[0], &p[1])).collect();
        }
        for j in 0..8 {
            b.assert_eq(format!("token_root[{j}]"), level[0][j], r[j]);
        }

        Self { circuit: b.build(), depth, m, epoch, r, sk, siblings }
    }

    fn fill_public(&self, w: &mut WitnessFiller<'_>, m: &Digest, epoch: u64, r: &Digest) {
        for (wire, v) in self.m.iter().zip(hash::to_words(m)) {
            w[*wire] = Word(v as u64);
        }
        w[self.epoch] = Word(epoch);
        for (wire, v) in self.r.iter().zip(hash::to_words(r)) {
            w[*wire] = Word(v as u64);
        }
    }

    /// The public-input vector (inout segment) for a statement, as the verifier sees it.
    pub fn public_inputs(&self, m: &Digest, epoch: u64, r: &Digest) -> Vec<Word> {
        let mut w = self.circuit.new_witness_filler();
        self.fill_public(&mut w, m, epoch, r);
        w.value_vec().inout().to_vec()
    }

    /// Full witness. Fails if the statement does not hold.
    pub fn witness(
        &self,
        sk: &[u8; 32],
        path_index: u64,
        siblings: &[Digest],
        m: &Digest,
        epoch: u64,
        r: &Digest,
    ) -> Result<binius_core::constraint_system::ValueVec, String> {
        assert_eq!(siblings.len(), self.depth);
        let mut w = self.circuit.new_witness_filler();
        self.fill_public(&mut w, m, epoch, r);
        for (wire, v) in self.sk.iter().zip(hash::to_words(sk)) {
            w[*wire] = Word(v as u64);
        }
        for (k, ((sw, dw), sib)) in self.siblings.iter().zip(siblings).enumerate() {
            for (wire, v) in sw.iter().zip(hash::to_words(sib)) {
                w[*wire] = Word(v as u64);
            }
            w[*dw] = if (path_index >> k) & 1 == 1 { Word::ALL_ONE } else { Word::ZERO };
        }
        self.circuit.populate_wire_witness(&mut w).map_err(|e| e.to_string())?;
        Ok(w.into_value_vec())
    }
}
