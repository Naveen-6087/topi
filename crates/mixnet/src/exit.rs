//! The exit: reassembles messages and answers them on their reply blocks. A pool spend is checked for
//! framing only and queued on disk for the exit prover, which validates, pairs, proves and settles it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};
use topi_sphinx::decode_payload;

use crate::events::{Event, Events, Reject};
use crate::message::{Fragment, KIND_COVER, KIND_DATA, KIND_PING};

/// Prefix of a spend submission: `SPEND_TAG | header length u32 BE | header JSON | client proof (.tar.gz)`.
/// The header is `{"fields": [...]}`, the spend's public settle fields.
pub const SPEND_TAG: &[u8] = b"pq-spend/v1";
pub const SPEND_FIELDS: usize = 82;

pub fn encode_spend(fields: &Value, proof: &[u8]) -> Vec<u8> {
    let header = json!({ "fields": fields }).to_string();
    let mut v = SPEND_TAG.to_vec();
    v.extend_from_slice(&(header.len() as u32).to_be_bytes());
    v.extend_from_slice(header.as_bytes());
    v.extend_from_slice(proof);
    v
}

struct Partial {
    kind: u8,
    parts: Vec<Option<Vec<u8>>>,
    surb: Option<Vec<u8>>,
}

pub struct Exit {
    queue: PathBuf,
    partial: Mutex<HashMap<u64, Partial>>,
}

/// A finished message's answer: the reply block to send it on, and the reply body.
pub struct Answer {
    pub surb: Vec<u8>,
    pub body: Vec<u8>,
}

impl Exit {
    pub fn new(queue: PathBuf) -> Exit {
        Exit { queue, partial: Mutex::default() }
    }

    /// Take one payload; when it completes a message, return the answer to send back.
    pub fn accept(&self, payload: &[u8], events: &Events) -> Option<Answer> {
        let Some(body) = decode_payload(payload) else {
            let _ = events.send(Event::Rejected(Reject::Integrity));
            return None;
        };
        let frag = Fragment::decode(&body)?;
        if frag.kind == KIND_COVER {
            let _ = events.send(Event::Cover);
            return None;
        }
        let (kind, parts, surb) = {
            let mut partial = self.partial.lock().unwrap();
            let p =
                partial.entry(frag.id).or_insert_with(|| Partial { kind: frag.kind, parts: vec![None; frag.count as usize], surb: None });
            if let Some(slot) = p.parts.get_mut(frag.index as usize) {
                *slot = Some(frag.data);
            }
            if frag.surb.is_some() {
                p.surb = frag.surb;
            }
            if !(p.parts.iter().all(Option::is_some) && p.surb.is_some()) {
                return None;
            }
            let p = partial.remove(&frag.id)?;
            (p.kind, p.parts, p.surb?)
        };
        let message: Vec<u8> = parts.into_iter().flatten().flatten().collect();
        let reply = self.respond(kind, &message, events);
        let mut body = frag.id.to_le_bytes().to_vec();
        body.extend_from_slice(reply.to_string().as_bytes());
        Some(Answer { surb, body })
    }

    fn respond(&self, kind: u8, message: &[u8], events: &Events) -> Value {
        match kind {
            KIND_PING => json!({ "ok": true, "pong": true }),
            KIND_DATA if message.starts_with(SPEND_TAG) => self.queue_spend(&message[SPEND_TAG.len()..], message.len(), events),
            _ => json!({ "ok": false, "reason": "unsupported message" }),
        }
    }

    fn queue_spend(&self, body: &[u8], total: usize, events: &Events) -> Value {
        let header_len = match body.get(..4) {
            Some(b) => u32::from_be_bytes(b.try_into().unwrap()) as usize,
            None => return json!({ "ok": false, "reason": "malformed spend" }),
        };
        let Some(header) = body.get(4..4 + header_len).and_then(|h| serde_json::from_slice::<Value>(h).ok()) else {
            return json!({ "ok": false, "reason": "malformed spend header" });
        };
        if header["fields"].as_array().map(Vec::len) != Some(SPEND_FIELDS) {
            return json!({ "ok": false, "reason": format!("a spend carries {SPEND_FIELDS} public fields") });
        }
        let id = format!("{:016x}", crate::transport::random_u64());
        let written = std::fs::create_dir_all(&self.queue).is_ok()
            && std::fs::write(self.queue.join(format!("{id}.tgz")), &body[4 + header_len..]).is_ok()
            && std::fs::write(self.queue.join(format!("{id}.json")), header.to_string()).is_ok();
        if !written {
            return json!({ "ok": false, "reason": "the exit could not queue the spend" });
        }
        let _ = events.send(Event::SpendQueued(format!("spend {id} queued for the prover ({} KB)", total / 1024)));
        json!({ "ok": true, "queued": id, "bytes": total })
    }
}
