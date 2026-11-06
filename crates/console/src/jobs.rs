//! Commands the console runs in the background, with their output captured line by line.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::paths::Paths;

const MAX_LINES: usize = 4000;

pub struct Spec {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// A long-running process (a local node) rather than a task that finishes.
    pub service: bool,
}

impl Spec {
    pub fn new(name: impl Into<String>, program: impl Into<String>, args: &[&str], cwd: PathBuf) -> Spec {
        Spec {
            name: name.into(),
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd,
            env: vec![],
            service: false,
        }
    }
}

pub struct Job {
    pub name: String,
    pub service: bool,
    pub started: Instant,
    pub lines: Arc<Mutex<Vec<String>>>,
    ended: Arc<Mutex<Option<(Instant, i32)>>>,
    pid: u32,
    pub reported: bool,
}

impl Job {
    pub fn running(&self) -> bool {
        self.ended.lock().unwrap().is_none()
    }

    pub fn code(&self) -> Option<i32> {
        self.ended.lock().unwrap().map(|e| e.1)
    }

    pub fn elapsed(&self) -> Duration {
        match *self.ended.lock().unwrap() {
            Some((t, _)) => t - self.started,
            None => self.started.elapsed(),
        }
    }

    /// Stop the job and everything it started.
    pub fn kill(&self) {
        if !self.running() {
            return;
        }
        let pid = self.pid.to_string();
        let status = if cfg!(windows) {
            Command::new("taskkill").args(["/T", "/F", "/PID", &pid]).stdout(Stdio::null()).stderr(Stdio::null()).status()
        } else {
            Command::new("kill").arg(&pid).status()
        };
        let _ = status;
    }
}

pub fn spawn(paths: &Paths, spec: Spec) -> Job {
    let program = Path::new(&spec.program).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let lines = Arc::new(Mutex::new(vec![format!("$ {program} {}", spec.args.join(" "))]));
    let ended = Arc::new(Mutex::new(None));
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&paths.env)
        .env("PATH", &paths.path_env)
        .env("MSYS_NO_PATHCONV", "1")
        .env("TOPI_BIN_DIR", &paths.bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: child processes never draw over the console
        cmd.creation_flags(0x0800_0000);
    }
    let pid = match cmd.spawn() {
        Ok(mut child) => {
            pump(child.stdout.take().expect("piped stdout"), lines.clone());
            pump(child.stderr.take().expect("piped stderr"), lines.clone());
            let pid = child.id();
            let e = ended.clone();
            std::thread::spawn(move || {
                let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
                *e.lock().unwrap() = Some((Instant::now(), code));
            });
            pid
        }
        Err(err) => {
            lines.lock().unwrap().push(format!("could not start {program}: {err}"));
            *ended.lock().unwrap() = Some((Instant::now(), -1));
            0
        }
    };
    Job { name: spec.name, service: spec.service, started: Instant::now(), lines, ended, pid, reported: false }
}

fn pump(r: impl Read + Send + 'static, lines: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        let mut buf = Vec::new();
        while r.read_until(b'\n', &mut buf).map(|n| n > 0).unwrap_or(false) {
            let line = clean(&buf);
            if !line.trim().is_empty() {
                let mut v = lines.lock().unwrap();
                v.push(line);
                if v.len() > MAX_LINES {
                    v.drain(..MAX_LINES / 4);
                }
            }
            buf.clear();
        }
    });
}

/// Strip ANSI escape sequences and control characters.
fn clean(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{1b}' => {
                if it.peek() == Some(&'[') {
                    it.next();
                    for d in it.by_ref() {
                        if d.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

pub fn spawn_fn<F>(name: impl Into<String>, f: F) -> Job
where
    F: FnOnce(&dyn Fn(&str)) -> Result<(), String> + Send + 'static,
{
    let name_str = name.into();
    let lines = Arc::new(Mutex::new(vec![format!("$ [native] {name_str}")]));
    let ended = Arc::new(Mutex::new(None));
    let started = Instant::now();
    let lines_clone = lines.clone();
    let ended_clone = ended.clone();

    std::thread::spawn(move || {
        let logger = |msg: &str| {
            let mut l = lines_clone.lock().unwrap();
            l.push(msg.to_string());
            if l.len() > MAX_LINES {
                l.drain(..MAX_LINES / 4);
            }
        };
        let res = f(&logger);
        let mut end = ended_clone.lock().unwrap();
        match res {
            Ok(()) => *end = Some((Instant::now(), 0)),
            Err(e) => {
                let mut l = lines_clone.lock().unwrap();
                l.push(format!("error: {e}"));
                *end = Some((Instant::now(), 1));
            }
        }
    });

    Job { name: name_str, service: false, started, lines, ended, pid: std::process::id(), reported: false }
}
