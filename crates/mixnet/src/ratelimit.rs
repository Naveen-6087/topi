//! Anonymous rate limiting at the gateway. A member proves once per epoch, in zero knowledge, that it
//! holds a registered identity (a Binius64 ticket over SHA-256); every packet then carries a small hash
//! token. The gateway admits at most `tokens_per_epoch` messages per member per epoch without learning
//! which member sent them.
//!
//! Frames: ticket `1 | len u32 | ticket`; packet `2 | token root [32] | len u16 | token | packet`;
//! continuation `3 | token value [32] | packet`, admitted while that token's budget lasts. A message spends
//! one token on its first packet and rides its remaining packets on that token's continuation budget.
//! The token value travels in the clear to the gateway, so an on-path observer could exhaust one message's
//! continuation budget; it cannot mint new budget.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use topi_ratelimit::{AcceptResult, Digest, EpochTicket, Identity, RateLimiter, Token};
use topi_sphinx::PACKET_SIZE;

use crate::events::{Event, Events, Reject};
use crate::topology::{EPOCH_SECS, TOKENS_PER_EPOCH, Topology, membership};
use crate::transport::{self, connect};
use crate::{Error, Result, Role};

/// Packets one token can carry after the first (a 1.3 MB spend is about 280 packets).
pub const CONTINUATIONS_PER_TOKEN: u32 = 512;

pub fn current_epoch() -> u64 {
    crate::events::now_secs() / EPOCH_SECS
}

/// Continuation budgets by token value, with the epoch each was granted in. A budget stays valid
/// through the next epoch too, so a long message that straddles an epoch boundary is not cut off.
type Budgets = Arc<Mutex<HashMap<[u8; 32], (u64, u32)>>>;

pub async fn serve(bind: &str, port: u16, topo: Arc<Topology>, events: Events) -> Result<()> {
    let root = topo.root()?;
    let system = topi_ratelimit::system(topi_ratelimit::Params::default()).map_err(|e| Error::RateLimit(format!("{e:?}")))?;
    let limiter = Arc::new(Mutex::new(RateLimiter::new(system, root, current_epoch())));
    let budgets: Budgets = Arc::default();
    let listener = TcpListener::bind((bind, port)).await?;
    loop {
        let Ok((s, _)) = listener.accept().await else { continue };
        let (limiter, budgets, events, topo) = (limiter.clone(), budgets.clone(), events.clone(), topo.clone());
        tokio::spawn(async move {
            let _ = handle(s, limiter, budgets, root, &topo, events).await;
        });
    }
}

