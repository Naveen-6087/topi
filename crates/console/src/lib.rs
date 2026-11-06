//! topi: a terminal control room for the whole system. Every action runs a real command and streams its
//! output into the jobs pane.
//!
//! Usage: `topi [overview|network|cloud|wallet|prover|proofs|chain]` opens on that tab.

pub mod account;
pub mod app;
pub mod assets;
pub mod commands;
pub mod crypto;
pub mod demo;
pub mod funding;
pub mod jobs;
pub mod links;
pub mod map;
pub mod mixnet_client;
pub mod paths;
pub mod poll;
pub mod prover_ops;
pub mod theme;
pub mod ui;
pub mod wallet_ops;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::{Action, App, TABS};

pub fn run(initial_tab: Option<usize>) {
    let mut app = App::new(paths::Paths::discover());
    if let Some(t) = initial_tab {
        app.tab = t;
    }
    if let Some(arg) = std::env::args().nth(1) {
        let arg = arg.to_lowercase();
        let arg = if arg == "network" { "mixnet".to_string() } else { arg };
        app.tab = TABS
            .iter()
            .position(|t| t.to_lowercase().split_whitespace().any(|w| w == arg) || t.eq_ignore_ascii_case(&arg))
            .unwrap_or(app.tab);
    }
    poll::watch_nodes(app.targets.clone(), app.stats.clone());
    poll::watch_chain(app.chain_cfg.clone(), app.chain.clone());

    let mut terminal = ratatui::init();
    let (mut last_stats, mut last_refresh) = (Instant::now(), Instant::now());
    loop {
        app.advance();
        if last_stats.elapsed() >= Duration::from_secs(1) {
            last_stats = Instant::now();
            app.on_stats();
        }
        if last_refresh.elapsed() >= Duration::from_secs(2) {
            last_refresh = Instant::now();
            app.refresh();
        }
        app.on_finished();
        let _ = terminal.draw(|f| ui::draw(f, &mut app));
        if !event::poll(Duration::from_millis(60)).unwrap_or(false) {
            continue;
        }
        let Ok(Event::Key(key)) = event::read() else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if !handle_key(&mut app, key.code, key.modifiers) {
            break;
        }
    }
    ratatui::restore();
    app.shutdown();
    let running: Vec<&str> = app.jobs.iter().filter(|j| j.running() && !j.service).map(|j| j.name.as_str()).collect();
    if !running.is_empty() {
        println!("still running in the background: {}", running.join(", "));
    }
    if app.aws_deployed() {
        println!("the AWS deployment is still up (about ${:.3} per hour); stop it with: bash infra/aws/deploy.sh down", app::AWS_HOURLY);
    }
}

/// Returns false to quit.
fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) -> bool {
    // 1. Handle active text input prompt (modal)
    if let Some(prompt) = app.prompt.take() {
        match (code, prompt) {
            (KeyCode::Esc, _) => {
                app.notify("cancelled", theme::DIM);
            }
            (KeyCode::Enter, crate::app::InputPrompt::DepositAmount { note, buffer }) => {
                let trimmed = buffer.trim();
                match trimmed.parse::<f64>() {
                    Ok(eth_val) if eth_val > 0.0 => {
                        let amount_wei = (eth_val * 1e18) as u128;
                        app.act(Action::DepositNote { note, amount_wei });
                    }
                    _ => {
                        app.notify(format!("invalid ETH amount: '{trimmed}'"), theme::BAD);
                    }
                }
            }
            (KeyCode::Enter, crate::app::InputPrompt::WitnessRecipient { note, buffer }) => {
                let trimmed = buffer.trim();
                if trimmed.starts_with("0x") && trimmed.len() == 42 && hex::decode(&trimmed[2..]).is_ok() {
                    app.act(Action::WitnessNote { note, recipient: trimmed.to_string() });
                } else {
                    app.notify("invalid recipient address (must be 0x followed by 40 hex digits)", theme::BAD);
                }
            }
            (KeyCode::Backspace, crate::app::InputPrompt::DepositAmount { note, mut buffer }) => {
                buffer.pop();
                app.prompt = Some(crate::app::InputPrompt::DepositAmount { note, buffer });
            }
            (KeyCode::Backspace, crate::app::InputPrompt::WitnessRecipient { note, mut buffer }) => {
                buffer.pop();
                app.prompt = Some(crate::app::InputPrompt::WitnessRecipient { note, buffer });
            }
            (KeyCode::Char(c), crate::app::InputPrompt::DepositAmount { note, mut buffer }) => {
                if c.is_ascii_digit() || c == '.' {
                    buffer.push(c);
                }
                app.prompt = Some(crate::app::InputPrompt::DepositAmount { note, buffer });
            }
            (KeyCode::Char(c), crate::app::InputPrompt::WitnessRecipient { note, mut buffer }) => {
                if c.is_ascii_hexdigit() || c == 'x' || c == 'X' {
                    buffer.push(c);
                }
                app.prompt = Some(crate::app::InputPrompt::WitnessRecipient { note, buffer });
            }
            (_, other) => {
                app.prompt = Some(other);
            }
        }
        return true;
    }

    // 2. Handle confirmation dialog
    if let Some((_, action)) = app.confirm.take() {
        if matches!(code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            app.act(action);
        } else {
            app.notify("cancelled", theme::DIM);
        }
        return true;
    }

    // 3. Main keys
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => return false,
        KeyCode::Tab | KeyCode::Right => app.tab = (app.tab + 1) % TABS.len(),
        KeyCode::BackTab | KeyCode::Left => app.tab = (app.tab + TABS.len() - 1) % TABS.len(),
        KeyCode::Char(c @ '1'..='3') => app.tab = c as usize - '1' as usize,
        KeyCode::Char('j') => app.show_jobs = !app.show_jobs,
        KeyCode::Char(c @ ('o' | 'g')) => {
            // o: the explorer on the Chain tab, the project site elsewhere; g: the repository.
            let chain_id = app.pool["chainId"].as_u64().unwrap_or(8141);
            let url = match (c, app.tab) {
                ('o', 2) => links::explorer_for(chain_id),
                ('o', _) => links::SITE.to_string(),
                _ => links::REPO.to_string(),
            };
            let url = url.as_str();
            match links::open(url) {
                Ok(()) => app.notify(format!("opened {}", links::short(url)), theme::MUTED),
                Err(e) => app.notify(format!("could not open a browser ({e}): {url}"), theme::BAD),
            }
        }
        KeyCode::Char('[') => {
            app.selected_job = app.selected_job.saturating_sub(1);
            app.follow = true;
        }
        KeyCode::Char(']') => {
            app.selected_job = (app.selected_job + 1).min(app.jobs.len().saturating_sub(1));
            app.follow = true;
        }
        KeyCode::PageUp => {
            app.follow = false;
            app.scroll = app.scroll.saturating_sub(8);
        }
        KeyCode::PageDown => {
            app.follow = false;
            app.scroll += 8;
        }
        KeyCode::End => app.follow = true,
        KeyCode::Char('x') => {
            if let Some(j) = app.jobs.get(app.selected_job) {
                j.kill();
            }
        }
        KeyCode::Up if app.tab == 0 => app.selected_note = app.selected_note.saturating_sub(1),
        KeyCode::Down if app.tab == 0 => app.selected_note = (app.selected_note + 1).min(app.notes.len().saturating_sub(1)),
        KeyCode::Char(c) => tab_key(app, c),
        _ => {}
    }
    true
}

