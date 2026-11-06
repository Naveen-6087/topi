//! The network topology: which nodes exist, where they listen, and their public keys.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use topi_ratelimit::{Digest, MembershipTree};
use topi_sphinx::{EncapsulationKey, pk_bytes, pk_from_bytes};

use crate::{Error, Result};

/// Depth of the rate-limit membership tree.
pub const MEMBERSHIP_DEPTH: usize = topi_ratelimit::DEFAULT_DEPTH;
/// Rate-limit tokens per member per epoch.
pub const TOKENS_PER_EPOCH: usize = topi_ratelimit::DEFAULT_L;
/// Rate-limit epoch length.
pub const EPOCH_SECS: u64 = 600;

pub const RATE_LIMIT_PORT: u16 = 47201;
pub const REPLY_STORE_PORT: u16 = 47250;
pub const STATS_PORT: u16 = 47300;

/// A node's role. Its id doubles as the routing address inside packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// First hop: admits rate-limited packets, hosts the reply store, emits cover traffic.
    Gateway,
    /// Second hop: collects packets into batches and releases each batch shuffled.
    Mix,
    /// Last hop: reassembles messages and queues pool spends for the prover.
    Exit,
    /// First hop of the reply path.
    ReplyRelay,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Gateway, Role::Mix, Role::Exit, Role::ReplyRelay];

    pub fn id(self) -> u16 {
        match self {
            Role::Gateway => 1,
            Role::Mix => 2,
            Role::Exit => 3,
            Role::ReplyRelay => 4,
        }
    }

    pub fn from_id(id: u16) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Gateway => "gateway",
            Role::Mix => "mix",
            Role::Exit => "exit",
            Role::ReplyRelay => "reply-relay",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == s)
    }

    pub fn default_port(self) -> u16 {
        47100 + self.id()
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeEntry {
    pub role: Role,
    pub host: String,
    pub port: u16,
    /// X-Wing public key, hex.
    pub public_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub version: u32,
    pub nodes: BTreeMap<u16, NodeEntry>,
    /// `host:port` of the gateway's rate limiter.
    pub rate_limit: String,
    /// `host:port` of the gateway's reply store.
    pub reply_store: String,
    pub epoch_secs: u64,
    pub tokens_per_epoch: usize,
    /// Root of the rate-limit membership tree, hex.
    pub membership_root: String,
    /// Member commitments, hex, in tree order.
    pub members: Vec<String>,
}

impl Topology {
    /// A topology with every role at `hosts[role]`; the rate limiter and reply store live at the gateway.
    pub fn new(keys: &BTreeMap<Role, EncapsulationKey>, hosts: &BTreeMap<Role, String>, members: &[Digest]) -> Result<Topology> {
        let mut nodes = BTreeMap::new();
        for role in Role::ALL {
            let key = keys.get(&role).ok_or_else(|| Error::Topology(format!("no key for {role}")))?;
            let host = hosts.get(&role).ok_or_else(|| Error::Topology(format!("no host for {role}")))?;
            nodes.insert(
                role.id(),
                NodeEntry { role, host: host.clone(), port: role.default_port(), public_key: hex::encode(pk_bytes(key)) },
            );
        }
        let gateway = &hosts[&Role::Gateway];
        Ok(Topology {
            version: 1,
            nodes,
            rate_limit: format!("{gateway}:{RATE_LIMIT_PORT}"),
            reply_store: format!("{gateway}:{REPLY_STORE_PORT}"),
            epoch_secs: EPOCH_SECS,
            tokens_per_epoch: TOKENS_PER_EPOCH,
            membership_root: hex::encode(membership(members).root()),
            members: members.iter().map(hex::encode).collect(),
        })
    }

    /// Move every port by `offset` (several networks on one host, e.g. in tests).
    pub fn shift_ports(&mut self, offset: u16) {
        for n in self.nodes.values_mut() {
            n.port += offset;
        }
        for addr in [&mut self.rate_limit, &mut self.reply_store] {
            if let Some((host, port)) = addr.rsplit_once(':')
                && let Ok(p) = port.parse::<u16>()
            {
                *addr = format!("{host}:{}", p + offset);
            }
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Topology> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| Error::Topology(e.to_string()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self).expect("topology serialises"))?;
        Ok(())
    }

    fn node(&self, id: u16) -> Result<&NodeEntry> {
        self.nodes.get(&id).ok_or_else(|| Error::Topology(format!("no node {id}")))
    }

    /// `host:port` of a node.
    pub fn addr(&self, id: u16) -> Result<String> {
        self.node(id).map(|n| format!("{}:{}", n.host, n.port))
    }

    pub fn public_key(&self, id: u16) -> Result<EncapsulationKey> {
        let bytes = hex::decode(&self.node(id)?.public_key).map_err(|e| Error::Topology(e.to_string()))?;
        Ok(pk_from_bytes(&bytes))
    }

    pub fn members(&self) -> Result<Vec<Digest>> {
        self.members.iter().map(|m| decode_digest(m)).collect()
    }

    pub fn root(&self) -> Result<Digest> {
        decode_digest(&self.membership_root)
    }
}

pub fn membership(members: &[Digest]) -> MembershipTree {
    MembershipTree::new(MEMBERSHIP_DEPTH, members)
}

fn decode_digest(s: &str) -> Result<Digest> {
    hex::decode(s).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| Error::Topology(format!("not a 32-byte hex digest: {s}")))
}

/// Member commitments from a file with one hex commitment per line.
pub fn read_members(path: impl AsRef<Path>) -> Result<Vec<Digest>> {
    std::fs::read_to_string(path)?.lines().map(str::trim).filter(|l| !l.is_empty()).map(decode_digest).collect()
}
