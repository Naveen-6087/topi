//! # topi-ratelimit — PQ-RLN, design 1
//!
//! Post-quantum anonymous per-epoch rate limiting for mixnet packets.
//!
//! Once per epoch a client produces an [`EpochTicket`]: a token root `R` plus a Binius64
//! **zero-knowledge** proof (hash-based: binary-field sumcheck + BaseFold/FRI over SHA-256 Merkle
//! commitments, wrapped by Binius' Spartan ZK wrapper) of
//!
//! > I know `sk` such that `H("TPID"‖sk)` is a leaf of the membership tree with root `M`, and
//! > `R` is the Merkle root of the `L` tokens `t_i = H("TPTK"‖sk‖epoch‖i)`, `i ∈ 0..L`.
//!
//! with public inputs `(M, epoch, R)`. Each packet then carries one [`Token`] (`t_i` + its path
//! to `R`). Entry nodes verify the ticket once per `R` and accept each distinct token once.

mod circuit;
pub mod hash;
pub mod merkle;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
};

use binius_hash::StdHashSuite;
use binius_prover::{OptimalPackedB128, zk_config::ZKProver};
use binius_verifier::{
    config::StdChallenger,
    transcript::{ProverTranscript, VerifierTranscript},
    zk_config::ZKVerifier,
};
pub use hash::Digest;
pub use merkle::{MembershipTree, MerklePath};

use crate::circuit::EpochCircuit;

/// Default membership tree depth (2^16 members).
pub const DEFAULT_DEPTH: usize = 16;
/// Default tokens per epoch.
pub const DEFAULT_L: usize = 16;
/// log2 of the inverse Reed–Solomon rate used by BaseFold/FRI (rate 1/4; smallest proofs measured).
pub const LOG_INV_RATE: usize = 2;

/// Public parameters of a deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Params {
    /// Membership tree depth.
    pub depth: usize,
    /// Tokens per epoch (power of two, >= 2).
    pub l: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self { depth: DEFAULT_DEPTH, l: DEFAULT_L }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("statement does not hold (bad path / root / identity): {0}")]
    Unsatisfied(String),
    #[error("bad parameters: {0}")]
    Params(String),
    #[error("proving failed: {0}")]
    Prove(String),
}

// ---------------------------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------------------------

/// A client's long-term secret.
#[derive(Clone)]
pub struct Identity {
    sk: [u8; 32],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Identity(..)")
    }
}

impl Identity {
    /// Fresh identity from the OS CSPRNG.
    pub fn random() -> Self {
        let mut sk = [0u8; 32];
        getrandom::fill(&mut sk).expect("OS RNG failure");
        Self { sk }
    }

    pub fn from_secret(sk: [u8; 32]) -> Self {
        Self { sk }
    }

    /// Public commitment `H("TPID" ‖ sk)` to register in the membership tree.
    pub fn commitment(&self) -> Digest {
        hash::leaf(&self.sk)
    }
}

// ---------------------------------------------------------------------------------------------
// Tickets & tokens
// ---------------------------------------------------------------------------------------------

/// Per-epoch ticket: token root `R` and the ZK proof bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochTicket {
    pub token_root: Digest,
    pub proof: Vec<u8>,
}

impl EpochTicket {
    /// Wire encoding: `R[32] ‖ proof`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = self.token_root.to_vec();
        v.extend_from_slice(&self.proof);
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 32 {
            return None;
        }
        Some(Self { token_root: b[..32].try_into().unwrap(), proof: b[32..].to_vec() })
    }
}

/// A per-packet token: `t_i`, its index `i`, and its path to `R`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub t: Digest,
    pub index: u32,
    pub path: Vec<Digest>,
}

impl Token {
    /// Wire encoding: `t[32] ‖ i:u32be ‖ path[log2 L][32]`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = self.t.to_vec();
        v.extend_from_slice(&self.index.to_be_bytes());
        for p in &self.path {
            v.extend_from_slice(p);
        }
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 36 || !(b.len() - 36).is_multiple_of(32) {
            return None;
        }
        Some(Self {
            t: b[..32].try_into().unwrap(),
            index: u32::from_be_bytes(b[32..36].try_into().unwrap()),
            path: b[36..].chunks(32).map(|c| c.try_into().unwrap()).collect(),
        })
    }

    pub fn encoded_len(&self) -> usize {
        36 + 32 * self.path.len()
    }
}

fn token_tree(sk: &[u8; 32], epoch: u64, l: usize) -> Vec<Vec<Digest>> {
    merkle::full_tree((0..l as u32).map(|i| hash::token(sk, epoch, i)).collect())
}

