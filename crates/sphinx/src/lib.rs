//! KEM-Sphinx: onion packets with a key encapsulation (X-Wing) per hop.
//!
//! Each hop gets its own X-Wing (X25519 + ML-KEM-768) ciphertext. The header is
//! built with the classic Sphinx "filler" technique so every hop sees a header of
//! exactly the same size, and every packet is exactly `PACKET_SIZE` bytes.
//!
//! Simplifications for the prototype (documented, not hidden):
//! - Layer encryption uses the BLAKE3 XOF as a stream cipher.
//! - The payload uses a plain stream layer per hop, not a wide-block cipher (Lioness);
//!   integrity is checked end to end with a tag at the final recipient.

use x_wing::{Decapsulate, Encapsulate, Kem, KeyExport, XWingKem};

pub use x_wing::{DecapsulationKey, EncapsulationKey};

/// Number of mix hops in every route.
pub const HOPS: usize = 3;
/// X-Wing ciphertext size.
pub const CT: usize = x_wing::CIPHERTEXT_SIZE; // 1120
/// MAC size.
pub const MAC: usize = 16;
/// Routing information per hop.
pub const ROUTING: usize = 32;
/// One header entry: routing + next MAC + next ciphertext.
pub const ENTRY: usize = ROUTING + MAC + CT;
/// Encrypted routing region length.
pub const BETA: usize = (HOPS - 1) * ENTRY + ROUTING;
/// Header size: ciphertext + MAC + beta.
pub const HEADER: usize = CT + MAC + BETA;
/// Every packet is exactly this many bytes.
pub const PACKET_SIZE: usize = 8192;
/// Payload size.
pub const PAYLOAD: usize = PACKET_SIZE - HEADER;
/// Integrity tag at the start of the plaintext payload.
pub const TAG: usize = 16;

const _: () = assert!(HEADER == HOPS * ENTRY);

/// What a hop does after processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routing {
    /// True if this hop is the last one (deliver the payload).
    pub is_exit: bool,
    /// Next node id (ignored at exit) or destination id (at exit).
    pub next: u16,
    /// Delay in milliseconds this hop should hold the packet.
    pub delay_ms: u32,
}

impl Routing {
    fn encode(&self) -> [u8; ROUTING] {
        let mut b = [0u8; ROUTING];
        b[0] = self.is_exit as u8;
        b[1..3].copy_from_slice(&self.next.to_le_bytes());
        b[3..7].copy_from_slice(&self.delay_ms.to_le_bytes());
        b
    }
    fn decode(b: &[u8]) -> Self {
        Routing { is_exit: b[0] == 1, next: u16::from_le_bytes([b[1], b[2]]), delay_ms: u32::from_le_bytes([b[3], b[4], b[5], b[6]]) }
    }
}

/// Keys derived from one hop's shared secret.
#[derive(Clone)]
pub struct HopKeys {
    mac: [u8; 32],
    header: [u8; 32],
    payload: [u8; 32],
    replay: [u8; 32],
}

impl HopKeys {
    /// 128-byte encoding, for a client that must remember a SURB's keys across a restart.
    pub fn to_bytes(&self) -> Vec<u8> {
        [self.mac, self.header, self.payload, self.replay].concat()
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != 128 {
            return None;
        }
        let k = |i: usize| -> [u8; 32] { b[i * 32..(i + 1) * 32].try_into().unwrap() };
        Some(HopKeys { mac: k(0), header: k(1), payload: k(2), replay: k(3) })
    }
    fn derive(ss: &[u8]) -> Self {
        HopKeys {
            mac: blake3::derive_key("kem-sphinx v1 header mac", ss),
            header: blake3::derive_key("kem-sphinx v1 header stream", ss),
            payload: blake3::derive_key("kem-sphinx v1 payload stream", ss),
            replay: blake3::derive_key("kem-sphinx v1 replay tag", ss),
        }
    }
}

fn stream(key: &[u8; 32], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    blake3::Hasher::new_keyed(key).finalize_xof().fill(&mut out);
    out
}

fn xor_in_place(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.iter_mut().zip(src) {
        *d ^= *s;
    }
}

fn mac(key: &[u8; 32], data: &[u8]) -> [u8; MAC] {
    let h = blake3::keyed_hash(key, data);
    let mut out = [0u8; MAC];
    out.copy_from_slice(&h.as_bytes()[..MAC]);
    out
}

/// Generate a node keypair.
pub fn keygen() -> (DecapsulationKey, EncapsulationKey) {
    XWingKem::generate_keypair()
}

