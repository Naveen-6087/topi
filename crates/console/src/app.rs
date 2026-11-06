//! Console state: what is deployed, what is running, and what the files on disk say.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::mixnet_client::NodeStats;
use ratatui::style::Color;
use serde_json::Value;

use crate::commands::{self, LOCAL_ROLES, LOCAL_STATS_PORT, MEMPOOL_JOB};
use crate::jobs::{Job, Spec, spawn};
use crate::map::Flight;
use crate::paths::Paths;
use crate::poll::{Chain, Stats, Target};
use crate::theme::{AMBER, BAD, OK, PALE, RED};

pub const TABS: [&str; 3] = ["Wallet", "Mixnet Map", "Chain"];

/// Hourly price of the AWS deployment: four t3.micro instances (per-region prices), four public IPv4
/// addresses and four 8 GB gp3 volumes.
pub const AWS_HOURLY: f64 = 0.0112 + 0.0120 + 0.0104 + 0.0132 + 4.0 * 0.005 + 4.0 * 8.0 * 0.08 / 730.0;

/// Stages of an end-to-end run and the log text that marks each one done (logs/pipeline.log).
pub const STAGES: [(&str, &str); 11] = [
    ("nodes up", "4 nodes up"),
    ("provisioned", "provisioned: topology"),
    ("round trip", "round trip through"),
    ("deposits", "PQ account deposited note"),
    ("witnesses", "witnessed against the pool's current roots"),
    ("client proofs", "wallet proof ready"),
    ("through mixnet", "queued at the exit as"),
    ("queue fetched", "new spend(s)"),
    ("recursion", "terminal proof ready"),
    ("settled", "settled "),
    ("torn down", "nothing left running"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Local,
    Aws,
}

#[derive(Clone, Default)]
pub struct Note {
    pub name: String,
    pub amount: Option<u128>,
    pub leaf: Option<u64>,
    pub block: Option<u64>,
    pub witness: bool,
    pub proof: bool,
    pub queued: Option<String>,
    pub settled: bool,
}

#[derive(Clone, Default)]
pub struct Queue {
    pub pending: Vec<String>,
    pub done: usize,
    pub rejected: usize,
}

#[derive(Clone, Debug)]
pub enum InputPrompt {
    DepositAmount { note: String, buffer: String },
    WitnessRecipient { note: String, buffer: String },
}

#[derive(Clone)]
pub enum Action {
    Aws(&'static str),
    BuildLinux,
    LocalUp,
    LocalDown,
    Ping,
    CreateAccount,
    DeleteAccount,
    NewNote,
    DepositNote {
        note: String,
        amount_wei: u128,
    },
    WitnessNote {
        note: String,
        recipient: String,
    },
    NoteStep(&'static str),
    FetchQueue,
    Prove,
    /// Start or stop the alternative mempool for settles.
    Mempool,
    LeanBuild,
    LeanAudit,
    Differential(&'static str),
    /// The whole private payment in one go (crate::demo).
    Demo,
    /// Top up the depositing key and the paymaster from the other accounts (crate::funding).
    Fund,
}

pub struct App {
    pub paths: Paths,
    pub tab: usize,
    pub jobs: Vec<Job>,
    pub selected_job: usize,
    pub show_jobs: bool,
    pub follow: bool,
    pub scroll: usize,
    pub mode: Mode,
    pub targets: Arc<Mutex<Vec<Target>>>,
    pub stats: Stats,
    previous: HashMap<String, NodeStats>,
    pub rate: VecDeque<u64>,
    pub flights: Vec<Flight>,
    pub pulse: HashMap<String, u64>,
    pub zoom: bool,
    pub chain: Arc<Mutex<Chain>>,
    pub chain_cfg: Arc<Mutex<crate::poll::ChainConfig>>,
    settles_seen: Option<usize>,
    pub notes: Vec<Note>,
    pub selected_note: usize,
    pub local_queue: Queue,
    pub aws_queue: Queue,
    pub audit: Vec<[String; 4]>,
    pub audit_done: bool,
    pub identity: Option<String>,
    pub aws_started: Option<u64>,
    pub theorems: Vec<(String, String, String)>,
    pub pipeline: Vec<(u64, String, String)>,
    pub confirm: Option<(String, Action)>,
    pub prompt: Option<InputPrompt>,
    pub toast: Option<(Instant, String, Color)>,
    pub tick: u64,
    pub pool: Value,
    pub account: Value,
    pub accounts: Vec<crate::account::WalletAccount>,
    pub active_account_idx: usize,
    /// The alternative mempool's altmempool_status, while it runs.
    pub mempool: Option<Value>,
}

pub fn read_json(p: &Path) -> Value {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

pub fn now_secs() -> u64 {
    crate::mixnet_client::now_secs()
}

fn json_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn read_queue(dir: &Path) -> Queue {
    Queue {
        pending: json_files(dir).into_iter().map(|n| n.trim_end_matches(".json").to_string()).collect(),
        done: json_files(&dir.join("done")).len(),
        rejected: json_files(&dir.join("rejected")).len(),
    }
}

impl App {
    pub fn new(paths: Paths) -> App {
        let theorems = std::fs::read_to_string(paths.p("lean/README.md"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("| ") && l.contains('`'))
            .filter_map(|l| {
                let c: Vec<&str> = l.split('|').map(str::trim).collect();
                (c.len() >= 4).then(|| (c[1].to_string(), c[2].replace('`', ""), c[3].to_string()))
            })
            .collect();
        let pool = read_json(&paths.pool_deployment());
        let account = read_json(&paths.p("deployments/8141/account.json"));
        let accounts = crate::account::discover_accounts(&paths.accounts_dir());
        let rpc = paths.setting("RPC_URL", "https://rpc1.privacy.ethrex.xyz");
        let mut app = App {
            paths,
            tab: 0,
            jobs: vec![],
            selected_job: 0,
            show_jobs: false,
            follow: true,
            scroll: 0,
            mode: Mode::Off,
            targets: Arc::default(),
            stats: Arc::default(),
            previous: HashMap::new(),
            rate: VecDeque::from(vec![0; 120]),
            flights: vec![],
            pulse: HashMap::new(),
            zoom: false,
            chain: Arc::default(),
            chain_cfg: Arc::new(Mutex::new(crate::poll::ChainConfig {
                rpc,
                pool: pool["pool"].as_str().unwrap_or_default().to_string(),
                account: account["account"].as_str().unwrap_or_default().to_string(),
                paymaster: pool["paymaster"].as_str().unwrap_or_default().to_string(),
                from_block: 544_500,
            })),
            settles_seen: None,
            notes: vec![],
            selected_note: 0,
            local_queue: Queue::default(),
            aws_queue: Queue::default(),
            audit: vec![],
            audit_done: false,
            identity: None,
            aws_started: None,
            theorems,
            pipeline: vec![],
            confirm: None,
            prompt: None,
            toast: None,
            tick: 0,
            pool,
            account,
            accounts,
            active_account_idx: 0,
            mempool: None,
        };
        app.sync_active_account();
        app.refresh();
        app
    }

    pub fn sync_active_account(&mut self) {
        if self.accounts.is_empty() {
            self.account = serde_json::Value::Null;
            self.identity = None;
            if let Ok(mut cfg) = self.chain_cfg.lock() {
                cfg.account.clear();
            }
            if let Ok(mut chain) = self.chain.lock() {
                chain.account_wei = 0;
            }
            return;
        }
        if self.active_account_idx >= self.accounts.len() {
            self.active_account_idx = 0;
        }
        let active = &self.accounts[self.active_account_idx];
        self.account = serde_json::json!({
            "name": active.meta.name,
            "account": active.meta.account,
            "commitment": active.meta.commitment,
        });
        self.identity = Some(active.identity_file.display().to_string());
        if let Ok(mut cfg) = self.chain_cfg.lock() {
            cfg.account = active.meta.account.clone();
        }
    }

    pub fn notify(&mut self, s: impl Into<String>, c: Color) {
        self.toast = Some((Instant::now(), s.into(), c));
    }

    pub fn theorem_count(&self) -> usize {
        self.theorems.iter().map(|t| t.1.split(',').count()).sum()
    }

    /// A local mixnet node is running (the mempool is a service too, but not a node).
    pub fn local_running(&self) -> bool {
        if self.jobs.iter().any(|j| j.service && j.name.starts_with("local ") && j.running()) {
            return true;
        }
        // Also a mixnet started outside the console (run_local_demo.sh, a script): its stats ports answer.
        ["127.0.0.1:47300", "127.0.0.1:47301"]
            .iter()
            .any(|a| std::net::TcpStream::connect_timeout(&a.parse().unwrap(), std::time::Duration::from_millis(50)).is_ok())
    }

    pub fn mempool_running(&self) -> bool {
        self.jobs.iter().any(|j| j.name == MEMPOOL_JOB && j.running())
    }

    pub fn aws_deployed(&self) -> bool {
        self.paths.p("data/aws/nodes.txt").exists()
    }

    pub fn topology(&self) -> Option<String> {
        let local_topo = self.paths.s("data/local/topology.json");
        if std::path::Path::new(&local_topo).exists() {
            return Some(local_topo);
        }
        match self.mode {
            Mode::Aws => Some(self.paths.s("data/aws/topology.json")),
            Mode::Local => Some(local_topo),
            Mode::Off => None,
        }
    }

    pub fn queue_dir(&self) -> String {
        if self.mode == Mode::Aws { self.paths.s("data/aws/spend-queue") } else { self.paths.s("data/local/spend-queue") }
    }

    /// Re-read everything that lives in files: deployed nodes, wallet notes, queues, the pipeline log.
    pub fn refresh(&mut self) {
        self.mempool = if self.mempool_running() { crate::poll::mempool_status(&commands::mempool_url()) } else { None };
        let mut targets = vec![];
        if self.aws_deployed() && self.mode != Mode::Local {
            for l in std::fs::read_to_string(self.paths.p("data/aws/nodes.txt")).unwrap_or_default().lines() {
                if let [role, host, region, instance] = l.split_whitespace().collect::<Vec<_>>()[..] {
                    targets.push(Target {
                        role: role.into(),
                        host: host.into(),
                        stats_port: 47300,
                        region: region.into(),
                        instance: instance.into(),
                    });
                }
            }
            self.mode = Mode::Aws;
            self.aws_started = std::fs::read_to_string(self.paths.p("data/aws/started")).ok().and_then(|s| s.trim().parse().ok());
        } else if self.local_running() {
            for (i, role) in LOCAL_ROLES.iter().enumerate() {
                targets.push(Target {
                    role: role.to_string(),
                    host: "127.0.0.1".into(),
                    stats_port: LOCAL_STATS_PORT + i as u16,
                    region: String::new(),
                    instance: "local".into(),
                });
            }
            self.mode = Mode::Local;
        } else {
            self.mode = Mode::Off;
        }
        if !self.aws_deployed() {
            self.aws_started = None;
        }
        *self.targets.lock().unwrap() = targets;

        // Refresh discovered accounts
        self.accounts = crate::account::discover_accounts(&self.paths.accounts_dir());
        self.sync_active_account();

        if let Some(acc) = self.accounts.get(self.active_account_idx) {
            let dir = acc.notes_dir.clone();
            self.notes = json_files(&dir)
                .into_iter()
                .filter(|f| f.starts_with("note-"))
                .map(|f| {
                    let name = f[5..f.len() - 5].to_string();
                    let n = read_json(&dir.join(&f));
                    let queued = read_json(&dir.join(format!("sent-{name}.json")))["queued"].as_str().map(String::from);
                    let settled = queued.as_ref().is_some_and(|q| {
                        self.paths.p("data/local/spend-queue/done").join(format!("{q}.json")).exists()
                            || self.paths.p("data/aws/spend-queue/done").join(format!("{q}.json")).exists()
                    });
                    Note {
                        amount: n["amount"].as_str().and_then(|s| s.parse().ok()),
                        leaf: n["leaf_index"].as_u64(),
                        block: n["deposit_block"].as_u64(),
                        witness: dir.join(format!("input-{name}.json")).exists(),
                        proof: dir.join(format!("send/{name}.tgz")).exists(),
                        queued,
                        settled,
                        name,
                    }
                })
                .collect();
        } else {
            self.notes.clear();
        }
        self.selected_note = self.selected_note.min(self.notes.len().saturating_sub(1));
        self.local_queue = read_queue(&self.paths.p("data/local/spend-queue"));
        self.aws_queue = read_queue(&self.paths.p("data/aws/spend-queue"));
        self.pipeline = std::fs::read_to_string(self.paths.p("logs/pipeline.log"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut it = l.splitn(3, ' ');
                Some((it.next()?.parse().ok()?, it.next()?.to_string(), it.next().unwrap_or("").to_string()))
            })
            .collect();
    }

    pub fn run_job(&mut self, job: Job) {
        self.notify(format!("started: {}", job.name), PALE);
        self.jobs.push(job);
        self.selected_job = self.jobs.len() - 1;
        self.show_jobs = true;
        self.follow = true;
    }

    fn run(&mut self, spec: Spec) {
        let job = spawn(&self.paths, spec);
        self.run_job(job);
    }

    pub fn act(&mut self, action: Action) {
        let p = &self.paths;
        let spec = match action {
            Action::Aws(sub) => commands::aws(p, sub),
            Action::BuildLinux => commands::build_linux(p),
            Action::LocalUp => return self.local_up(),
            Action::Fund => {
                let p = self.paths.clone();
                let job = crate::jobs::spawn_fn("fund accounts", move |log| crate::funding::ensure_funded(&p, log));
                self.show_jobs = true;
                return self.run_job(job);
            }
            Action::Demo => {
                if self.accounts.is_empty() {
                    self.act(Action::CreateAccount);
                }
                if !self.local_running() {
                    self.local_up();
                }
                let Some(acc) = self.accounts.get(self.active_account_idx) else {
                    return self.notify("could not create an account for the demo", BAD);
                };
                let (notes_dir, account_dir, recipient) = (acc.notes_dir.clone(), acc.dir.clone(), acc.meta.account.clone());
                let topology = self.paths.s("data/local/topology.json");
                let queue = self.paths.s("data/local/spend-queue");
                let mempool = self.mempool_running().then(commands::mempool_url);
                let p = self.paths.clone();
                let job = crate::jobs::spawn_fn("full demo", move |log| {
                    let d = crate::demo::Demo {
                        notes_dir: &notes_dir,
                        account_dir: &account_dir,
                        recipient: &recipient,
                        topology: &topology,
                        queue: &queue,
                        mempool: mempool.as_deref(),
                    };
                    crate::demo::run(&p, &d, log)
                });
                self.show_jobs = true;
                return self.run_job(job);
            }
            Action::LocalDown => {
                self.jobs.iter().filter(|j| j.service && j.name.starts_with("local ")).for_each(Job::kill);
                return self.notify("local network stopped", PALE);
            }
            Action::Ping => match self.topology() {
                Some(t) => commands::ping(p, &t),
                None => return self.notify("no network: start one here (Network: l) or on AWS (Cloud: u, p)", BAD),
            },
            Action::CreateAccount => {
                let count = self.accounts.len() + 1;
                let name = format!("account-{}", count);
                let base = self.paths.accounts_dir();
                let members_file = self.paths.p(&self.paths.members());
                match crate::account::WalletAccount::create(&base, &name, Some(&members_file)) {
                    Ok(acc) => {
                        let short_addr = acc.meta.account.clone();
                        self.accounts.push(acc);
                        self.active_account_idx = self.accounts.len() - 1;
                        self.sync_active_account();
                        self.refresh();
                        return self.notify(format!("created PQ account: {name} ({short_addr})"), OK);
                    }
                    Err(e) => return self.notify(format!("failed to create account: {e}"), BAD),
                }
            }
            Action::DeleteAccount => {
                if self.accounts.is_empty() {
                    return self.notify("no account to delete", BAD);
                }
                let victim = self.accounts.remove(self.active_account_idx);
                let _ = std::fs::remove_dir_all(&victim.dir);
                if self.active_account_idx >= self.accounts.len() && !self.accounts.is_empty() {
                    self.active_account_idx = self.accounts.len() - 1;
                }
                self.sync_active_account();
                self.refresh();
                return self.notify(format!("deleted account: {}", victim.meta.name), AMBER);
            }
            Action::NewNote => {
                let used: Vec<&str> = self.notes.iter().map(|n| n.name.as_str()).collect();
                let name =
                    (b'a'..=b'z').map(|c| (c as char).to_string()).find(|n| !used.contains(&n.as_str())).unwrap_or_else(|| "z".into());
                let notes_dir = self
                    .accounts
                    .get(self.active_account_idx)
                    .map(|a| a.notes_dir.clone())
                    .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                let p = self.paths.clone();
                let job =
                    crate::jobs::spawn_fn(format!("new note {name}"), move |log| crate::wallet_ops::new_note(&p, &name, &notes_dir, log));
                return self.run_job(job);
            }
            Action::DepositNote { note, amount_wei } => {
                let notes_dir = self
                    .accounts
                    .get(self.active_account_idx)
                    .map(|a| a.notes_dir.clone())
                    .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                let account_dir = self.accounts.get(self.active_account_idx).map(|a| a.dir.clone());
                let p = self.paths.clone();
                let job = crate::jobs::spawn_fn(format!("deposit {note}"), move |log| {
                    crate::wallet_ops::deposit(&p, &note, amount_wei, &notes_dir, account_dir.as_deref(), log)
                });
                return self.run_job(job);
            }
            Action::WitnessNote { note, recipient } => {
                let notes_dir = self
                    .accounts
                    .get(self.active_account_idx)
                    .map(|a| a.notes_dir.clone())
                    .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                let p = self.paths.clone();
                let job = crate::jobs::spawn_fn(format!("witness {note}"), move |log| {
                    crate::wallet_ops::witness(&p, &note, &recipient, &notes_dir, log)
                });
                return self.run_job(job);
            }
            Action::NoteStep(step) => {
                let Some(note) = self.notes.get(self.selected_note).map(|n| n.name.clone()) else {
                    return self.notify("no note selected: press n for a new one", BAD);
                };
                if step == "prove" {
                    let notes_dir = self
                        .accounts
                        .get(self.active_account_idx)
                        .map(|a| a.notes_dir.clone())
                        .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                    let p = self.paths.clone();
                    let job = crate::jobs::spawn_fn(format!("prove {note}"), move |log| {
                        crate::wallet_ops::prove_note(&p, &note, &notes_dir, log)
                    });
                    return self.run_job(job);
                } else if step == "send" {
                    let topology = self.topology();
                    if topology.is_none() {
                        return self
                            .notify("no network available to send through: start the local mixnet with 'l' on the Network tab", BAD);
                    }
                    let notes_dir = self
                        .accounts
                        .get(self.active_account_idx)
                        .map(|a| a.notes_dir.clone())
                        .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                    commands::send(p, &note, topology.as_deref(), Some(&notes_dir))
                } else {
                    return self.notify(format!("unknown step: {step}"), BAD);
                }
            }
            Action::FetchQueue => commands::aws(p, "fetch-queue"),
            Action::Prove => {
                let queue = self.queue_dir();
                let p = self.paths.clone();
                let mempool = self.mempool_running().then(commands::mempool_url);
                let job = crate::jobs::spawn_fn("exit prover", move |log| {
                    crate::prover_ops::run_exit_prover(&p, &queue, mempool.as_deref(), log)
                });
                return self.run_job(job);
            }
            Action::Mempool => {
                if self.mempool_running() {
                    self.jobs.iter().filter(|j| j.name.starts_with(MEMPOOL_JOB)).for_each(Job::kill);
                    self.mempool = None;
                    return self.notify("alt mempool stopped; settles go straight to the chain", PALE);
                }
                // Two nodes of the same mempool, gossiping: the second joins the first once it is up.
                let peer = crate::jobs::spawn(&self.paths, commands::mempool_peer(p));
                self.jobs.push(peer);
                commands::mempool(p)
            }
            Action::LeanBuild => commands::lean_build(p),
            Action::LeanAudit => commands::lean_audit(p),
            Action::Differential(which) => commands::differential(p, which),
        };
        self.run(spec);
    }

    /// Four nodes on this machine, each with its own key and stats port.
    fn local_up(&mut self) {
        if self.local_running() {
            return self.notify("the local network is already running", PALE);
        }
        let dir = self.paths.p("data/local");
        let _ = std::fs::create_dir_all(dir.join("keys"));
        let node = self.paths.exe("topi-node");
        for role in LOCAL_ROLES {
            let key = dir.join(format!("keys/{role}.key"));
            if !key.exists() {
                let _ = Command::new(&node).args(["keygen", &crate::paths::slash(&key)]).output();
            }
        }
        let topo = crate::paths::slash(&dir.join("topology.json"));
        let keys = crate::paths::slash(&dir.join("keys"));
        let members = self.paths.members();
        let made = Command::new(&node)
            .args([
                "topology",
                &topo,
                "--members",
                &members,
                "--keys",
                &keys,
                "--hosts",
                "127.0.0.1",
                "127.0.0.1",
                "127.0.0.1",
                "127.0.0.1",
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !made {
            return self.notify("could not write the local topology (is the member list present?)", BAD);
        }
        for (i, role) in LOCAL_ROLES.iter().enumerate() {
            let job = spawn(&self.paths, commands::local_node(&self.paths, role, i));
            self.jobs.push(job);
        }
        self.mode = Mode::Local;
        self.refresh();
        self.notify("local network up: four nodes on this machine", OK);
    }

    /// Report finished jobs and pick up what they changed.
    pub fn on_finished(&mut self) {
        let mut changed = false;
        let mut done = vec![];
        for j in self.jobs.iter_mut().filter(|j| !j.running() && !j.reported) {
            j.reported = true;
            changed = true;
            let lines = j.lines.lock().unwrap().clone();
            if j.name == "aws audit" {
                self.audit = lines
                    .iter()
                    .filter(|l| l.starts_with("FOUND"))
                    .map(|l| {
                        let f: Vec<&str> = l.splitn(5, ' ').collect();
                        let g = |i: usize| f.get(i).unwrap_or(&"").to_string();
                        [g(1), g(2), g(3), g(4)]
                    })
                    .collect();
                self.audit_done = true;
            }
            if j.name == "aws whoami" {
                self.identity = lines.iter().find(|l| l.contains("\"Arn\"")).and_then(|l| l.split('"').nth(3)).map(String::from);
            }
            if let Some(note) = j.name.strip_prefix("send ") {
                for l in lines.iter().rev() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(l)
                        && let Some(queued) = v.get("queued").and_then(|q| q.as_str())
                    {
                        let notes_dir = self
                            .accounts
                            .get(self.active_account_idx)
                            .map(|a| a.notes_dir.clone())
                            .unwrap_or_else(|| self.paths.p("data/wallet/demo"));
                        let sent_file = notes_dir.join(format!("sent-{note}.json"));
                        let _ = std::fs::write(&sent_file, v.to_string());
                        crate::wallet_ops::pipeline_log(&self.paths, "wallet", &format!("spend {note}: queued at the exit as {queued}"));
                        break;
                    }
                }
            }
            done.push((j.name.clone(), j.code() == Some(0), j.elapsed()));
        }
        if let Some((name, ok, took)) = done.pop() {
            let text = format!("{name} {} in {}", if ok { "finished" } else { "failed" }, crate::theme::duration(took));
            self.notify(text, if ok { OK } else { BAD });
        }
        if changed {
            self.refresh();
        }
    }

    /// Turn new node counters into packets in flight, and new chain settles into a burst to the chain.
    pub fn on_stats(&mut self) {
        let now: HashMap<String, NodeStats> =
            self.stats.lock().unwrap().iter().filter_map(|(k, v)| v.clone().map(|s| (k.clone(), s))).collect();
        let mut total = 0;
        for (role, s) in &now {
            // a node seen for the first time (or restarted) sets the baseline, not a burst
            let Some(p) = self.previous.get(role).filter(|p| s.received >= p.received) else { continue };
            let d = s.received - p.received;
            total += d;
            if d > 0 {
                self.pulse.insert(role.clone(), self.tick);
            }
            let n = d.min(6) as usize;
            match role.as_str() {
                "gateway" => (0..n).for_each(|k| self.flights.push(Flight::new(&["you", "gateway"], k as f64 * 0.18, 0.045, RED))),
                "gw-1" => (0..n).for_each(|k| self.flights.push(Flight::new(&["you", "gw-1"], k as f64 * 0.18, 0.045, RED))),
                "gw-2" => (0..n).for_each(|k| self.flights.push(Flight::new(&["you", "gw-2"], k as f64 * 0.18, 0.045, RED))),
                "mix" => (0..n).for_each(|k| self.flights.push(Flight::new(&["gateway", "mix"], k as f64 * 0.2, 0.03, RED))),
                "mix-1" => (0..n).for_each(|k| {
                    let from = if k % 2 == 0 { "gw-1" } else { "gw-2" };
                    self.flights.push(Flight::new(&[from, "mix-1"], k as f64 * 0.2, 0.03, RED));
                }),
                "mix-2" => (0..n).for_each(|k| {
                    let from = if k % 2 == 0 { "gw-1" } else { "gw-2" };
                    self.flights.push(Flight::new(&[from, "mix-2"], k as f64 * 0.2, 0.03, RED));
                }),
                "exit" => {
                    let burst = if s.batches > p.batches { s.last_batch.clamp(1, 8) } else { n };
                    (0..burst.max(n)).for_each(|k| {
                        self.flights.push(Flight::new(&["mix", "exit"], k as f64 * 0.025, 0.026, RED));
                        // also animate reply path so relay is always lit
                        if k < 2 {
                            self.flights.push(Flight::new(&["exit", "reply-relay", "mix", "gateway", "you"], k as f64 * 0.4, 0.032, PALE));
                        }
                    });
                }
                "exit-1" => {
                    let burst = if s.batches > p.batches { s.last_batch.clamp(1, 8) } else { n };
                    (0..burst.max(n)).for_each(|k| {
                        let from = if k % 2 == 0 { "mix-1" } else { "mix-2" };
                        self.flights.push(Flight::new(&[from, "exit-1"], k as f64 * 0.025, 0.026, RED));
                        // animate reply path: exit-1 → relay-1 → mix-1 → gw-1 → you
                        if k < 2 {
                            self.flights.push(Flight::new(&["exit-1", "relay-1", "mix-1", "gw-1", "you"], k as f64 * 0.4, 0.032, PALE));
                        }
                    });
                }
                "exit-2" => {
                    let burst = if s.batches > p.batches { s.last_batch.clamp(1, 8) } else { n };
                    (0..burst.max(n)).for_each(|k| {
                        let from = if k % 2 == 0 { "mix-1" } else { "mix-2" };
                        self.flights.push(Flight::new(&[from, "exit-2"], k as f64 * 0.025, 0.026, RED));
                        // animate reply path: exit-2 → relay-2 → mix-2 → gw-2 → you
                        if k < 2 {
                            self.flights.push(Flight::new(&["exit-2", "relay-2", "mix-2", "gw-2", "you"], k as f64 * 0.4, 0.032, PALE));
                        }
                    });
                }
                "reply-relay" => (0..n).for_each(|k| {
                    self.flights.push(Flight::new(&["exit", "reply-relay", "mix", "gateway", "you"], k as f64 * 0.3, 0.035, PALE))
                }),
                "relay-1" => (0..n).for_each(|k| {
                    let exit = if k % 2 == 0 { "exit-1" } else { "exit-2" };
                    let mix = if k % 2 == 0 { "mix-1" } else { "mix-2" };
                    let gw = if k % 2 == 0 { "gw-1" } else { "gw-2" };
                    self.flights.push(Flight::new(&[exit, "relay-1", mix, gw, "you"], k as f64 * 0.3, 0.035, PALE))
                }),
                "relay-2" => (0..n).for_each(|k| {
                    let exit = if k % 2 == 0 { "exit-1" } else { "exit-2" };
                    let mix = if k % 2 == 0 { "mix-1" } else { "mix-2" };
                    let gw = if k % 2 == 0 { "gw-1" } else { "gw-2" };
                    self.flights.push(Flight::new(&[exit, "relay-2", mix, gw, "you"], k as f64 * 0.3, 0.035, PALE))
                }),
                _ => {}
            }
            if (role == "mix" || role.starts_with("mix")) && s.batches > p.batches {
                self.notify(format!("{role} released a batch of {} shuffled packets", s.last_batch), AMBER);
            }
        }
        let settles = self.chain.lock().unwrap().events.iter().filter(|e| e.kind == "settle").count();
        let block = self.chain.lock().unwrap().block;
        if self.settles_seen.is_some_and(|prev| settles > prev) {
            // From the exit(s) that actually run: the four-node network has one.
            let stratified = now.contains_key("exit-1");
            for k in 0..4 {
                let from = if !stratified {
                    "exit"
                } else if k % 2 == 0 {
                    "exit-1"
                } else {
                    "exit-2"
                };
                let mut f = Flight::new(&[from, "chain"], k as f64 * 0.15, 0.025, AMBER);
                f.heavy = true;
                self.flights.push(f);
            }
            let chain_id = self.pool["chainId"].as_u64().unwrap_or(8141);
            self.notify(format!("settle landed on chain {chain_id} near block {block}"), OK);
        }
        if block > 0 {
            self.settles_seen = Some(settles);
        }
        self.rate.push_back(total);
        if self.rate.len() > 120 {
            self.rate.pop_front();
        }
        if self.flights.len() > 220 {
            let extra = self.flights.len() - 220;
            self.flights.drain(..extra);
        }
        self.previous = now;
    }

    pub fn advance(&mut self) {
        self.tick += 1;
        for f in &mut self.flights {
            f.t += f.speed;
        }
        self.flights.retain(|f| !f.done());
    }

    pub fn shutdown(&self) {
        self.jobs.iter().filter(|j| j.service).for_each(Job::kill);
    }
}
