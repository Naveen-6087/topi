//! Every command the console can run, in one place.

use std::path::Path;

use crate::jobs::Spec;
use crate::paths::{Paths, slash};

pub const LOCAL_ROLES: [&str; 4] = ["gateway", "mix", "exit", "reply-relay"];
pub const LOCAL_STATS_PORT: u16 = 47300;
pub const LOCAL_METRICS_PORT: u16 = 47400;
/// The alternative mempool's JSON-RPC port (the ERC-8421 sidecar, mempool/sidecar).
pub const MEMPOOL_PORT: u16 = 47500;
/// The job name the console gives the mempool service.
pub const MEMPOOL_JOB: &str = "alt mempool";

pub fn mempool_url() -> String {
    format!("http://127.0.0.1:{MEMPOOL_PORT}/rpc")
}

/// The second mempool node's JSON-RPC port; it gossips with the first.
pub const MEMPOOL_PEER_PORT: u16 = 47501;

/// The topi privacy-pool mempool on the ERC-8421 sidecar (mempool/sidecar/start.sh), as a service
/// the console stops when it quits: the first node gossips on 4337 (libp2p).
pub fn mempool(p: &Paths) -> Spec {
    sidecar(p, MEMPOOL_JOB.to_string(), &format!("--port {MEMPOOL_PORT} --p2p --p2p-port 4337"))
}

/// The second node of the same mempool: it dials the first and receives its settles by gossip.
pub fn mempool_peer(p: &Paths) -> Spec {
    sidecar(p, format!("{MEMPOOL_JOB} peer"), &format!("--port {MEMPOOL_PEER_PORT} --p2p-port 4338 --join {MEMPOOL_PORT}"))
}

/// The sidecar runs on Linux: through WSL on Windows.
fn sidecar(p: &Paths, name: String, args: &str) -> Spec {
    let mut spec = if cfg!(windows) {
        let distro = p.setting("WSL_DISTRO", "Ubuntu");
        let script = p.wsl("mempool/sidecar/start.sh");
        Spec::new(name, "wsl", &["-d", &distro, "--exec", "bash", "-lc", &format!("exec bash '{script}' {args}")], p.root.clone())
    } else {
        let mut all = vec!["mempool/sidecar/start.sh"];
        all.extend(args.split_whitespace());
        Spec::new(name, "bash", &all, p.root.clone())
    };
    spec.service = true;
    spec
}

pub fn aws(p: &Paths, sub: &str) -> Spec {
    Spec::new(format!("aws {sub}"), p.bash.display().to_string(), &["infra/aws/deploy.sh", sub], p.root.clone())
}

pub fn build_linux(p: &Paths) -> Spec {
    let script = p.wsl("infra/aws/build-linux.sh");
    if cfg!(windows) {
        let distro = p.setting("WSL_DISTRO", "Ubuntu");
        Spec::new("build node for Linux", "wsl", &["-d", &distro, "--exec", "bash", &script], p.root.clone())
    } else {
        Spec::new("build node for Linux", "bash", &[&script], p.root.clone())
    }
}

/// One local node; stats and metrics ports count up from 47300 and 47400 in role order.
pub fn local_node(p: &Paths, role: &str, index: usize) -> Spec {
    let topo = p.s("data/local/topology.json");
    let key = p.s(&format!("data/local/keys/{role}.key"));
    let queue = p.s("data/local/spend-queue");
    let stats = (LOCAL_STATS_PORT + index as u16).to_string();
    let metrics = (LOCAL_METRICS_PORT + index as u16).to_string();
    let mut spec = Spec::new(
        format!("local {role}"),
        p.exe("topi-node"),
        &[
            "run",
            role,
            "--topology",
            &topo,
            "--key",
            &key,
            "--bind",
            "127.0.0.1",
            "--stats-port",
            &stats,
            "--metrics-port",
            &metrics,
            "--spend-queue",
            &queue,
        ],
        p.root.clone(),
    );
    spec.service = true;
    spec
}

pub fn ping(p: &Paths, topology: &str) -> Spec {
    let id = p.identity();
    Spec::new("ping through the mixnet", p.exe("topi-client"), &["ping", "--topology", topology, "--identity", &id], p.root.clone())
}

pub fn send(p: &Paths, note: &str, topology: Option<&str>, notes_dir: Option<&Path>) -> Spec {
    let dir = notes_dir.map(|d| d.to_path_buf()).unwrap_or_else(|| p.p("data/wallet/demo"));
    let proof = dir.join("send").join(format!("{note}.tgz"));
    let public = dir.join(format!("public-{note}.json"));
    let topo = topology.map(|t| t.to_string()).unwrap_or_else(|| p.s("data/local/topology.json"));
    let id = p.identity();
    Spec::new(
        format!("send {note}"),
        p.exe("topi-client"),
        &["spend", "--topology", &topo, "--identity", &id, "--proof", &slash(&proof), "--public", &slash(&public)],
        p.root.clone(),
    )
}

/// The exit prover; with `mempool`, its settle goes through the alternative mempool.
pub fn exit_prover(p: &Paths, queue: &str, mempool: Option<String>) -> Spec {
    let node = p.node.display().to_string();
    let mut spec = Spec::new("exit prover", node, &["scripts/exit-prover.ts", "--queue", queue], p.root.clone());
    if let Some(url) = mempool {
        spec.env.push(("MEMPOOL_URL".into(), url));
    }
    spec
}

pub fn lean_build(p: &Paths) -> Spec {
    Spec::new("build Lean proofs", "lake", &["build"], p.p("lean"))
}

pub fn lean_audit(p: &Paths) -> Spec {
    Spec::new("audit Lean axioms", "lake", &["env", "lean", "Audit.lean"], p.p("lean"))
}

pub fn differential(p: &Paths, which: &str) -> Spec {
    let node = p.node.display().to_string();
    let script = format!("scripts/differential/{which}.ts");
    Spec::new(format!("differential test: {which}"), node, &[&script], p.root.clone())
}