/// A node's secret key as its 32-byte X-Wing seed (for the node's key file).
pub fn sk_bytes(sk: &DecapsulationKey) -> [u8; 32] {
    *sk.as_bytes()
}

/// Restore a node keypair from its 32-byte seed.
pub fn keypair_from_seed(seed: [u8; 32]) -> (DecapsulationKey, EncapsulationKey) {
    let sk = DecapsulationKey::from(seed);
    let pk = x_wing::Decapsulator::encapsulation_key(&sk).clone();
    (sk, pk)
}

/// Serialize a public key (1216 bytes) for the topology.
pub fn pk_bytes(pk: &EncapsulationKey) -> Vec<u8> {
    pk.to_bytes().to_vec()
}

/// Parse a public key from the topology.
pub fn pk_from_bytes(b: &[u8]) -> EncapsulationKey {
    EncapsulationKey::try_from(b).expect("valid x-wing public key")
}

/// A header plus the per-hop keys the sender keeps (needed for replies).
pub struct BuiltHeader {
    pub header: Vec<u8>,
    pub keys: Vec<HopKeys>,
}

/// Build a header for `route` (public keys in order) with per-hop routing info.
/// The last routing entry must have `is_exit = true`.
pub fn build_header(route: &[EncapsulationKey], routing: &[Routing]) -> BuiltHeader {
    assert_eq!(route.len(), HOPS);
    assert_eq!(routing.len(), HOPS);
    assert!(routing[HOPS - 1].is_exit);

    // Encapsulate to every hop.
    let mut cts = Vec::with_capacity(HOPS);
    let mut keys = Vec::with_capacity(HOPS);
    for pk in route {
        let (ct, ss) = pk.encapsulate();
        cts.push(ct.to_vec());
        keys.push(HopKeys::derive(ss.as_slice()));
    }

    // Filler so the tail stays consistent as each hop shifts the header.
    let mut phi: Vec<u8> = Vec::new();
    for k in keys.iter().take(HOPS - 1) {
        phi.extend_from_slice(&[0u8; ENTRY]);
        let rho = stream(&k.header, BETA + ENTRY);
        let start = BETA + ENTRY - phi.len();
        xor_in_place(&mut phi, &rho[start..]);
    }

    // Innermost (last hop) beta.
    let last = HOPS - 1;
    let mut beta = routing[last].encode().to_vec();
    let rho = stream(&keys[last].header, BETA + ENTRY);
    xor_in_place(&mut beta, &rho[..ROUTING]);
    beta.extend_from_slice(&phi);
    assert_eq!(beta.len(), BETA);
    let mut gamma = mac(&keys[last].mac, &beta);

    // Wrap outward.
    for i in (0..last).rev() {
        let mut plain = Vec::with_capacity(ENTRY + BETA);
        plain.extend_from_slice(&routing[i].encode());
        plain.extend_from_slice(&gamma);
        plain.extend_from_slice(&cts[i + 1]);
        plain.extend_from_slice(&beta);
        let rho = stream(&keys[i].header, BETA + ENTRY);
        xor_in_place(&mut plain, &rho);
        plain.truncate(BETA);
        beta = plain;
        gamma = mac(&keys[i].mac, &beta);
    }

    let mut header = Vec::with_capacity(HEADER);
    header.extend_from_slice(&cts[0]);
    header.extend_from_slice(&gamma);
    header.extend_from_slice(&beta);
    assert_eq!(header.len(), HEADER);
    BuiltHeader { header, keys }
}

/// Build a full forward packet carrying `message` (at most PAYLOAD - TAG - 2 bytes).
pub fn build_packet(route: &[EncapsulationKey], routing: &[Routing], message: &[u8]) -> Vec<u8> {
    let built = build_header(route, routing);
    let mut payload = encode_payload(message);
    // Pre-apply every hop's payload layer; each hop removes one (XOR).
    for k in &built.keys {
        xor_in_place(&mut payload, &stream(&k.payload, PAYLOAD));
    }
    let mut pkt = built.header;
    pkt.extend_from_slice(&payload);
    assert_eq!(pkt.len(), PACKET_SIZE);
    pkt
}

/// Plaintext payload: tag || len (u16) || message || zero padding.
pub fn encode_payload(message: &[u8]) -> Vec<u8> {
    assert!(message.len() <= PAYLOAD - TAG - 2, "message too large");
    let mut p = vec![0u8; PAYLOAD];
    p[TAG..TAG + 2].copy_from_slice(&(message.len() as u16).to_le_bytes());
    p[TAG + 2..TAG + 2 + message.len()].copy_from_slice(message);
    let t = blake3::hash(&p[TAG..]);
    p[..TAG].copy_from_slice(&t.as_bytes()[..TAG]);
    p
}