async fn handle(
    mut s: TcpStream,
    limiter: Arc<Mutex<RateLimiter>>,
    budgets: Budgets,
    root: Digest,
    topo: &Topology,
    events: Events,
) -> Result<()> {
    let mut kind = [0u8; 1];
    s.read_exact(&mut kind).await?;
    let epoch = current_epoch();
    {
        let mut l = limiter.lock().unwrap();
        if l.epoch() != epoch {
            l.reset(root, epoch);
        }
    }
    let gateway = topo.addr(Role::Gateway.id())?;
    match kind[0] {
        1 => {
            let mut len = [0u8; 4];
            s.read_exact(&mut len).await?;
            let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
            s.read_exact(&mut buf).await?;
            let ok = match EpochTicket::from_bytes(&buf) {
                Some(t) => {
                    let l = limiter.clone();
                    tokio::task::spawn_blocking(move || l.lock().unwrap().register(&t)).await.unwrap_or(false)
                }
                None => false,
            };
            let _ = events.send(Event::Ticket(ok, buf.len()));
            s.write_all(&[ok as u8]).await?;
        }
        2 => {
            let mut r = [0u8; 32];
            s.read_exact(&mut r).await?;
            let mut len = [0u8; 2];
            s.read_exact(&mut len).await?;
            let mut tok = vec![0u8; u16::from_be_bytes(len) as usize];
            s.read_exact(&mut tok).await?;
            let mut pkt = vec![0u8; PACKET_SIZE];
            s.read_exact(&mut pkt).await?;
            let verdict = match Token::from_bytes(&tok) {
                Some(t) => limiter.lock().unwrap().accept(&r, &t),
                None => AcceptResult::Invalid,
            };
            if verdict == AcceptResult::Accepted {
                if tok.len() >= 32 {
                    let mut b = budgets.lock().unwrap();
                    b.retain(|_, (e, _)| *e + 1 >= epoch);
                    b.insert(tok[..32].try_into().unwrap(), (epoch, CONTINUATIONS_PER_TOKEN));
                }
                transport::send(&gateway, &pkt).await?;
                s.write_all(&[1]).await?;
            } else {
                let _ = events.send(Event::Rejected(Reject::InvalidToken));
                s.write_all(&[0]).await?;
            }
        }
        3 => {
            let mut t = [0u8; 32];
            s.read_exact(&mut t).await?;
            let mut pkt = vec![0u8; PACKET_SIZE];
            s.read_exact(&mut pkt).await?;
            let ok = match budgets.lock().unwrap().get_mut(&t) {
                Some((e, n)) if *n > 0 && *e + 1 >= epoch => {
                    *n -= 1;
                    true
                }
                _ => false,
            };
            if ok {
                transport::send(&gateway, &pkt).await?;
                s.write_all(&[1]).await?;
            } else {
                let _ = events.send(Event::Rejected(Reject::NoBudget));
                s.write_all(&[0]).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Create a member identity at `path` and return its commitment (to be added to the member list).
pub fn new_identity(path: impl AsRef<Path>) -> Result<Digest> {
    let mut sk = [0u8; 32];
    getrandom::fill(&mut sk).expect("system randomness");
    std::fs::write(path, hex::encode(sk))?;
    Ok(Identity::from_secret(sk).commitment())
}

pub fn load_identity(path: impl AsRef<Path>) -> Result<Identity> {
    let text = std::fs::read_to_string(path)?;
    let sk: [u8; 32] = hex::decode(text.trim())
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Error::RateLimit("an identity file holds a 32-byte hex secret".into()))?;
    Ok(Identity::from_secret(sk))
}

/// Exclusive lock file next to the identity, so concurrent senders never reuse a token index.
struct FileLock(PathBuf);

impl FileLock {
    fn acquire(identity: &Path) -> FileLock {
        let lock = identity.with_extension("lock");
        for _ in 0..6000 {
            if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_ok() {
                return FileLock(lock);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&lock);
        let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock);
        FileLock(lock)
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A member's rate-limit state for the current epoch, persisted next to its identity file.
pub struct RateLimitClient {
    identity: PathBuf,
    state: PathBuf,
    id: Identity,
    epoch: u64,
    token_root: Digest,
    gateway: String,
}

impl RateLimitClient {
    /// Make sure the gateway holds this epoch's ticket for `identity` (proving it once per epoch).
    pub async fn connect(topo: &Topology, identity: impl AsRef<Path>) -> Result<RateLimitClient> {
        let identity = identity.as_ref().to_path_buf();
        let _guard = FileLock::acquire(&identity);
        let id = load_identity(&identity)?;
        let state = identity.with_extension("state");
        let epoch = current_epoch();
        let members = topo.members()?;
        let root = topo.root()?;
        let tree = membership(&members);
        let index = tree.position(&id.commitment()).ok_or_else(|| Error::RateLimit("identity is not a registered member".into()))?;
        let already = read_state(&state).is_some_and(|(e, _)| e == epoch);
        if !already {
            let path = tree.path(index).ok_or_else(|| Error::RateLimit("no membership path".into()))?;
            let prover = load_identity(&identity)?;
            let ticket = tokio::task::spawn_blocking(move || topi_ratelimit::prove_epoch(&prover, &path, &root, epoch, TOKENS_PER_EPOCH))
                .await
                .map_err(|e| Error::RateLimit(e.to_string()))?
                .map_err(|e| Error::RateLimit(format!("prove: {e:?}")))?;
            let bytes = ticket.to_bytes();
            let mut frame = vec![1u8];
            frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            frame.extend_from_slice(&bytes);
            if !exchange(&topo.rate_limit, &frame).await? {
                return Err(Error::RateLimit("the gateway refused this epoch's ticket".into()));
            }
            std::fs::write(&state, format!("{epoch} 0"))?;
        }
        Ok(RateLimitClient {
            token_root: topi_ratelimit::token_root(&id, epoch, TOKENS_PER_EPOCH),
            identity,
            state,
            id,
            epoch,
            gateway: topo.rate_limit.clone(),
        })
    }

    /// Send a message's first packet with the next unused token; returns the token value its
    /// further packets ride on.
    pub async fn send_first(&mut self, packet: &[u8]) -> Result<[u8; 32]> {
        let index = {
            let _guard = FileLock::acquire(&self.identity);
            let next = read_state(&self.state).filter(|(e, _)| *e == self.epoch).map(|(_, n)| n).unwrap_or(0);
            std::fs::write(&self.state, format!("{} {}", self.epoch, next + 1))?;
            next
        };
        let tok = topi_ratelimit::token(&self.id, self.epoch, index as u32, TOKENS_PER_EPOCH)
            .ok_or_else(|| Error::RateLimit("no tokens left this epoch".into()))?
            .to_bytes();
        let mut frame = vec![2u8];
        frame.extend_from_slice(&self.token_root);
        frame.extend_from_slice(&(tok.len() as u16).to_be_bytes());
        frame.extend_from_slice(&tok);
        frame.extend_from_slice(packet);
        let value: [u8; 32] = tok.get(..32).and_then(|v| v.try_into().ok()).ok_or_else(|| Error::RateLimit("short token".into()))?;
        if !exchange(&self.gateway, &frame).await? {
            return Err(Error::RateLimit("the gateway refused the packet".into()));
        }
        Ok(value)
    }

    pub async fn send_continuation(&self, token: &[u8; 32], packet: &[u8]) -> Result<()> {
        let mut frame = vec![3u8];
        frame.extend_from_slice(token);
        frame.extend_from_slice(packet);
        if !exchange(&self.gateway, &frame).await? {
            return Err(Error::RateLimit("the gateway refused a continuation".into()));
        }
        Ok(())
    }
}

fn read_state(path: &Path) -> Option<(u64, u64)> {
    let v: Vec<u64> = std::fs::read_to_string(path).ok()?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (v.len() == 2).then(|| (v[0], v[1]))
}

async fn exchange(addr: &str, frame: &[u8]) -> Result<bool> {
    let mut s = connect(addr).await?;
    s.write_all(frame).await?;
    let mut ok = [0u8; 1];
    s.read_exact(&mut ok).await?;
    Ok(ok[0] == 1)
}
