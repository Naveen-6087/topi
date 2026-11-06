//! Sending messages through the mixnet and collecting their replies.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use topi_sphinx::{PAYLOAD, Routing, build_packet, build_surb, open_reply};

use crate::message::{KIND_DATA, KIND_PING, fragment};
use crate::ratelimit::RateLimitClient;
use crate::transport::{exp_delay_ms, random_u64};
use crate::{Error, Result, Role, Topology, exit, replies};

const POLL_EVERY: Duration = Duration::from_millis(700);
/// Mean gap between a message's packets, in milliseconds.
const SEND_GAP_MS: f64 = 60.0;

pub struct Client {
    topo: Topology,
    gw_id: u16,
    limit: RateLimitClient,
    /// Mean per-hop mixing delay requested for each packet, in milliseconds.
    pub mean_delay_ms: f64,
}

impl Client {
    /// Connect as the member whose identity file is at `identity` (proving this epoch's ticket if needed).
    pub async fn connect(topo: Topology, identity: impl AsRef<Path>) -> Result<Client> {
        let candidates = topo.nodes_for_role(Role::Gateway);
        let gw_id = if candidates.is_empty() {
            Role::Gateway.id()
        } else {
            let idx = (random_u64() as usize) % candidates.len();
            candidates[idx]
        };
        let rate_limit_addr = topo.rate_limit_addr(gw_id)?;
        let limit = RateLimitClient::connect_to(&rate_limit_addr, &topo, identity).await?;
        Ok(Client { topo, gw_id, limit, mean_delay_ms: 100.0 })
    }

    /// A round trip through every hop: gateway, mix, exit, then the reply path back.
    pub async fn ping(&mut self, timeout: Duration) -> Result<Value> {
        self.request(KIND_PING, b"", timeout).await
    }

    /// Submit a pool spend (its public settle fields and client proof) to the exit's prover queue.
    pub async fn submit_spend(&mut self, fields: &Value, proof: &[u8], timeout: Duration) -> Result<Value> {
        self.request(KIND_DATA, &exit::encode_spend(fields, proof), timeout).await
    }

    /// Send one message and wait for the exit's answer.
    pub async fn request(&mut self, kind: u8, data: &[u8], timeout: Duration) -> Result<Value> {
        // Uniform fair random sampler across nodes available in a given layer
        let sample_node = |topo: &Topology, role: Role| -> u16 {
            let candidates = topo.nodes_for_role(role);
            if candidates.is_empty() {
                role.id()
            } else {
                let idx = (random_u64() as usize) % candidates.len();
                candidates[idx]
            }
        };

        // 1. Session-sticky Gateway chosen fairly at random during connect
        let gw_id = self.gw_id;
        let gw_key = self.topo.public_key(gw_id)?;

        // 2. Message-sticky Exit node chosen fairly at random from available Exits
        let exit_id = sample_node(&self.topo, Role::Exit);
        let exit_key = self.topo.public_key(exit_id)?;

        // 3. Sample return path nodes (Relay -> Mix -> Gateway)
        let reply_relay_id = sample_node(&self.topo, Role::ReplyRelay);
        let reply_mix_id = sample_node(&self.topo, Role::Mix);
        let reply_relay_key = self.topo.public_key(reply_relay_id)?;
        let reply_mix_key = self.topo.public_key(reply_mix_id)?;

        let back = [reply_relay_key, reply_mix_key, gw_key.clone()];

        // A reply slot only we know: the reply waits under it in the reply store.
        let slot = 1000 + (random_u64() % 60000) as u16;
        let surb = build_surb(
            &back,
            &[
                Routing { is_exit: false, next: reply_mix_id, delay_ms: exp_delay_ms(self.mean_delay_ms) },
                Routing { is_exit: false, next: gw_id, delay_ms: 0 },
                Routing { is_exit: true, next: slot, delay_ms: 0 },
            ],
        );
        let id = random_u64();
        let mut token = None;

        for frag in fragment(kind, id, reply_relay_id, surb.header.clone(), data) {
            // For each packet fragment, sample Mix independently for maximum mixing entropy
            let mix_id = sample_node(&self.topo, Role::Mix);
            let mix_key = self.topo.public_key(mix_id)?;

            let forward = [gw_key.clone(), mix_key, exit_key.clone()];
            let routing = [
                Routing { is_exit: false, next: mix_id, delay_ms: exp_delay_ms(self.mean_delay_ms) },
                Routing { is_exit: false, next: exit_id, delay_ms: 0 },
                Routing { is_exit: true, next: exit_id, delay_ms: 0 },
            ];
            let packet = build_packet(&forward, &routing, &frag.encode());
            tokio::time::sleep(Duration::from_millis(exp_delay_ms(SEND_GAP_MS) as u64)).await;
            match &token {
                None => token = Some(self.limit.send_first(&packet).await?),
                Some(t) => self.limit.send_continuation(t, &packet).await?,
            }
        }

        let reply_store_addr = self.topo.reply_store_addr(self.gw_id)?;
        let body = tokio::time::timeout(timeout, async {
            loop {
                tokio::time::sleep(POLL_EVERY).await;
                let Ok(packets) = replies::fetch(&reply_store_addr, slot).await else { continue };
                for pkt in packets {
                    if let Some(msg) = open_reply(&surb.keys, pkt[..PAYLOAD].to_vec())
                        && msg.len() >= 8
                        && msg[..8] == id.to_le_bytes()
                    {
                        return msg[8..].to_vec();
                    }
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)?;
        serde_json::from_slice(&body).map_err(|e| Error::Protocol(e.to_string()))
    }
}
