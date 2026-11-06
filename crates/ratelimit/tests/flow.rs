use std::sync::{Arc, OnceLock};

use topi_ratelimit::{
    AcceptResult, EpochTicket, Error, Identity, MembershipTree, Params, RateLimiter, RlnSystem, Token, hash, system, token, verify_epoch,
    verify_token, verify_token_with_l,
};

const EPOCH: u64 = 1_700_000_000;

struct Fixture {
    ids: Vec<Identity>,
    tree: MembershipTree,
    sys: Arc<RlnSystem>,
    /// Ticket of ids[3] for EPOCH.
    ticket: EpochTicket,
}

/// One shared default-parameter (depth 16, L 16) setup + proof for all tests.
fn fx() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let ids: Vec<Identity> = (0..10).map(|_| Identity::random()).collect();
        let cms: Vec<_> = ids.iter().map(Identity::commitment).collect();
        let tree = MembershipTree::new(16, &cms);
        let sys = system(Params::default()).unwrap();
        let ticket = sys.prove_epoch(&ids[3], &tree.path(3).unwrap(), &tree.root(), EPOCH).unwrap();
        Fixture { ids, tree, sys, ticket }
    })
}

#[test]
fn valid_flow() {
    let f = fx();
    let m = f.tree.root();
    assert!(verify_epoch(&m, EPOCH, &f.ticket));
    assert_eq!(f.ticket.token_root, topi_ratelimit::token_root(&f.ids[3], EPOCH, 16));

    let mut rl = RateLimiter::new(f.sys.clone(), m, EPOCH);
    for i in 0..16u32 {
        let t = token(&f.ids[3], EPOCH, i, 16).unwrap();
        assert_eq!(t.encoded_len(), 32 + 4 + 4 * 32);
        assert!(verify_token(&f.ticket.token_root, &t));
        let res = if i == 0 { rl.accept(&f.ticket, &t) } else { rl.accept(&f.ticket.token_root, &t) };
        assert_eq!(res, AcceptResult::Accepted, "token {i}");
    }
    assert_eq!(rl.used_count(&f.ticket.token_root), 16);
}

#[test]
fn ticket_bytes_roundtrip() {
    let f = fx();
    let b = f.ticket.to_bytes();
    assert_eq!(EpochTicket::from_bytes(&b).unwrap(), f.ticket);
    let t = token(&f.ids[3], EPOCH, 5, 16).unwrap();
    assert_eq!(Token::from_bytes(&t.to_bytes()).unwrap(), t);
}

#[test]
fn wrong_membership_root_rejected() {
    let f = fx();
    let other = MembershipTree::new(16, &[Identity::random().commitment()]);
    assert!(!verify_epoch(&other.root(), EPOCH, &f.ticket));
    let mut bad = f.tree.root();
    bad[31] ^= 1;
    assert!(!verify_epoch(&bad, EPOCH, &f.ticket));
    // And the RateLimiter configured with the wrong M rejects the ticket.
    let mut rl = RateLimiter::new(f.sys.clone(), other.root(), EPOCH);
    let t = token(&f.ids[3], EPOCH, 0, 16).unwrap();
    assert_eq!(rl.accept(&f.ticket, &t), AcceptResult::Invalid);
}

#[test]
fn prover_refuses_false_statements() {
    let f = fx();
    // Wrong root for a valid path.
    let r = f.sys.prove_epoch(&f.ids[3], &f.tree.path(3).unwrap(), &[7u8; 32], EPOCH);
    assert!(matches!(r, Err(Error::Unsatisfied(_))));
    // Non-member using a member's path.
    let outsider = Identity::random();
    let r = f.sys.prove_epoch(&outsider, &f.tree.path(3).unwrap(), &f.tree.root(), EPOCH);
    assert!(matches!(r, Err(Error::Unsatisfied(_))));
    // Member with the wrong index (path of someone else).
    let r = f.sys.prove_epoch(&f.ids[3], &f.tree.path(4).unwrap(), &f.tree.root(), EPOCH);
    assert!(matches!(r, Err(Error::Unsatisfied(_))));
}

#[test]
fn tampered_proof_rejected() {
    let f = fx();
    let m = f.tree.root();
    let n = f.ticket.proof.len();
    for pos in [0, 1, 17, n / 7, n / 3, n / 2, 2 * n / 3, n - 33, n - 1] {
        let mut t = f.ticket.clone();
        t.proof[pos] ^= 0x01;
        assert!(!verify_epoch(&m, EPOCH, &t), "flip at {pos} accepted");
    }
    let mut t = f.ticket.clone();
    t.proof.truncate(n - 1);
    assert!(!verify_epoch(&m, EPOCH, &t));
    let mut t = f.ticket.clone();
    t.proof.push(0);
    assert!(!verify_epoch(&m, EPOCH, &t));
    assert!(!verify_epoch(&m, EPOCH, &EpochTicket { token_root: f.ticket.token_root, proof: vec![] }));
    // Swapping in another R (e.g. trying to claim a fresh set of L tokens) is rejected.
    let mut t = f.ticket.clone();
    t.token_root = topi_ratelimit::token_root(&f.ids[3], EPOCH + 1, 16);
    assert!(!verify_epoch(&m, EPOCH, &t));
}

#[test]
fn wrong_epoch_rejected() {
    let f = fx();
    assert!(!verify_epoch(&f.tree.root(), EPOCH + 1, &f.ticket));
    assert!(!verify_epoch(&f.tree.root(), EPOCH ^ (1 << 40), &f.ticket));
}

