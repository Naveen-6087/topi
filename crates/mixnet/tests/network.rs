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

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("topi-mixnet-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread")]
async fn ping_and_spend_round_trip() {
    let dir = scratch();
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