/// The token root `R` an identity commits to for `epoch`.
pub fn token_root(id: &Identity, epoch: u64, l: usize) -> Digest {
    *token_tree(&id.sk, epoch, l).last().unwrap().first().unwrap()
}

/// Token `i` of `epoch`. Returns `None` for `i >= L`: no path to `R` exists for such an index.
pub fn token(id: &Identity, epoch: u64, i: u32, l: usize) -> Option<Token> {
    if !l.is_power_of_two() || (i as usize) >= l {
        return None;
    }
    let levels = token_tree(&id.sk, epoch, l);
    let path = (0..levels.len() - 1).map(|k| levels[k][((i as usize) >> k) ^ 1]).collect();
    Some(Token { t: levels[0][i as usize], index: i, path })
}

/// Checks that `token` is a leaf of the token tree with root `R` (any `L` = 2^path.len()).
pub fn verify_token(r: &Digest, token: &Token) -> bool {
    if token.path.len() >= 32 || (token.index as u64) >> token.path.len() != 0 {
        return false;
    }
    let p = MerklePath { index: token.index as u64, siblings: token.path.clone() };
    &p.root_from(&token.t) == r
}

/// As [`verify_token`] but also enforces the deployment's `L`.
pub fn verify_token_with_l(r: &Digest, token: &Token, l: usize) -> bool {
    l.is_power_of_two() && token.path.len() == l.trailing_zeros() as usize && verify_token(r, token)
}

// ---------------------------------------------------------------------------------------------
// Proof system
// ---------------------------------------------------------------------------------------------

type Zkv = ZKVerifier<StdHashSuite>;
type Zkp = ZKProver<OptimalPackedB128, StdHashSuite>;

/// Compiled circuit + ZK prover/verifier keys for one [`Params`]. Expensive to build; reuse it.
pub struct RlnSystem {
    params: Params,
    circuit: EpochCircuit,
    verifier: Zkv,
    prover: OnceLock<Zkp>,
}

impl RlnSystem {
    /// Builds the circuit and the ZK verifier (the prover key is built lazily on first prove).
    pub fn new(params: Params) -> Result<Self, Error> {
        if !params.l.is_power_of_two() || params.l < 2 || params.l > (1 << 20) {
            return Err(Error::Params("L must be a power of two in [2, 2^20]".into()));
        }
        if params.depth == 0 || params.depth > 40 {
            return Err(Error::Params("depth must be in [1, 40]".into()));
        }
        let circuit = EpochCircuit::build(params.depth, params.l);
        let verifier = Zkv::setup(circuit.circuit.constraint_system().clone(), LOG_INV_RATE).map_err(|e| Error::Params(e.to_string()))?;
        Ok(Self { params, circuit, verifier, prover: OnceLock::new() })
    }

    pub fn params(&self) -> Params {
        self.params
    }

    /// Number of AND constraints in the inner Binius64 constraint system.
    pub fn n_and_constraints(&self) -> usize {
        self.circuit.circuit.constraint_system().n_and_constraints()
    }

    /// Forces prover-key construction (otherwise done on first [`Self::prove_epoch`]).
    pub fn prover_setup(&self) {
        self.prover();
    }

    fn prover(&self) -> &Zkp {
        self.prover.get_or_init(|| Zkp::setup(&self.verifier).expect("ZK prover setup"))
    }

    /// Produces this epoch's ticket. `root` must be the root `path` leads to.
    pub fn prove_epoch(&self, id: &Identity, path: &MerklePath, root: &Digest, epoch: u64) -> Result<EpochTicket, Error> {
        if path.depth() != self.params.depth {
            return Err(Error::Params(format!("path depth {} != system depth {}", path.depth(), self.params.depth)));
        }
        let r = token_root(id, epoch, self.params.l);
        let witness = self.circuit.witness(&id.sk, path.index, &path.siblings, root, epoch, &r).map_err(Error::Unsatisfied)?;
        let mut transcript = ProverTranscript::new(StdChallenger::default());
        // ZK mode: ZKProver commits with random masks/blinding drawn from this CSPRNG.
        let mut rng = rand::rng();
        self.prover().prove(&witness, &mut rng, &mut transcript).map_err(|e| Error::Prove(e.to_string()))?;
        Ok(EpochTicket { token_root: r, proof: transcript.finalize() })
    }

    /// Verifies a ticket against membership root `M` and `epoch`.
    pub fn verify_epoch(&self, m: &Digest, epoch: u64, ticket: &EpochTicket) -> bool {
        let inout = self.circuit.public_inputs(m, epoch, &ticket.token_root);
        let proof = ticket.proof.clone();
        // Treat any panic on malformed input as rejection.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut t = VerifierTranscript::new(StdChallenger::default(), proof);
            self.verifier.verify(&inout, &mut t).is_ok() && t.finalize().is_ok()
        }))
        .unwrap_or(false)
    }
}

