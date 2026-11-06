//! Where things are: the repository, the toolchain, local settings from `.env`, and data locations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
    /// Directory holding this binary and the other workspace binaries.
    pub bin: PathBuf,
    pub node: PathBuf,
    pub bash: PathBuf,
    /// Settings from `.env`, passed to every command the console runs.
    pub env: HashMap<String, String>,
    pub path_env: String,
}

impl Paths {
    pub fn discover() -> Paths {
        let root = find_root();
        let env = read_env(&root.join(".env"));
        let bin =
            std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_else(|| root.join("target/release"));
        let home = PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default());
        let local_node = root.join("tools/node24").join(format!("node{}", std::env::consts::EXE_SUFFIX));
        let node =
            env.get("NODE_BIN").map(PathBuf::from).unwrap_or_else(|| if local_node.exists() { local_node } else { PathBuf::from("node") });
        let git_bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        let bash = if cfg!(windows) && git_bash.exists() { git_bash } else { PathBuf::from("bash") };
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut path = vec![home.join(".foundry/bin"), home.join(".elan/bin"), home.join(".cargo/bin")];
        if let Some(dir) = node.parent().filter(|d| !d.as_os_str().is_empty()) {
            path.insert(0, dir.to_path_buf());
        }
        let path_env = path.iter().map(|p| p.display().to_string()).chain(std::env::var("PATH").ok()).collect::<Vec<_>>().join(sep);
        Paths { root, bin, node, bash, env, path_env }
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn s(&self, rel: &str) -> String {
        slash(&self.p(rel))
    }

    /// A setting from `.env` (or the environment) resolved against the repository, or the default.
    pub fn setting(&self, key: &str, default: &str) -> String {
        let v = std::env::var(key).ok().or_else(|| self.env.get(key).cloned()).unwrap_or_else(|| default.to_string());
        if Path::new(&v).is_absolute() || v.starts_with("http://") || v.starts_with("https://") || !v.contains('/') {
            v
        } else {
            self.s(&v)
        }
    }

    pub fn exe(&self, name: &str) -> String {
        slash(&self.bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
    }

    /// The path as WSL sees it (`C:/x` → `/mnt/c/x`).
    pub fn wsl(&self, rel: &str) -> String {
        let s = self.s(rel);
        match s.split_once(':') {
            Some((drive, rest)) if drive.len() == 1 => format!("/mnt/{}{}", drive.to_lowercase(), rest),
            _ => s,
        }
    }

    pub fn identity(&self) -> String {
        if self.p("data/local/member.id").exists() {
            return self.s("data/local/member.id");
        }
        self.setting("MEMBER_IDENTITY", ".secrets/member.id")
    }

    pub fn members(&self) -> String {
        self.setting("MEMBERS_FILE", "data/members.txt")
    }

    /// User's persistent Topi state directory (~/.topi)
    pub fn user_topi_dir(&self) -> PathBuf {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
        PathBuf::from(home).join(".topi")
    }

    /// Accounts directory: preferentially data/wallet/accounts if repo root is writable, or ~/.topi/accounts
    pub fn accounts_dir(&self) -> PathBuf {
        let local_acc = self.p("data/wallet/accounts");
        if local_acc.exists() || self.p("data/wallet").exists() || self.root.join("Cargo.toml").exists() {
            local_acc
        } else {
            self.user_topi_dir().join("accounts")
        }
    }

    /// Upstream prover home directory (~/.local/share/pq-prover)
    /// The pool deployment record (POOL_DEPLOYMENT, default deployments/8141/pool.json).
    pub fn pool_deployment(&self) -> PathBuf {
        self.p(&self.setting("POOL_DEPLOYMENT", "deployments/8141/pool.json"))
    }

    /// A path as the prover's Linux shell sees it (`C:/x` → `/mnt/c/x` on Windows).
    pub fn linux(&self, p: &Path) -> String {
        let s = slash(p);
        if !cfg!(windows) {
            return s;
        }
        match s.split_once(':') {
            Some((drive, rest)) if drive.len() == 1 => format!("/mnt/{}{}", drive.to_lowercase(), rest),
            _ => s,
        }
    }

    /// `script` in the prover's Linux shell (WSL on Windows, bash elsewhere) after sourcing prover/env.sh,
    /// which sets PROVER_HOME, WALLET_BIN and FIXTURE_BIN; `$1`.. are `args`, already Linux paths.
    pub fn prover_shell(&self, script: &str, args: &[String]) -> std::process::Command {
        let env_sh = self.linux(&self.root.join("prover/env.sh"));
        let body = format!(". \"$0\" && {script}");
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("wsl");
            c.args(["-d", &self.setting("WSL_DISTRO", "Ubuntu"), "--exec", "bash"]);
            c
        } else {
            std::process::Command::new("bash")
        };
        cmd.arg("-c").arg(body).arg(env_sh).args(args);
        cmd
    }

    /// Runs `script` in the prover shell; its stdout on success, stderr (or stdout) on failure.
    pub fn prover_run(&self, what: &str, script: &str, args: &[String]) -> Result<String, String> {
        let out = self.prover_shell(script, args).output().map_err(|e| format!("{what}: cannot start the prover shell: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if out.status.success() {
            return Ok(stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = if stderr.trim().is_empty() { stdout.trim().to_string() } else { stderr.trim().to_string() };
        Err(format!("{what} failed (run bash prover/setup.sh if the prover is not installed): {detail}"))
    }
}

pub fn slash(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

fn find_root() -> PathBuf {
    let is_root = |p: &Path| p.join("Cargo.toml").exists() && p.join("crates").is_dir() && p.join("contracts").is_dir();
    if let Ok(r) = std::env::var("TOPI_ROOT") {
        return PathBuf::from(r);
    }
    // TOPI_BUNDLED=1: run from the embedded files even next to a checkout.
    if std::env::var("TOPI_BUNDLED").as_deref() == Ok("1") {
        let home = crate::assets::home();
        if crate::assets::unpack(&home).is_ok() {
            return home;
        }
    }
    let start = std::env::current_dir().unwrap_or_default();
    if let Some(r) = start.ancestors().find(|p| is_root(p)) {
        return r.to_path_buf();
    }
    if let Some(r) = std::env::current_exe().ok().and_then(|e| e.ancestors().find(|p| is_root(p)).map(Path::to_path_buf)) {
        return r;
    }
    // Installed with `cargo install` (~/.cargo/bin/topi): the repository it was built from.
    // (No canonicalize: on Windows it returns a \?\ path, which WSL path conversion cannot read.)
    if let Some(built_from) = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent)
        && is_root(built_from)
    {
        return built_from.to_path_buf();
    }
    // No checkout: the files embedded in this binary, unpacked into ~/.topi.
    let home = crate::assets::home();
    if crate::assets::unpack(&home).is_ok() {
        return home;
    }
    start
}

/// `KEY=VALUE` lines; `#` comments and blank lines skipped; `$HOME` expanded.
fn read_env(path: &Path) -> HashMap<String, String> {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').replace("$HOME", &home)))
        .collect()
}
