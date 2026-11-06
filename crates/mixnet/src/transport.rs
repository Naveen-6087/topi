//! TCP transport: one connection per packet.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use topi_sphinx::PACKET_SIZE;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn connect(addr: &str) -> std::io::Result<TcpStream> {
    tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("connect to {addr} timed out")))?
}

pub async fn send(addr: &str, bytes: &[u8]) -> std::io::Result<()> {
    connect(addr).await?.write_all(bytes).await
}

pub async fn read_packet(mut s: TcpStream) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; PACKET_SIZE];
    s.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

/// An exponentially distributed delay with the given mean (the mixing delay distribution).
pub fn exp_delay_ms(mean_ms: f64) -> u32 {
    let u = ((random_u64() >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0);
    (-u.ln() * mean_ms) as u32
}

pub fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, (random_u64() as usize) % (i + 1));
    }
}

pub fn random_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("system randomness");
    u64::from_le_bytes(b)
}
