use std::{collections::HashSet, time::Instant};
use topi_sphinx::*;
fn main() {
    let keys: Vec<_> = (0..HOPS).map(|_| keygen()).collect();
    let pks: Vec<_> = keys.iter().map(|k| k.1.clone()).collect();
    let r = vec![
        Routing { is_exit: false, next: 1, delay_ms: 0 },
        Routing { is_exit: false, next: 2, delay_ms: 0 },
        Routing { is_exit: true, next: 0, delay_ms: 0 },
    ];
    let n = 300;
    let t = Instant::now();
    let pkts: Vec<_> = (0..n).map(|_| build_packet(&pks, &r, b"bench")).collect();
    let build = t.elapsed() / n as u32;
    let t = Instant::now();
    let mut seen = HashSet::new();
    for p in &pkts {
        process(&keys[0].0, p, &mut seen).unwrap();
    }
    let hop = t.elapsed() / n as u32;
    println!(
        "build packet: {:?}  process one hop: {:?}  => ~{} packets/s per core per node",
        build,
        hop,
        1_000_000_000u128 / hop.as_nanos().max(1)
    );
}
