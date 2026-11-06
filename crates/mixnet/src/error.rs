use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    #[error("invalid topology: {0}")]
    Topology(String),
    #[error("rate limit: {0}")]
    RateLimit(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("timed out waiting for the reply")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