/// Check the tag and extract the message; None if the payload was tampered with.
pub fn decode_payload(p: &[u8]) -> Option<Vec<u8>> {
    let t = blake3::hash(&p[TAG..]);
    if t.as_bytes()[..TAG] != p[..TAG] {
        return None;
    }
    let len = u16::from_le_bytes([p[TAG], p[TAG + 1]]) as usize;
    if TAG + 2 + len > p.len() {
        return None;
    }
    Some(p[TAG + 2..TAG + 2 + len].to_vec())
}

#[derive(Debug, PartialEq, Eq)]
pub enum HopError {
    BadSize,
    BadMac,
    Replay,
}

/// Result of processing a packet at a hop.
pub struct Processed {
    pub routing: Routing,
    /// The packet to forward (same size), or at exit the decrypted payload.
    pub packet: Vec<u8>,
    /// Replay tag to remember for this key epoch.
    pub replay_tag: [u8; 32],
}

/// Process one packet at a hop: decapsulate, verify MAC, peel a layer, shift header.
/// `seen` is the replay cache for the current key epoch.
pub fn process(sk: &DecapsulationKey, packet: &[u8], seen: &mut std::collections::HashSet<[u8; 32]>) -> Result<Processed, HopError> {
    if packet.len() != PACKET_SIZE {
        return Err(HopError::BadSize);
    }
    let (ct, rest) = packet.split_at(CT);
    let (gamma, rest) = rest.split_at(MAC);
    let (beta, payload) = rest.split_at(BETA);

    let ss = sk.decapsulate_slice(ct).map_err(|_| HopError::BadSize)?;
    let k = HopKeys::derive(ss.as_slice());

    if seen.contains(&k.replay) {
        return Err(HopError::Replay);
    }
    if mac(&k.mac, beta) != gamma {
        return Err(HopError::BadMac);
    }

    let mut tmp = beta.to_vec();
    tmp.extend_from_slice(&[0u8; ENTRY]);
    xor_in_place(&mut tmp, &stream(&k.header, BETA + ENTRY));

    let routing = Routing::decode(&tmp[..ROUTING]);
    let mut body = payload.to_vec();
    xor_in_place(&mut body, &stream(&k.payload, PAYLOAD));
    seen.insert(k.replay);

    if routing.is_exit {
        return Ok(Processed { routing, packet: body, replay_tag: k.replay });
    }

    let next_gamma = &tmp[ROUTING..ROUTING + MAC];
    let next_ct = &tmp[ROUTING + MAC..ENTRY];
    let next_beta = &tmp[ENTRY..ENTRY + BETA];
    let mut out = Vec::with_capacity(PACKET_SIZE);
    out.extend_from_slice(next_ct);
    out.extend_from_slice(next_gamma);
    out.extend_from_slice(next_beta);
    out.extend_from_slice(&body);
    debug_assert_eq!(out.len(), PACKET_SIZE);
    Ok(Processed { routing, packet: out, replay_tag: k.replay })
}

/// A single-use reply block: the header the replier attaches, plus the keys
/// the original sender keeps to remove the reply's layers.
pub struct Surb {
    pub header: Vec<u8>,
    pub keys: Vec<HopKeys>,
}

/// Build a SURB routed back through `route` to the sender.
pub fn build_surb(route: &[EncapsulationKey], routing: &[Routing]) -> Surb {
    let b = build_header(route, routing);
    Surb { header: b.header, keys: b.keys }
}

/// Replier side: attach a reply message to a SURB header.
pub fn reply_packet(surb_header: &[u8], message: &[u8]) -> Vec<u8> {
    let mut pkt = surb_header.to_vec();
    pkt.extend_from_slice(&encode_payload(message));
    pkt
}

/// Sender side: the reply payload arrives with every hop's layer applied; remove them.
pub fn open_reply(keys: &[HopKeys], mut payload: Vec<u8>) -> Option<Vec<u8>> {
    // The last hop already applied its layer during `process`, and so did the others:
    // each hop XORs its payload stream, so XOR all of them again to undo.
    for k in keys {
        xor_in_place(&mut payload, &stream(&k.payload, PAYLOAD));
    }
    decode_payload(&payload)
}

/// A random cover packet that is indistinguishable in size from a real one.
pub fn cover_packet(route: &[EncapsulationKey], routing: &[Routing]) -> Vec<u8> {
    build_packet(route, routing, b"")
}

/// Fill with OS randomness.
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    getrandom::fill(&mut v).expect("os rng");
    v
}