fn tab_key(app: &mut App, c: char) {
    let action = match (app.tab, c) {
        // Tab 0: Wallet
        (0, 'c') => Some(Action::CreateAccount),
        (0, 'X') => {
            if let Some(acc) = app.accounts.get(app.active_account_idx) {
                app.confirm = Some((format!("Permanently delete account '{}' and its secrets?", acc.meta.name), Action::DeleteAccount));
            } else {
                app.notify("no account to delete", theme::BAD);
            }
            None
        }
        (0, 'a') => {
            if !app.accounts.is_empty() {
                app.active_account_idx = (app.active_account_idx + 1) % app.accounts.len();
                app.sync_active_account();
                app.refresh();
                if let Some(acc) = app.accounts.get(app.active_account_idx) {
                    app.notify(format!("switched to account: {}", acc.meta.name), theme::PALE);
                }
            } else {
                app.notify("no accounts: press 'c' to create one", theme::BAD);
            }
            None
        }
        (0, 'n') => Some(Action::NewNote),
        (0, 'd') => {
            if let Some(note) = app.notes.get(app.selected_note) {
                if note.amount.is_some() {
                    app.notify(format!("note '{}' is already deposited", note.name), theme::AMBER);
                } else {
                    app.prompt = Some(crate::app::InputPrompt::DepositAmount { note: note.name.clone(), buffer: "0.01".to_string() });
                }
            } else {
                app.notify("no note selected: press 'n' for a new one", theme::BAD);
            }
            None
        }
        (0, 'w') => {
            if let Some(note) = app.notes.get(app.selected_note) {
                if note.amount.is_none() {
                    app.notify(format!("note '{}' has no deposit yet (press 'd')", note.name), theme::BAD);
                } else {
                    let default_recipient = app
                        .accounts
                        .get(app.active_account_idx)
                        .map(|a| a.meta.account.clone())
                        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".to_string());
                    app.prompt = Some(crate::app::InputPrompt::WitnessRecipient { note: note.name.clone(), buffer: default_recipient });
                }
            } else {
                app.notify("no note selected: press 'n' for a new one", theme::BAD);
            }
            None
        }
        (0, 'm') => Some(Action::Mempool),
        (0, 'F') => Some(Action::Fund),
        (0, 'D') => {
            app.confirm = Some((
                "Run the full demo? Two notes, deposits (0.02 testnet ETH), proofs, mixnet sends, then the exit prover settles them."
                    .into(),
                Action::Demo,
            ));
            None
        }
        (0, 'e') => {
            // The local sidecar while it runs, else the ERC-8421 devnet's own node.
            let node = if app.mempool_running() { commands::mempool_url() } else { "https://altmempool.forshtat.com/rpc".into() };
            let url = links::mempool_explorer(&node);
            match links::open(&url) {
                Ok(()) => app.notify(format!("opened the alt mempool explorer for {node}"), theme::MUTED),
                Err(e) => app.notify(format!("could not open a browser ({e}): {url}"), theme::BAD),
            }
            None
        }
        (0, 'p') => Some(Action::NoteStep("prove")),
        (0, 's') => Some(Action::NoteStep("send")),
        (0, 'r') => {
            app.confirm = Some((
                "Run the exit prover? It pairs two spends, recurses for 12 to 20 minutes, then settles on chain 8141.".into(),
                Action::Prove,
            ));
            None
        }

        // Tab 1: Mixnet Map
        (1, 'z') => {
            app.zoom = !app.zoom;
            None
        }
        (1, 'l') => Some(Action::LocalUp),
        (1, 'k') => Some(Action::LocalDown),
        (1, 'p') => Some(Action::Ping),
        _ => None,
    };
    if let Some(a) = action {
        app.act(a);
    }
}