#[test]
fn token_from_other_epoch_or_r_rejected() {
    let f = fx();
    let r = f.ticket.token_root;
    let next = token(&f.ids[3], EPOCH + 1, 0, 16).unwrap();
    assert!(!verify_token(&r, &next));
    let other_id = token(&f.ids[4], EPOCH, 0, 16).unwrap();
    assert!(!verify_token(&r, &other_id));

    let mut rl = RateLimiter::new(f.sys.clone(), f.tree.root(), EPOCH);
    assert_eq!(rl.accept(&f.ticket, &next), AcceptResult::Invalid);
    assert_eq!(rl.accept(&f.ticket, &other_id), AcceptResult::Invalid);
    // A bare R that was never backed by a verified ticket is rejected.
    let unknown = topi_ratelimit::token_root(&f.ids[4], EPOCH, 16);
    assert_eq!(rl.accept(&unknown, &other_id), AcceptResult::Invalid);
    // Tampered path / index.
    let mut t = token(&f.ids[3], EPOCH, 2, 16).unwrap();
    t.index = 3;
    assert_eq!(rl.accept(&r, &t), AcceptResult::Invalid);
    let mut t = token(&f.ids[3], EPOCH, 2, 16).unwrap();
    t.path[1][0] ^= 1;
    assert_eq!(rl.accept(&r, &t), AcceptResult::Invalid);
    assert_eq!(rl.used_count(&r), 0);
}

#[test]
fn duplicate_token_rejected() {
    let f = fx();
    let mut rl = RateLimiter::new(f.sys.clone(), f.tree.root(), EPOCH);
    let t = token(&f.ids[3], EPOCH, 7, 16).unwrap();
    assert_eq!(rl.accept(&f.ticket, &t), AcceptResult::Accepted);
    assert_eq!(rl.accept(&f.ticket, &t), AcceptResult::Duplicate);
    assert_eq!(rl.accept(&f.ticket.token_root, &t), AcceptResult::Duplicate);
}

#[test]
fn no_valid_token_beyond_l() {
    let f = fx();
    let id = &f.ids[3];
    let r = f.ticket.token_root;
    // The API refuses to produce index >= L.
    assert!(token(id, EPOCH, 16, 16).is_none());
    assert!(token(id, EPOCH, 1000, 16).is_none());

    // Forge "t_16" = H(sk, epoch, 16) and try every possible index/path from the real tree.
    // (Identity hides sk, so use an identity with a known secret to play the attacker.)
    let base = token(id, EPOCH, 0, 16).unwrap();
    let sk = [42u8; 32];
    let me = Identity::from_secret(sk);
    let r_me = topi_ratelimit::token_root(&me, EPOCH, 16);
    let forged_t = hash::token(&sk, EPOCH, 16);
    for idx in 0..64u32 {
        for donor in 0..16u32 {
            let donor_tok = token(&me, EPOCH, donor, 16).unwrap();
            let forged = Token { t: forged_t, index: idx, path: donor_tok.path.clone() };
            assert!(!verify_token(&r_me, &forged));
        }
    }
    // Index >= L with an otherwise-valid path is rejected structurally.
    let mut t = base.clone();
    t.index = 16;
    assert!(!verify_token(&r, &t));
    // Longer path (claiming L = 32) is rejected by the L-aware check and by the limiter.
    let mut t = base.clone();
    t.path.push([0u8; 32]);
    assert!(!verify_token_with_l(&r, &t, 16));

    // So after L accepts, everything else is Duplicate or Invalid — never a 17th Accepted.
    let mut rl = RateLimiter::new(f.sys.clone(), f.tree.root(), EPOCH);
    for i in 0..16 {
        assert_eq!(rl.accept(&f.ticket, &token(id, EPOCH, i, 16).unwrap()), AcceptResult::Accepted);
    }
    for i in 0..16 {
        assert_eq!(rl.accept(&r, &token(id, EPOCH, i, 16).unwrap()), AcceptResult::Duplicate);
    }
    assert_eq!(rl.used_count(&r), 16);
}

#[test]
fn zk_proofs_are_randomized() {
    // Hiding mode draws fresh blinding randomness: two proofs of the same statement differ.
    let f = fx();
    let t2 = f.sys.prove_epoch(&f.ids[3], &f.tree.path(3).unwrap(), &f.tree.root(), EPOCH).unwrap();
    assert_eq!(t2.token_root, f.ticket.token_root);
    assert_ne!(t2.proof, f.ticket.proof);
    assert!(verify_epoch(&f.tree.root(), EPOCH, &t2));
}

#[test]
fn small_params_configurable() {
    let params = Params { depth: 4, l: 4 };
    let sys = Arc::new(RlnSystem::new(params).unwrap());
    let ids: Vec<Identity> = (0..16).map(|_| Identity::random()).collect();
    let tree = MembershipTree::new(4, &ids.iter().map(Identity::commitment).collect::<Vec<_>>());
    let tk = sys.prove_epoch(&ids[15], &tree.path(15).unwrap(), &tree.root(), 9).unwrap();
    assert!(sys.verify_epoch(&tree.root(), 9, &tk));
    // Default-parameter verifier must not accept a small-parameter ticket.
    assert!(!verify_epoch(&tree.root(), 9, &tk));
    let mut rl = RateLimiter::new(sys, tree.root(), 9);
    for i in 0..4 {
        assert_eq!(rl.accept(&tk, &token(&ids[15], 9, i, 4).unwrap()), AcceptResult::Accepted);
    }
    assert!(token(&ids[15], 9, 4, 4).is_none());
    // A token with L=16 shape from the same identity does not fit an L=4 limiter.
    assert_eq!(rl.accept(&tk, &token(&ids[15], 9, 0, 16).unwrap()), AcceptResult::Invalid);
}
