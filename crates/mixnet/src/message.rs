//! Messages inside packet payloads. A message is split into fragments; the first one carries the
//! sender's reply block (SURB).
//!
//! Fragment: `kind u8 | message id u64 | index u16 | count u16 | has_surb u8 | [SURB header] | data`.

use topi_sphinx::{HEADER, PAYLOAD, TAG};

pub const KIND_COVER: u8 = 0;
pub const KIND_DATA: u8 = 2;
pub const KIND_PING: u8 = 3;

const FIXED: usize = 14;
/// Data bytes in a message's first fragment (which also carries the SURB header and 2-byte reply relay ID).
pub const FIRST_CAPACITY: usize = PAYLOAD - TAG - 2 - FIXED - 2 - HEADER;
/// Data bytes in every further fragment.
pub const NEXT_CAPACITY: usize = PAYLOAD - TAG - 2 - FIXED;

#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub kind: u8,
    pub id: u64,
    pub index: u16,
    pub count: u16,
    pub relay_id: Option<u16>,
    pub surb: Option<Vec<u8>>,
    pub data: Vec<u8>,
}

impl Fragment {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = vec![self.kind];
        v.extend_from_slice(&self.id.to_le_bytes());
        v.extend_from_slice(&self.index.to_le_bytes());
        v.extend_from_slice(&self.count.to_le_bytes());
        match (&self.surb, self.relay_id) {
            (Some(surb), Some(relay)) => {
                v.push(2);
                v.extend_from_slice(&relay.to_le_bytes());
                v.extend_from_slice(surb);
            }
            (Some(surb), None) => {
                v.push(1);
                v.extend_from_slice(surb);
            }
            (None, _) => v.push(0),
        }
        v.extend_from_slice(&self.data);
        v
    }

    /// An empty payload is cover traffic.
    pub fn decode(b: &[u8]) -> Option<Fragment> {
        if b.is_empty() {
            return Some(Fragment { kind: KIND_COVER, id: 0, index: 0, count: 0, relay_id: None, surb: None, data: vec![] });
        }
        if b.len() < FIXED {
            return None;
        }
        let mut off = FIXED;
        let (relay_id, surb) = if b[13] == 2 {
            let rid = u16::from_le_bytes([*b.get(off)?, *b.get(off + 1)?]);
            off += 2;
            let s = b.get(off..off + HEADER)?.to_vec();
            off += HEADER;
            (Some(rid), Some(s))
        } else if b[13] == 1 {
            let s = b.get(off..off + HEADER)?.to_vec();
            off += HEADER;
            (None, Some(s))
        } else {
            (None, None)
        };
        Some(Fragment {
            kind: b[0],
            id: u64::from_le_bytes(b[1..9].try_into().ok()?),
            index: u16::from_le_bytes(b[9..11].try_into().ok()?),
            count: u16::from_le_bytes(b[11..13].try_into().ok()?),
            relay_id,
            surb,
            data: b[off..].to_vec(),
        })
    }
}

/// Fragments a message of `len` bytes needs.
pub fn fragment_count(len: usize) -> usize {
    1 + len.saturating_sub(FIRST_CAPACITY).div_ceil(NEXT_CAPACITY)
}

/// Split `data` into fragments; `surb` and `relay_id` ride on the first.
pub fn fragment(kind: u8, id: u64, relay_id: u16, surb: Vec<u8>, data: &[u8]) -> Vec<Fragment> {
    let first = FIRST_CAPACITY.min(data.len());
    let rest = &data[first..];
    let count = fragment_count(data.len()) as u16;
    let mut out = vec![Fragment { kind, id, index: 0, count, relay_id: Some(relay_id), surb: Some(surb), data: data[..first].to_vec() }];
    for (i, chunk) in rest.chunks(NEXT_CAPACITY).enumerate() {
        out.push(Fragment { kind, id, index: i as u16 + 1, count, relay_id: None, surb: None, data: chunk.to_vec() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_round_trip() {
        let data: Vec<u8> = (0..FIRST_CAPACITY + 2 * NEXT_CAPACITY + 7).map(|i| i as u8).collect();
        let frags = fragment(KIND_DATA, 42, 4, vec![7; HEADER], &data);
        assert_eq!(frags.len(), fragment_count(data.len()));
        assert_eq!(frags.len(), 4);
        let decoded: Vec<Fragment> = frags.iter().map(|f| Fragment::decode(&f.encode()).unwrap()).collect();
        assert_eq!(decoded, frags);
        let joined: Vec<u8> = decoded.into_iter().flat_map(|f| f.data).collect();
        assert_eq!(joined, data);
    }

    #[test]
    fn empty_payload_is_cover() {
        assert_eq!(Fragment::decode(&[]).unwrap().kind, KIND_COVER);
    }
}