/// Process-wide cache of systems by parameters.
pub fn system(params: Params) -> Result<Arc<RlnSystem>, Error> {
    static CACHE: OnceLock<Mutex<HashMap<Params, Arc<RlnSystem>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().unwrap().get(&params) {
        return Ok(s.clone());
    }
    let s = Arc::new(RlnSystem::new(params)?);
    Ok(cache.lock().unwrap().entry(params).or_insert(s).clone())
}

/// Convenience: prove with depth = `path.depth()` and the given `L`.
pub fn prove_epoch(id: &Identity, path: &MerklePath, root: &Digest, epoch: u64, l: usize) -> Result<EpochTicket, Error> {
    system(Params { depth: path.depth(), l })?.prove_epoch(id, path, root, epoch)
}

/// Convenience: verify under the default parameters (depth 16, L 16).
pub fn verify_epoch(m: &Digest, epoch: u64, ticket: &EpochTicket) -> bool {
    verify_epoch_with(Params::default(), m, epoch, ticket)
}

pub fn verify_epoch_with(params: Params, m: &Digest, epoch: u64, ticket: &EpochTicket) -> bool {
    match system(params) {
        Ok(s) => s.verify_epoch(m, epoch, ticket),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Entry-node rate limiter
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptResult {
    Accepted,
    Duplicate,
    OverLimit,
    Invalid,
}

/// Either a full ticket (verified on first sight) or a bare `R` that must already be known.
#[derive(Clone, Copy, Debug)]
pub enum TicketRef<'a> {
    Ticket(&'a EpochTicket),
    Root(&'a Digest),
}

impl<'a> From<&'a EpochTicket> for TicketRef<'a> {
    fn from(t: &'a EpochTicket) -> Self {
        TicketRef::Ticket(t)
    }
}

impl<'a> From<&'a Digest> for TicketRef<'a> {
    fn from(r: &'a Digest) -> Self {
        TicketRef::Root(r)
    }
}

/// Entry-node state for one (membership root, epoch).
pub struct RateLimiter {
    system: Arc<RlnSystem>,
    membership_root: Digest,
    epoch: u64,
    /// Verified `R` -> tokens already used under it.
    used: HashMap<Digest, HashSet<Digest>>,
    /// `R`s whose ticket failed verification (cache to avoid re-verifying garbage).
    rejected: HashSet<Digest>,
}

impl RateLimiter {
    pub fn new(system: Arc<RlnSystem>, membership_root: Digest, epoch: u64) -> Self {
        Self { system, membership_root, epoch, used: HashMap::new(), rejected: HashSet::new() }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Moves to a new epoch / membership root, dropping all per-R state.
    pub fn reset(&mut self, membership_root: Digest, epoch: u64) {
        self.membership_root = membership_root;
        self.epoch = epoch;
        self.used.clear();
        self.rejected.clear();
    }

    /// Verifies and registers a ticket. Returns whether `R` is now known-good.
    pub fn register(&mut self, ticket: &EpochTicket) -> bool {
        let r = ticket.token_root;
        if self.used.contains_key(&r) {
            return true;
        }
        // NB: `rejected` is only a cache of bad (R, proof) pairs keyed by R; a different (valid)
        // proof for the same R is still accepted.
        if self.system.verify_epoch(&self.membership_root, self.epoch, ticket) {
            self.rejected.remove(&r);
            self.used.insert(r, HashSet::new());
            true
        } else {
            self.rejected.insert(r);
            false
        }
    }

    pub fn accept<'a>(&mut self, ticket: impl Into<TicketRef<'a>>, token: &Token) -> AcceptResult {
        let r = match ticket.into() {
            TicketRef::Ticket(t) => {
                if !self.register(t) {
                    return AcceptResult::Invalid;
                }
                t.token_root
            }
            TicketRef::Root(r) => *r,
        };
        let l = self.system.params.l;
        let Some(used) = self.used.get_mut(&r) else {
            return AcceptResult::Invalid;
        };
        if !verify_token_with_l(&r, token, l) {
            return AcceptResult::Invalid;
        }
        if used.contains(&token.t) {
            return AcceptResult::Duplicate;
        }
        // Unreachable with valid tokens (only L leaves exist under R); kept as defence in depth.
        if used.len() >= l {
            return AcceptResult::OverLimit;
        }
        used.insert(token.t);
        AcceptResult::Accepted
    }

    /// Number of tokens consumed under `R`.
    pub fn used_count(&self, r: &Digest) -> usize {
        self.used.get(r).map_or(0, HashSet::len)
    }
}
