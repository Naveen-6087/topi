//! Everything the console runs besides its own code, embedded in the binary: the prover scripts, the
//! alt mempool (sidecar setup, patch, rules and policy, with the TypeScript SDK it imports; the
//! sidecar's setup installs the SDK's npm dependencies), the AWS
//! scripts, the deployment records and the contracts' bytecode. Without a repository checkout the
//! console unpacks them into ~/.topi and works from there; data/ and .secrets/ live there too.

use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

static PROVER: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../prover");
static MEMPOOL_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../mempool/src");
static MEMPOOL_SIDECAR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../mempool/sidecar");
static SDK_SRC: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../sdk/src");
static AWS_NODE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../infra/aws/node");
static DEPLOYMENTS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../deployments");
static YUL: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../contracts/yul/build");

/// Single files: (path in the asset root, contents). Terraform state is deliberately absent.
const FILES: &[(&str, &[u8])] = &[
    ("mempool/RULES.md", include_bytes!("../../../mempool/RULES.md")),
    ("mempool/package.json", include_bytes!("../../../mempool/package.json")),
    ("sdk/package.json", include_bytes!("../../../sdk/package.json")),
    ("infra/aws/deploy.sh", include_bytes!("../../../infra/aws/deploy.sh")),
    ("infra/aws/prover.sh", include_bytes!("../../../infra/aws/prover.sh")),
    ("infra/aws/build-linux.sh", include_bytes!("../../../infra/aws/build-linux.sh")),
    ("infra/aws/main.tf", include_bytes!("../../../infra/aws/main.tf")),
    ("infra/aws/README.md", include_bytes!("../../../infra/aws/README.md")),
    ("contracts/test/fixtures/terminal/manifest.json", include_bytes!("../../../contracts/test/fixtures/terminal/manifest.json")),
    (".env.example", include_bytes!("../../../.env.example")),
    // Marks the asset root as a topi root for Paths (it looks for these three).
    ("Cargo.toml", b"# topi console assets (unpacked from the topi binary)\n"),
];

const DIRS: &[(&str, &Dir)] = &[
    ("prover", &PROVER),
    ("mempool/src", &MEMPOOL_SRC),
    ("mempool/sidecar", &MEMPOOL_SIDECAR),
    ("sdk/src", &SDK_SRC),
    ("infra/aws/node", &AWS_NODE),
    ("deployments", &DEPLOYMENTS),
    ("contracts/yul/build", &YUL),
];

/// The asset root: ~/.topi (TOPI_HOME overrides).
pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("TOPI_HOME") {
        return PathBuf::from(h);
    }
    let user = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    PathBuf::from(user).join(".topi")
}

/// Writes every embedded file under `root`, replacing older copies (a newer binary brings newer
/// scripts). Never touches data/ or .secrets/. Shell scripts get LF line endings for bash.
pub fn unpack(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root.join("crates"))?;
    std::fs::create_dir_all(root.join("contracts"))?;
    for (rel, bytes) in FILES {
        write(&root.join(rel), bytes)?;
    }
    for (rel, dir) in DIRS {
        write_dir(&root.join(rel), dir)?;
    }
    Ok(())
}

fn write_dir(dest: &Path, dir: &Dir) -> std::io::Result<()> {
    for file in dir.files() {
        write(&dest.join(file.path().strip_prefix(dir.path()).unwrap_or(file.path())), file.contents())?;
    }
    for sub in dir.dirs() {
        write_dir(&dest.join(sub.path().strip_prefix(dir.path()).unwrap_or(sub.path())), sub)?;
    }
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lf: Vec<u8>;
    let bytes = if path.extension().is_some_and(|e| e == "sh") {
        lf = bytes.iter().copied().filter(|&b| b != b'\r').collect();
        &lf
    } else {
        bytes
    };
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unpacks_a_working_root() {
        let root = std::env::temp_dir().join(format!("topi-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        super::unpack(&root).unwrap();
        for f in [
            "prover/prove-exit.sh",
            "mempool/src/policy.ts",
            "mempool/sidecar/sidecar.patch",
            "sdk/src/index.ts",
            "deployments/8141/pool.json",
            "contracts/yul/build/PQPaymaster.bin",
            "infra/aws/prover.sh",
        ] {
            assert!(root.join(f).exists(), "{f} missing");
        }
        assert!(!std::fs::read(root.join("prover/prove-exit.sh")).unwrap().contains(&b'\r'));
        let _ = std::fs::remove_dir_all(&root);
    }
}
