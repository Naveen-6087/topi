//! `cargo run --release --example bench [-- <depth> <L> <iters>]`
//!
//! Prints setup / prove / verify times, epoch proof size and token size.

use std::time::{Duration, Instant};

use topi_ratelimit::{AcceptResult, Identity, MembershipTree, Params, RateLimiter, RlnSystem, token, verify_token};

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let depth = args.first().copied().unwrap_or(16);
    let l = args.get(1).copied().unwrap_or(16);
    let iters = args.get(2).copied().unwrap_or(5).max(1);
    let params = Params { depth, l };
    println!("PQ-RLN design 1 bench: depth={depth} L={l} iters={iters}");
    println!("threads: {}", std::thread::available_parallelism().map_or(1, |n| n.get()));

    // Membership: 1000 members (tree is still depth `depth`).
    let ids: Vec<Identity> = (0..1000).map(|_| Identity::random()).collect();
    let commitments: Vec<_> = ids.iter().map(Identity::commitment).collect();
    let t = Instant::now();
    let tree = MembershipTree::new(depth, &commitments);
    println!("membership tree build (1000 members): {}", ms(t.elapsed()));
    let m = tree.root();
    let me = &ids[123];
    let path = tree.path(123).unwrap();

    let t = Instant::now();
    let sys = RlnSystem::new(params).unwrap();
    println!("circuit + ZK verifier setup: {}", ms(t.elapsed()));
    println!("inner AND constraints: {}", sys.n_and_constraints());
    let t = Instant::now();
    sys.prover_setup();
    println!("ZK prover setup (one-off): {}", ms(t.elapsed()));

    let mut prove = Vec::new();
    let mut verify = Vec::new();
    let mut ticket = None;
    for e in 0..iters as u64 {
        let t = Instant::now();
        let tk = sys.prove_epoch(me, &path, &m, 1000 + e).unwrap();
        prove.push(t.elapsed());
        let t = Instant::now();
        assert!(sys.verify_epoch(&m, 1000 + e, &tk));
        verify.push(t.elapsed());
        ticket = Some((1000 + e, tk));
    }
    let med = |v: &mut Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    let (epoch, ticket) = ticket.unwrap();
    println!("epoch prove  (median of {iters}): {}", ms(med(&mut prove)));
    println!("epoch verify (median of {iters}): {}", ms(med(&mut verify)));
    println!("epoch proof size: {} bytes (+32 B token root = {} B ticket)", ticket.proof.len(), ticket.to_bytes().len());

    let tok = token(me, epoch, 0, l).unwrap();
    println!("token size: {} bytes (t 32 + index 4 + path {}x32)", tok.encoded_len(), tok.path.len());
    let t = Instant::now();
    let n = 100_000;
    for _ in 0..n {
        assert!(verify_token(&ticket.token_root, &tok));
    }
    println!("token verify: {:.2} us", t.elapsed().as_secs_f64() * 1e6 / n as f64);

    let mut rl = RateLimiter::new(std::sync::Arc::new(sys), m, epoch);
    let t = Instant::now();
    assert_eq!(rl.accept(&ticket, &tok), AcceptResult::Accepted);
    println!("RateLimiter first accept (incl. ticket verify): {}", ms(t.elapsed()));
    let t = Instant::now();
    for i in 1..l as u32 {
        let tk = token(me, epoch, i, l).unwrap();
        assert_eq!(rl.accept(&ticket.token_root, &tk), AcceptResult::Accepted);
    }
    println!("RateLimiter accept of remaining {} tokens: {}", l - 1, ms(t.elapsed()));
    assert_eq!(rl.accept(&ticket.token_root, &tok), AcceptResult::Duplicate);
}
