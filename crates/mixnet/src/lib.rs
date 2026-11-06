//! A post-quantum mixnet for carrying private transactions to a chain.
//!
//! Four roles route every message over three hops and bring the reply back over three more:
//!
//! ```text
//! client ─► gateway ─► mix ─► exit            (forward: rate-limited, batched, shuffled)
//!   ▲          ▲        │      │
//!   └─ reply store ◄────┴── reply relay ◄─┘   (reply: on a single-use reply block, SURB)
//! ```
//!
//! Packets are fixed-size KEM-Sphinx packets ([`topi_sphinx`]) with an X-Wing key encapsulation per hop.
//! The gateway admits a packet only with an anonymous rate-limit token ([`topi_ratelimit`]). Replies wait
//! in the gateway's reply store under a slot only the sender knows, and the sender polls for them, so
//! clients never accept connections.

pub mod client;
pub mod error;
pub mod events;
pub mod exit;
pub mod keys;
pub mod message;
pub mod metrics;
pub mod node;
pub mod ratelimit;
pub mod replies;
pub mod topology;
pub mod transport;

pub use client::Client;
pub use error::{Error, Result};
pub use topology::{Role, Topology};
