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
    /// Write an 8-node stratified topology (2 nodes per role: gw-1, gw-2, mix-1, mix-2, exit-1, exit-2, relay-1, relay-2).
    TopologyStratified {
        out: PathBuf,
        /// File with one member commitment (hex) per line.
        #[arg(long)]
        members: PathBuf,
        /// Directory holding the 8 node key files.
        #[arg(long)]
        keys: PathBuf,
        /// Hostname or IP to use for all nodes.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
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
        /// Node ID in the topology (e.g. 11, 12, 21, 22), defaults to role ID.
        #[arg(long)]
        node_id: Option<u16>,
        /// Port to listen on for Sphinx packets (overrides topology).
        #[arg(long)]
        packet_port: Option<u16>,
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
        Command::TopologyStratified { out, members, keys: dir, host } => {
            let member_digests = read_members(&members)?;
            let root = hex::encode(topi_mixnet::topology::membership(&member_digests).root());
            let mut nodes = BTreeMap::new();
            let defs = [
                (Role::Gateway, 11, 47101, Some(47201), Some(47251), "gw-1"),
                (Role::Gateway, 12, 47102, Some(47202), Some(47252), "gw-2"),
                (Role::Mix, 21, 47111, None, None, "mix-1"),
                (Role::Mix, 22, 47112, None, None, "mix-2"),
                (Role::Exit, 31, 47121, None, None, "exit-1"),
                (Role::Exit, 32, 47122, None, None, "exit-2"),
                (Role::ReplyRelay, 41, 47131, None, None, "relay-1"),
                (Role::ReplyRelay, 42, 47132, None, None, "relay-2"),
            ];
            for (role, id, port, rate_limit_port, reply_store_port, name) in defs {
                let key_path = dir.join(format!("{name}.key"));
                let (_, pk) = keys::load(&key_path)?;
                nodes.insert(
                    id,
                    topi_mixnet::topology::NodeEntry {
                        role,
                        host: host.clone(),
                        port,
                        public_key: hex::encode(topi_sphinx::pk_bytes(&pk)),
                        rate_limit_port,
                        reply_store_port,
                    },
                );
            }
            let topo = Topology {
                version: 2,
                nodes,
                rate_limit: format!("{host}:47201"),
                reply_store: format!("{host}:47251"),
                epoch_secs: topi_mixnet::topology::EPOCH_SECS,
                tokens_per_epoch: topi_mixnet::topology::TOKENS_PER_EPOCH,
                membership_root: root,
                members: member_digests.iter().map(hex::encode).collect(),
            };
            topo.save(&out)?;
            println!("wrote stratified topology to {}", out.display());
        }
        Command::Run { role, topology, key, bind, stats_port, metrics_port, batch_ms, spend_queue, node_id, packet_port } => {
            let topo = Arc::new(Topology::load(&topology)?);
            let (sk, _) = keys::load(&key)?;
            let (tx, mut rx) = mpsc::unbounded_channel();
            let stats = Arc::new(Mutex::new(NodeStats::new(role.name())));
            tokio::spawn(serve_stats(stats_port, stats.clone()));
            tokio::spawn(metrics::serve(metrics_port, stats.clone()));
            let cfg = NodeConfig {
                bind,
                batch_interval: Duration::from_millis(batch_ms),
                spend_queue,
                node_id,
                port: packet_port,
                ..NodeConfig::new(role)
            };
            let node = tokio::spawn(run(cfg, sk, topo, tx));
            println!("{role} node running (id={:?})", node_id.unwrap_or(role.id()));
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
