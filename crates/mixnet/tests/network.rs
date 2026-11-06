//! A whole network on localhost: four nodes, a registered member, a ping round trip and a spend submission.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::mpsc;
use topi_mixnet::exit::SPEND_FIELDS;
use topi_mixnet::node::{NodeConfig, run};
use topi_mixnet::{Client, Role, Topology, keys, ratelimit};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("topi-mixnet-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread")]
async fn ping_and_spend_round_trip() {
    let dir = scratch("round-trip");
    let identity = dir.join("member.id");
    let member = ratelimit::new_identity(&identity).unwrap();

    let mut public = BTreeMap::new();
    let mut secret = BTreeMap::new();
    let mut hosts = BTreeMap::new();
    for role in Role::ALL {
        let path = dir.join(format!("{role}.key"));
        public.insert(role, keys::generate(&path).unwrap());
        secret.insert(role, keys::load(&path).unwrap().0);
        hosts.insert(role, "127.0.0.1".to_string());
    }
    let mut topo = Topology::new(&public, &hosts, &[member]).unwrap();
    topo.shift_ports(5000);
    let topo = Arc::new(topo);

    let queue = dir.join("queue");
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    for (role, sk) in secret {
        let cfg = NodeConfig {
            bind: "127.0.0.1".into(),
            batch_interval: Duration::from_millis(200),
            mean_delay_ms: 10.0,
            cover_interval_ms: 300.0,
            spend_queue: queue.clone(),
            ..NodeConfig::new(role)
        };
        tokio::spawn(run(cfg, sk, topo.clone(), tx.clone()));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut client = Client::connect((*topo).clone(), &identity).await.unwrap();
    client.mean_delay_ms = 10.0;
    let pong = client.ping(Duration::from_secs(60)).await.unwrap();
    assert_eq!(pong["pong"], true);

    let fields = json!((0..SPEND_FIELDS).collect::<Vec<_>>());
    let proof = vec![7u8; 20_000];
    let reply = client.submit_spend(&fields, &proof, Duration::from_secs(60)).await.unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    let id = reply["queued"].as_str().unwrap();
    assert_eq!(std::fs::read(queue.join(format!("{id}.tgz"))).unwrap(), proof);
    let header: serde_json::Value = serde_json::from_slice(&std::fs::read(queue.join(format!("{id}.json"))).unwrap()).unwrap();
    assert_eq!(header["fields"], fields);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn stratified_multi_node_8_routes_test() {
    let dir = scratch("stratified-8-routes");
    let identity = dir.join("member.id");
    let member = ratelimit::new_identity(&identity).unwrap();

    let mut nodes = BTreeMap::new();
    let mut secrets = Vec::new();

    // Setup 8 nodes: 2 Gateways, 2 Mixes, 2 Exits, 2 ReplyRelays
    // IDs: 11, 12 (GW), 21, 22 (Mix), 31, 32 (Exit), 41, 42 (Relay)
    let roles_with_ids = [
        (Role::Gateway, 11, 5101, Some(5201), Some(5251)),
        (Role::Gateway, 12, 5102, Some(5202), Some(5252)),
        (Role::Mix, 21, 5111, None, None),
        (Role::Mix, 22, 5112, None, None),
        (Role::Exit, 31, 5121, None, None),
        (Role::Exit, 32, 5122, None, None),
        (Role::ReplyRelay, 41, 5131, None, None),
        (Role::ReplyRelay, 42, 5132, None, None),
    ];

    for (role, id, port, rate_limit_port, reply_store_port) in roles_with_ids {
        let path = dir.join(format!("node_{id}.key"));
        let pk = keys::generate(&path).unwrap();
        let sk = keys::load(&path).unwrap().0;
        nodes.insert(
            id,
            topi_mixnet::topology::NodeEntry {
                role,
                host: "127.0.0.1".into(),
                port,
                public_key: hex::encode(topi_sphinx::pk_bytes(&pk)),
                rate_limit_port,
                reply_store_port,
            },
        );
        secrets.push((role, id, port, sk));
    }

    let topo = Topology {
        version: 2,
        nodes,
        rate_limit: "127.0.0.1:5201".into(),
        reply_store: "127.0.0.1:5251".into(),
        epoch_secs: 600,
        tokens_per_epoch: 16,
        membership_root: hex::encode(topi_mixnet::topology::membership(&[member]).root()),
        members: vec![hex::encode(member)],
    };
    let topo = Arc::new(topo);

    let queue = dir.join("queue");
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            eprintln!("[NODE EVENT] {:?}", e);
        }
    });

    // Start all 8 nodes
    for (role, id, port, sk) in secrets {
        let cfg = NodeConfig {
            role,
            node_id: Some(id),
            port: Some(port),
            bind: "127.0.0.1".into(),
            batch_interval: Duration::from_millis(150),
            mean_delay_ms: 10.0,
            cover_interval_ms: 300.0,
            spend_queue: queue.clone(),
        };
        tokio::spawn(run(cfg, sk, topo.clone(), tx.clone()));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Run client through the 8-node stratified mesh
    let mut client = Client::connect((*topo).clone(), &identity).await.unwrap();
    client.mean_delay_ms = 10.0;
    let pong = client.ping(Duration::from_secs(60)).await.unwrap();
    assert_eq!(pong["pong"], true);

    let fields = json!((0..SPEND_FIELDS).collect::<Vec<_>>());
    let proof = vec![42u8; 15_000];
    let reply = client.submit_spend(&fields, &proof, Duration::from_secs(60)).await.unwrap();
    assert_eq!(reply["ok"], true, "{reply}");

    let _ = std::fs::remove_dir_all(&dir);
}
