//! topi-node: run one mixnet node, create node keys, and write topologies.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::sync::mpsc;
use topi_mixnet::events::{NodeStats, serve_stats};
use topi_mixnet::metrics::{self, METRICS_PORT};
use topi_mixnet::node::{NodeConfig, run};
use topi_mixnet::topology::{STATS_PORT, read_members};
use topi_mixnet::{Result, Role, Topology, keys};

#[derive(Parser)]
#[command(version, about = "Post-quantum mixnet node")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a node key and print its public key.
    Keygen { key: PathBuf },
    /// Write a topology from four node keys (DIR/<role>.key) and the hosts they run on.
    Topology {
        out: PathBuf,
        /// File with one member commitment (hex) per line.
        #[arg(long)]
        members: PathBuf,
        /// Directory holding gateway.key, mix.key, exit.key, reply-relay.key.
        #[arg(long)]
        keys: PathBuf,
        /// Hosts in role order: gateway, mix, exit, reply relay.
        #[arg(long, num_args = 4, value_names = ["GATEWAY", "MIX", "EXIT", "REPLY_RELAY"])]
        hosts: Vec<String>,
    },
    /// Run one node.
    Run {
        /// gateway, mix, exit or reply-relay.
        #[arg(value_parser = parse_role)]
        role: Role,
        #[arg(long)]
        topology: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        /// Port serving this node's counters as JSON.
        #[arg(long, default_value_t = STATS_PORT)]
        stats_port: u16,
        /// Port serving Prometheus metrics over HTTP (see METRICS.md).
        #[arg(long, default_value_t = METRICS_PORT)]
        metrics_port: u16,
        /// How long the mix collects each batch, in milliseconds.
        #[arg(long, default_value_t = 1500)]
        batch_ms: u64,
        /// Where the exit queues pool spends for the prover.
        #[arg(long, env = "TOPI_SPEND_QUEUE", default_value = "spend-queue")]
        spend_queue: PathBuf,
    },
}

fn parse_role(s: &str) -> std::result::Result<Role, String> {
    Role::parse(s).ok_or_else(|| "expected gateway, mix, exit or reply-relay".into())
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Keygen { key } => {
            let pk = keys::generate(&key)?;
            println!("{}", hex::encode(topi_sphinx::pk_bytes(&pk)));
        }
        Command::Topology { out, members, keys: dir, hosts } => {
            let mut public = BTreeMap::new();
            let mut by_role = BTreeMap::new();
            for (role, host) in Role::ALL.into_iter().zip(hosts) {
                public.insert(role, keys::load(dir.join(format!("{role}.key")))?.1);
                by_role.insert(role, host);
            }
            Topology::new(&public, &by_role, &read_members(&members)?)?.save(&out)?;
            println!("wrote {}", out.display());
        }
        Command::Run { role, topology, key, bind, stats_port, metrics_port, batch_ms, spend_queue } => {
            let topo = Arc::new(Topology::load(&topology)?);
            let (sk, _) = keys::load(&key)?;
            let (tx, mut rx) = mpsc::unbounded_channel();
            let stats = Arc::new(Mutex::new(NodeStats::new(role.name())));
            tokio::spawn(serve_stats(stats_port, stats.clone()));
            tokio::spawn(metrics::serve(metrics_port, stats.clone()));
            let cfg = NodeConfig { bind, batch_interval: Duration::from_millis(batch_ms), spend_queue, ..NodeConfig::new(role) };
            let node = tokio::spawn(run(cfg, sk, topo, tx));
            println!("{role} node running");
            while let Some(e) = rx.recv().await {
                if let Some(line) = stats.lock().unwrap().record(e) {
                    println!("{line}");
                }
            }
            node.await.expect("node task")?;
        }
    }
    Ok(())
}
