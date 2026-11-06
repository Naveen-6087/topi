//! The reply store at the gateway. A reply arrives on its sender's single-use reply block and waits here
//! under the slot the sender chose; the sender fetches it later, so clients never accept connections.
//! Stored packets are sealed to their sender's SURB keys.
//!
//! Requests: `P | slot u16 | packet` stores a packet; `G | slot u16` returns and removes the slot's packets.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use topi_sphinx::PACKET_SIZE;

use crate::transport::connect;

/// Most packets held at once; the oldest are dropped first.
const CAPACITY: usize = 4096;

/// Waiting packets with their slot, oldest first.
type Store = Arc<Mutex<Vec<(u16, Vec<u8>)>>>;

pub async fn serve(bind: &str, port: u16) -> std::io::Result<()> {
    let store: Store = Arc::default();
    let listener = TcpListener::bind((bind, port)).await?;
    loop {
        let Ok((mut s, _)) = listener.accept().await else { continue };
        let store = store.clone();
        tokio::spawn(async move {
            let mut buf = Vec::new();
            if s.read_to_end(&mut buf).await.is_err() || buf.len() < 3 {
                return;
            }
            let slot = u16::from_le_bytes([buf[1], buf[2]]);
            match buf[0] {
                b'P' if buf.len() == 3 + PACKET_SIZE => {
                    let mut st = store.lock().unwrap();
                    if st.len() >= CAPACITY {
                        st.remove(0);
                    }
                    st.push((slot, buf[3..].to_vec()));
                }
                b'G' => {
                    let mine: Vec<u8> = {
                        let mut st = store.lock().unwrap();
                        let (take, keep): (Vec<_>, Vec<_>) = st.drain(..).partition(|(s, _)| *s == slot);
                        *st = keep;
                        take.into_iter().flat_map(|(_, p)| p).collect()
                    };
                    let _ = s.write_all(&mine).await;
                }
                _ => {}
            }
        });
    }
}

pub async fn deposit(addr: &str, slot: u16, packet: &[u8]) -> std::io::Result<()> {
    let mut framed = vec![b'P'];
    framed.extend_from_slice(&slot.to_le_bytes());
    framed.extend_from_slice(packet);
    framed.resize(3 + PACKET_SIZE, 0);
    let mut s = connect(addr).await?;
    s.write_all(&framed).await?;
    s.shutdown().await
}

pub async fn fetch(addr: &str, slot: u16) -> std::io::Result<Vec<Vec<u8>>> {
    let mut s = connect(addr).await?;
    let mut req = vec![b'G'];
    req.extend_from_slice(&slot.to_le_bytes());
    s.write_all(&req).await?;
    s.shutdown().await?;
    let mut all = Vec::new();
    s.read_to_end(&mut all).await?;
    Ok(all.as_chunks::<PACKET_SIZE>().0.iter().map(|p| p.to_vec()).collect())
}
