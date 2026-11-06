//! Binary SHA-256 Merkle trees: the membership tree and the per-epoch token tree.

use crate::hash::{self, Digest};

/// Value of an unoccupied membership slot. Nobody knows an `sk` with `H("TPID"‖sk) = 0^256`.
pub const EMPTY_LEAF: Digest = [0u8; 32];

/// An authentication path: siblings bottom-up and the leaf index (bit k = 1 means the running
/// node is the *right* child at level k).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerklePath {
    pub index: u64,
    pub siblings: Vec<Digest>,
}

impl MerklePath {
    pub fn depth(&self) -> usize {
        self.siblings.len()
    }

    /// Folds `leaf` up the path.
    pub fn root_from(&self, leaf: &Digest) -> Digest {
        let mut cur = *leaf;
        for (k, sib) in self.siblings.iter().enumerate() {
            cur = if (self.index >> k) & 1 == 0 { hash::node(&cur, sib) } else { hash::node(sib, &cur) };
        }
        cur
    }
}

/// Sparse fixed-depth membership tree over identity commitments. Unfilled slots are
/// [`EMPTY_LEAF`]; empty subtrees are represented by precomputed "zero hashes".
#[derive(Clone, Debug)]
pub struct MembershipTree {
    depth: usize,
    /// `levels[0]` = leaves, `levels[depth]` = [root]. Only the occupied prefix is stored.
    levels: Vec<Vec<Digest>>,
    zeros: Vec<Digest>,
}

impl MembershipTree {
    /// Builds a tree of the given depth from commitments (placed at indices 0..n).
    pub fn new(depth: usize, commitments: &[Digest]) -> Self {
        assert!(depth <= 40, "depth too large");
        assert!((commitments.len() as u128) <= (1u128 << depth), "too many commitments for depth {depth}");
        let mut zeros = vec![EMPTY_LEAF];
        for k in 0..depth {
            let z = zeros[k];
            zeros.push(hash::node(&z, &z));
        }
        let mut levels = vec![commitments.to_vec()];
        for k in 0..depth {
            let prev = &levels[k];
            let next: Vec<Digest> = prev.chunks(2).map(|c| hash::node(&c[0], c.get(1).unwrap_or(&zeros[k]))).collect();
            levels.push(next);
        }
        Self { depth, levels, zeros }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn len(&self) -> usize {
        self.levels[0].len()
    }

    pub fn is_empty(&self) -> bool {
        self.levels[0].is_empty()
    }

    pub fn root(&self) -> Digest {
        self.levels[self.depth].first().copied().unwrap_or(self.zeros[self.depth])
    }

    fn node_at(&self, level: usize, idx: u64) -> Digest {
        self.levels[level].get(idx as usize).copied().unwrap_or(self.zeros[level])
    }

    /// Authentication path for the member at `index`.
    pub fn path(&self, index: u64) -> Option<MerklePath> {
        if index as usize >= self.len() {
            return None;
        }
        let siblings = (0..self.depth).map(|k| self.node_at(k, (index >> k) ^ 1)).collect();
        Some(MerklePath { index, siblings })
    }

    /// Index of a commitment, if present.
    pub fn position(&self, commitment: &Digest) -> Option<u64> {
        self.levels[0].iter().position(|c| c == commitment).map(|i| i as u64)
    }
}

/// Complete binary tree over exactly `leaves.len()` (a power of two) leaves; returns all levels.
pub(crate) fn full_tree(leaves: Vec<Digest>) -> Vec<Vec<Digest>> {
    assert!(leaves.len().is_power_of_two());
    let mut levels = vec![leaves];
    while levels.last().unwrap().len() > 1 {
        let next = levels.last().unwrap().chunks(2).map(|c| hash::node(&c[0], &c[1])).collect();
        levels.push(next);
    }
    levels
}
