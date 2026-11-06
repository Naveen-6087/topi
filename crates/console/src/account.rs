//! Post-Quantum account and mixnet identity management for the wallet.

use crate::mixnet_client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountMeta {
    pub name: String,
    pub account: String,
    pub commitment: String,
    pub created_at: u64,
}

#[derive(Clone, Debug)]
pub struct WalletAccount {
    pub meta: AccountMeta,
    pub dir: PathBuf,
    pub identity_file: PathBuf,
    pub notes_dir: PathBuf,
}

impl WalletAccount {
    /// Load account from directory.
    pub fn load_from_dir(dir: &Path) -> Option<WalletAccount> {
        let meta_file = dir.join("account.json");
        let text = fs::read_to_string(&meta_file).ok()?;
        let meta: AccountMeta = serde_json::from_str(&text).ok()?;
        let identity_file = dir.join("member.id");
        let notes_dir = dir.join("notes");
        let _ = fs::create_dir_all(&notes_dir);
        Some(WalletAccount { meta, dir: dir.to_path_buf(), identity_file, notes_dir })
    }

    /// Create a new account with a fresh rate-limiting identity and post-quantum keys.
    pub fn create(base_dir: &Path, name: &str, members_file: Option<&Path>) -> Result<WalletAccount, String> {
        let account_dir = base_dir.join(name);
        if account_dir.exists() && account_dir.join("account.json").exists() {
            return Err(format!("Account '{name}' already exists"));
        }
        fs::create_dir_all(&account_dir).map_err(|e| e.to_string())?;

        let identity_file = account_dir.join("member.id");
        let commitment = mixnet_client::new_identity(&identity_file).map_err(|e| e.to_string())?;
        let commitment_hex = hex::encode(commitment);

        // If a members file is specified, append the commitment
        if let Some(m_path) = members_file {
            if let Some(parent) = m_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut existing = fs::read_to_string(m_path).unwrap_or_default();
            if !existing.is_empty() && !existing.ends_with('\n') {
                existing.push('\n');
            }
            existing.push_str(&commitment_hex);
            existing.push('\n');
            let _ = fs::write(m_path, existing);
        }

        // Generate real ECDSA Ethereum account key and derive its address in pure Rust
        let key_file = account_dir.join("account.key");
        let priv_key = if key_file.exists() {
            let s = fs::read_to_string(&key_file).map_err(|e| e.to_string())?;
            crate::crypto::private_key_from_hex(&s).map_err(|e| e.to_string())?
        } else {
            let key = crate::crypto::random_private_key();
            fs::write(&key_file, format!("{}\n", hex::encode(key))).map_err(|e| e.to_string())?;
            key
        };
        let addr_bytes = crate::crypto::address_from_key(&priv_key).map_err(|e| e.to_string())?;
        let short_addr = crate::crypto::checksum_address(&addr_bytes);
        let now = mixnet_client::now_secs();

        let meta = AccountMeta { name: name.to_string(), account: short_addr, commitment: commitment_hex, created_at: now };

        let json_text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        fs::write(account_dir.join("account.json"), json_text).map_err(|e| e.to_string())?;

        let notes_dir = account_dir.join("notes");
        fs::create_dir_all(&notes_dir).map_err(|e| e.to_string())?;

        Ok(WalletAccount { meta, dir: account_dir, identity_file, notes_dir })
    }
}

/// Discover all accounts in the given base accounts directory.
pub fn discover_accounts(base_dir: &Path) -> Vec<WalletAccount> {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return vec![];
    };
    let mut accounts: Vec<WalletAccount> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() { WalletAccount::load_from_dir(&path) } else { None }
        })
        .collect();
    accounts.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    accounts
}
