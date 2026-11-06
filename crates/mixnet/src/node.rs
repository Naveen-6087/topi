//! Running a node. Every node strips one layer from each packet and forwards it. The mix holds packets
//! and releases them in shuffled batches; the gateway also runs the rate limiter, the reply store and
//! cover traffic; the exit answers messages.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::TcpListener;
use topi_sphinx::{DecapsulationKey, PACKET_SIZE, Routing, cover_packet, process, reply_packet};

use crate::events::{Event, Events};
use crate::exit::Exit;
use crate::transport::{self, exp_delay_ms, random_u64, read_packet, shuffle};
use crate::{Result, Role, Topology, ratelimit, replies};

/// Packets the mix is holding, with the node each goes to next.
type Batch = Arc<Mutex<Vec<(u16, Vec<u8>)>>>;

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub role: Role,
    /// Explicit node id in topology (e.g. 11, 12, 21, 22), defaults to role.id().
    pub node_id: Option<u16>,
    /// Explicit listen port override.
    pub port: Option<u16>,
    /// Address to listen on, e.g. `0.0.0.0`.
    pub bind: String,
    /// How long the mix collects a batch.
    pub batch_interval: Duration,
    /// Mean per-hop delay a sender picks for its packets, in milliseconds.
    pub mean_delay_ms: f64,
    /// Mean gap between cover packets from the gateway, in milliseconds.
    pub cover_interval_ms: f64,
    /// Where the exit queues pool spends.
    pub spend_queue: PathBuf,
}

impl NodeConfig {
    pub fn new(role: Role) -> NodeConfig {
        NodeConfig {
            role,
            node_id: None,
            port: None,
            bind: "0.0.0.0".into(),
            batch_interval: Duration::from_millis(1500),
            mean_delay_ms: 100.0,
            cover_interval_ms: 700.0,
            spend_queue: PathBuf::from("spend-queue"),
        }
    }
}

/// Run a node until its listener fails.
pub async fn run(cfg: NodeConfig, sk: DecapsulationKey, topo: Arc<Topology>, events: Events) -> Result<()> {
    let id = cfg.node_id.unwrap_or_else(|| cfg.role.id());
    let port = cfg.port.or_else(|| topo.nodes.get(&id).map(|n| n.port)).unwrap_or(cfg.role.default_port());
    let listener = TcpListener::bind((cfg.bind.as_str(), port)).await?;
    let sk = Arc::new(sk);
    let seen: Arc<Mutex<HashSet<[u8; 32]>>> = Arc::default();
    let batch: Batch = Arc::default();
    let exit = (cfg.role == Role::Exit).then(|| Arc::new(Exit::new(cfg.spend_queue.clone())));

    match cfg.role {
        Role::Gateway => {
            let node_entry = topo.nodes.get(&id);
            let rl_port = node_entry.and_then(|n| n.rate_limit_port).or_else(|| {
                let primary_gw = topo.nodes_for_role(Role::Gateway).first().copied().unwrap_or(Role::Gateway.id());
                if id == primary_gw { port_of(&topo.rate_limit) } else { None }
            });
            if let Some(rate_limit_port) = rl_port {
                let (bind, t, e) = (cfg.bind.clone(), topo.clone(), events.clone());
                tokio::spawn(async move { ratelimit::serve(&bind, rate_limit_port, id, t, e).await });
            }
            let rs_port = node_entry.and_then(|n| n.reply_store_port).or_else(|| {
                let primary_gw = topo.nodes_for_role(Role::Gateway).first().copied().unwrap_or(Role::Gateway.id());
                if id == primary_gw { port_of(&topo.reply_store) } else { None }
            });
            if let Some(reply_port) = rs_port {
                let bind = cfg.bind.clone();
                tokio::spawn(async move { replies::serve(&bind, reply_port).await });
            }
            tokio::spawn(cover_traffic(topo.clone(), cfg.clone()));
        }
        Role::Mix => {
            tokio::spawn(release_batches(batch.clone(), topo.clone(), events.clone(), cfg.batch_interval));
        }
        _ => {}
    }

    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let (sk, seen, batch, events, exit, topo, role) =
            (sk.clone(), seen.clone(), batch.clone(), events.clone(), exit.clone(), topo.clone(), cfg.role);
        tokio::spawn(async move {
            let Some(pkt) = read_packet(stream).await else { return };
            let _ = events.send(Event::Received);
            let p = match process(&sk, &pkt, &mut seen.lock().unwrap()) {
                Ok(p) => p,
                Err(e) => {
                    let _ = events.send(Event::Rejected(e.into()));
                    return;
                }
            };
            if p.routing.is_exit {
                if role == Role::Gateway {
                    // the end of a reply path: hold it in the reply store under the sender's slot
                    let _ = events.send(Event::Forwarded);
                    let reply_store = topo.reply_store_addr(id).unwrap_or_else(|_| topo.reply_store.clone());
                    let _ = replies::deposit(&reply_store, p.routing.next, &p.packet).await;
                } else if let Some(exit) = exit
                    && let Some(answer) = exit.accept(&p.packet, &events)
                {
                    let reply = reply_packet(&answer.surb, &answer.body);
                    if let Ok(addr) = topo.addr(answer.relay_id) {
                        let _ = transport::send(&addr, &reply).await;
                    }
                }
                return;
            }
            if role == Role::Mix {
                batch.lock().unwrap().push((p.routing.next, p.packet));
                return;
            }
            tokio::time::sleep(Duration::from_millis(p.routing.delay_ms as u64)).await;
            if let Ok(addr) = topo.addr(p.routing.next) {
                let _ = events.send(Event::Forwarded);
                let _ = transport::send(&addr, &p.packet).await;
            }
        });
    }
}

fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit_once(':').and_then(|(_, p)| p.parse().ok())
}

async fn release_batches(batch: Batch, topo: Arc<Topology>, events: Events, every: Duration) {
    loop {
        tokio::time::sleep(every).await;
        let mut out = std::mem::take(&mut *batch.lock().unwrap());
        if out.is_empty() {
            continue;
        }
        shuffle(&mut out);
        let _ = events.send(Event::Batch(out.len()));
        for (next, pkt) in out {
            if let Ok(addr) = topo.addr(next) {
                let _ = events.send(Event::Forwarded);
                let _ = transport::send(&addr, &pkt).await;
            }
        }
    }
}

/// Empty packets on the forward route, indistinguishable from real ones until the exit.
async fn cover_traffic(topo: Arc<Topology>, cfg: NodeConfig) {
    let gw_id = cfg.node_id.unwrap_or_else(|| cfg.role.id());
    let Ok(gw_key) = topo.public_key(gw_id) else { return };
    let Ok(gateway) = topo.addr(gw_id) else { return };

    let sample_node = |role: Role| -> u16 {
        let candidates = topo.nodes_for_role(role);
        if candidates.is_empty() {
            role.id()
        } else {
            let idx = (random_u64() as usize) % candidates.len();
            candidates[idx]
        }
    };

    loop {
        tokio::time::sleep(Duration::from_millis(exp_delay_ms(cfg.cover_interval_ms) as u64)).await;

        let mix_id = sample_node(Role::Mix);
        let exit_id = sample_node(Role::Exit);
        let (Ok(mix_key), Ok(exit_key)) = (topo.public_key(mix_id), topo.public_key(exit_id)) else { continue };

        let route = [gw_key.clone(), mix_key, exit_key];
        let routing = [
            Routing { is_exit: false, next: mix_id, delay_ms: exp_delay_ms(cfg.mean_delay_ms) },
            Routing { is_exit: false, next: exit_id, delay_ms: 0 },
            Routing { is_exit: true, next: exit_id, delay_ms: 0 },
        ];
        let pkt = cover_packet(&route, &routing);
        debug_assert_eq!(pkt.len(), PACKET_SIZE);
        let _ = transport::send(&gateway, &pkt).await;
    }
}
