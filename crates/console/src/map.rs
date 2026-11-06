//! Geography of the network view: where each node sits on the world map (longitude, latitude), the
//! curved routes between them, and packets in flight along them. The four-node network sits in the four
//! AWS regions; the stratified eight-node network pairs each role across two nearby regions.

use ratatui::style::Color;

use crate::theme::{AMBER, PALE, RED};

/// (longitude, latitude) of each actor: the regions, the operator, and the chain.
pub fn place(who: &str) -> (f64, f64) {
    match who {
        "you" => (77.6, 12.9),
        "gateway" | "gw-1" => (72.9, 19.1),
        "gw-2" => (80.3, 13.1),
        "mix" | "mix-1" => (8.7, 50.1),
        "mix-2" => (-0.1, 51.5),
        "exit" | "exit-1" => (-77.5, 39.0),
        "exit-2" => (-122.7, 45.5),
        "reply-relay" | "relay-1" => (103.8, 1.35),
        "relay-2" => (139.7, 35.7),
        "chain" => (-42.0, 14.0),
        _ => (0.0, 0.0),
    }
}

pub fn city(role: &str) -> &'static str {
    match role {
        "gateway" | "gw-1" => "Mumbai",
        "gw-2" => "Chennai",
        "mix" | "mix-1" => "Frankfurt",
        "mix-2" => "London",
        "exit" | "exit-1" => "N. Virginia",
        "exit-2" => "Oregon",
        "reply-relay" | "relay-1" => "Singapore",
        "relay-2" => "Tokyo",
        _ => "",
    }
}

pub fn node_name(role: &str) -> &str {
    match role {
        "gw-1" => "Gateway 1",
        "gw-2" => "Gateway 2",
        "mix-1" => "Mix 1",
        "mix-2" => "Mix 2",
        "exit-1" => "Exit 1",
        "exit-2" => "Exit 2",
        "relay-1" => "Relay 1",
        "relay-2" => "Relay 2",
        "gateway" => "Gateway",
        "mix" => "Mix",
        "exit" => "Exit",
        "reply-relay" => "Reply Relay",
        _ => role,
    }
}

pub fn node_tag(role: &str) -> &'static str {
    match role {
        "gw-1" => "GW1",
        "gw-2" => "GW2",
        "mix-1" => "MIX1",
        "mix-2" => "MIX2",
        "exit-1" => "EXT1",
        "exit-2" => "EXT2",
        "relay-1" => "REL1",
        "relay-2" => "REL2",
        "gateway" => "GW ",
        "mix" => "MIX",
        "exit" => "EXT",
        "reply-relay" => "REL",
        _ => "NOD",
    }
}

// ── Background Hops ──────────────────────────────────────────────────────────

/// 4-node single cluster fallback hops
pub const HOPS: [(&str, &str, Color); 8] = [
    ("you", "gateway", RED),
    ("gateway", "mix", RED),
    ("mix", "exit", RED),
    ("exit", "chain", AMBER),
    ("exit", "reply-relay", PALE),
    ("reply-relay", "mix", PALE),
    ("mix", "gateway", PALE),
    ("gateway", "you", PALE),
];

/// Stratified 8-node cluster static routes:
/// Parallel, balanced and non-overlapping streams
pub const STRATIFIED_HOPS: [(&str, &str, Color); 14] = [
    // Forward Stream 1 (Upper)
    ("you", "gw-1", RED),
    ("gw-1", "mix-1", RED),
    ("mix-1", "exit-1", RED),
    ("exit-1", "chain", AMBER),
    // Forward Stream 2 (Lower)
    ("you", "gw-2", RED),
    ("gw-2", "mix-2", RED),
    ("mix-2", "exit-2", RED),
    ("exit-2", "chain", AMBER),
    // Reply Loop 1: Exit 1 → Relay 1 → Mix 1 → GW 1 → You
    ("exit-1", "relay-1", PALE),
    ("relay-1", "mix-1", PALE),
    ("mix-1", "gw-1", PALE),
    ("gw-1", "you", PALE),
    // Reply Loop 2: Exit 2 → Relay 2 → Mix 2
    ("exit-2", "relay-2", PALE),
    ("relay-2", "mix-2", PALE),
];

// ── Smooth Bezier Calculation ────────────────────────────────────────────────

fn bend(a: &str, b: &str) -> f64 {
    let is_reply = a.contains("exit") && b.contains("relay")
        || a.contains("relay")
        || (a.contains("mix") && b.contains("gw"))
        || (a.contains("gw") && b == "you");

    if is_reply { -1.0 } else { 1.0 }
}

pub fn arc(a: &str, b: &str, s: f64) -> (f64, f64) {
    let (p, q) = (place(a), place(b));
    let dx = q.0 - p.0;
    let dy = q.1 - p.1;
    let d = (dx * dx + dy * dy).sqrt();

    if d < 1e-6 {
        return p;
    }

    let nx = -dy / d;
    let ny = dx / d;

    // Gentle curvature so routes look sleek and intentional
    let lift = d * 0.18 * bend(a, b);

    let cx = (p.0 + q.0) / 2.0 + nx * lift;
    let cy = (p.1 + q.1) / 2.0 + ny * lift;

    let u = 1.0 - s;
    (u * u * p.0 + 2.0 * u * s * cx + s * s * q.0, u * u * p.1 + 2.0 * u * s * cy + s * s * q.1)
}

// ── Packet Flights ───────────────────────────────────────────────────────────

pub struct Flight {
    pub route: Vec<&'static str>,
    pub t: f64,
    pub speed: f64,
    pub color: Color,
    pub heavy: bool,
}

impl Flight {
    pub fn new(route: &[&'static str], delay: f64, speed: f64, color: Color) -> Flight {
        Flight { route: route.to_vec(), t: -delay, speed, color, heavy: false }
    }

    pub fn done(&self) -> bool {
        self.t >= (self.route.len() - 1) as f64
    }

    pub fn position(&self, t: f64) -> Option<(f64, f64)> {
        if t < 0.0 || self.route.len() < 2 {
            return None;
        }
        let i = (t.floor() as usize).min(self.route.len() - 2);
        Some(arc(self.route[i], self.route[i + 1], (t - i as f64).min(1.0)))
    }
}
