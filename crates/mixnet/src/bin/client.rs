//! topi-client: register a member identity, ping the network, and submit pool spends.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use topi_mixnet::message::fragment_count;
use topi_mixnet::{Client, Result, Topology, exit, ratelimit};

#[derive(Parser)]
#[command(version, about = "Post-quantum mixnet client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct Connection {
    #[arg(long, env = "TOPI_TOPOLOGY")]
    topology: PathBuf,
    /// Member identity file (from `register`).
    #[arg(long, env = "TOPI_IDENTITY")]
    identity: PathBuf,
    /// Seconds to wait for the reply.
    #[arg(long, default_value_t = 300)]
    timeout: u64,
}

#[derive(Subcommand)]
enum Command {
    /// Create a member identity and append its commitment to the member list.
    Register {
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        members: PathBuf,
    },
    /// A round trip through every hop and back.
    Ping(Connection),
    /// Submit a pool spend to the exit's prover queue.
    Spend {
        #[command(flatten)]
        conn: Connection,
        /// The client proof (.tar.gz).
        #[arg(long)]
        proof: PathBuf,
        /// JSON with the spend's public settle fields: {"fields": [...]}.
        #[arg(long)]
        public: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Register { identity, members } => {
            let commitment = ratelimit::new_identity(&identity)?;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&members)?;
            writeln!(f, "{}", hex::encode(commitment))?;
            println!("registered member {}", hex::encode(commitment));
        }
        Command::Ping(conn) => {
            let start = Instant::now();
            let mut client = Client::connect(Topology::load(&conn.topology)?, &conn.identity).await?;
            let reply = client.ping(Duration::from_secs(conn.timeout)).await?;
            println!("{}", serde_json::json!({ "reply": reply, "round_trip_ms": start.elapsed().as_millis() }));
        }
        Command::Spend { conn, proof, public } => {
            let fields = serde_json::from_slice::<serde_json::Value>(&std::fs::read(&public)?)
                .map_err(|e| topi_mixnet::Error::Protocol(e.to_string()))?["fields"]
                .clone();
            let proof = std::fs::read(&proof)?;
            let size = exit::encode_spend(&fields, &proof).len();
            eprintln!("sending a {} KB spend as {} packets", size / 1024, fragment_count(size));
            let mut client = Client::connect(Topology::load(&conn.topology)?, &conn.identity).await?;
            let reply = client.submit_spend(&fields, &proof, Duration::from_secs(conn.timeout)).await?;
            println!("{reply}");
            if reply["ok"] != true {
                std::process::exit(2);
            }
        }
    }
    Ok(())
}
