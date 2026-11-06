//! Wallet: Shielded private payments and note lifecycle.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::theme::*;

const COMMANDS: [(&str, &str); 13] = [
    ("c", "New Account      : Generate fresh post-quantum identity & wallet"),
    ("a", "Switch Account   : Cycle between available accounts"),
    ("n", "New Note         : Prepare an unspent private payment note"),
    ("d", "Deposit (Amount) : Fund note into Shielded Pool (custom ETH amount)"),
    ("w", "Witness (Send)   : Build Merkle witness & set custom recipient"),
    ("p", "Prove            : Generate zero-knowledge proof (Spartan-WHIR)"),
    ("s", "Send Privately   : Route proof through Mixnet to Exit queue"),
    ("m", "Alt Mempool      : Start or stop the settle mempool; r then settles through it"),
    ("r", "Settle (Exit)    : Exit prover pairs spends and settles on chain"),
    ("e", "Mempool Explorer : Every admission, rule by rule (explorer.altmempool.wtf)"),
    ("F", "Fund             : Top up the depositing key and the paymaster from the other accounts"),
    ("D", "Full Demo        : Fund, account, 2 notes, deposits, proofs, mixnet, settle in one go"),
    ("X", "Delete Account   : Remove the active account and its secrets"),
];

const PROVER_STAGES: [&str; 4] = ["check spends", "recursion", "settle", "mined"];
const RECURSION_SECS: f64 = 900.0;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let job = app.jobs.iter().rev().find(|j| j.name == "exit prover");
    let has_prover_job = job.is_some();
    let prover_height = if has_prover_job { 4 } else { 0 };

    let [notes, prover_bar, help] =
        Layout::vertical([Constraint::Min(7), Constraint::Length(prover_height), Constraint::Length(COMMANDS.len() as u16 + 3)])
            .areas(area);

    let rows: Vec<Row> = app
        .notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let selected = i == app.selected_note;

            // Shielded Balance (0.01 ETH if deposited)
            let balance_str = n.amount.map(eth).unwrap_or_else(|| "0.0000 ETH".to_string());
            let balance_color = if n.amount.is_some() { OK } else { DIM };

            // Clear Human-Readable Pipeline Progress
            let (progress_str, progress_color) = if n.settled {
                ("100% Settled", OK)
            } else if n.queued.is_some() {
                ("75% Relayed (Mixnet)", AMBER)
            } else if n.proof {
                ("50% Proven (ZK Proof)", PALE)
            } else if n.witness {
                ("25% Ready to Spend", FG)
            } else if n.amount.is_some() {
                ("10% Shielded in Pool", OK)
            } else {
                ("0% Draft Note", DIM)
            };

            // Settlement Status Badge
            let status_badge = if n.settled {
                Span::styled("✓ Settled on-chain", Style::default().fg(OK).add_modifier(Modifier::BOLD))
            } else if let Some(q) = &n.queued {
                Span::styled(format!("⏳ In Exit Queue ({q})"), Style::default().fg(AMBER))
            } else if n.amount.is_some() {
                Span::styled("Shielded (Ready to Send)", Style::default().fg(OK))
            } else {
                Span::styled("Unfunded Note (Press 'd')", Style::default().fg(DIM))
            };

            Row::new(vec![
                Cell::from(if selected { "▶" } else { " " }).style(Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                Cell::from(n.name.clone()).style(Style::default().fg(FG).add_modifier(Modifier::BOLD)),
                Cell::from(balance_str).style(Style::default().fg(balance_color)),
                Cell::from(progress_str).style(Style::default().fg(progress_color)),
                Cell::from(Line::from(status_badge)),
            ])
            .style(if selected { Style::default().bg(SURFACE) } else { Style::default() })
        })
        .collect();

    let title = if let Some(acc) = app.accounts.get(app.active_account_idx) {
        let bal = app.chain.lock().unwrap().account_wei;
        format!(
            "Account [{}] {} · Balance: {} ({} account{})",
            acc.meta.name,
            acc.meta.account,
            eth(bal),
            app.accounts.len(),
            if app.accounts.len() == 1 { "" } else { "s" }
        )
    } else {
        "No account loaded: press 'c' to create your first post-quantum wallet".to_string()
    };

    if rows.is_empty() {
        let msg = if app.accounts.is_empty() {
            Line::from(vec![
                Span::styled("No wallet found. ", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
                Span::styled("Press 'c' to create a fresh post-quantum account.", Style::default().fg(MUTED)),
            ])
        } else {
            Line::from(vec![
                Span::styled("No private notes yet. ", Style::default().fg(FG)),
                Span::styled("Press 'n' to create a note, then 'd' to deposit.", Style::default().fg(MUTED)),
            ])
        };
        f.render_widget(
            Paragraph::new(vec![Line::from(""), msg]).alignment(ratatui::layout::Alignment::Center).block(panel(&title)),
            notes,
        );
    } else {
        f.render_widget(
            Table::new(
                rows,
                [Constraint::Length(2), Constraint::Length(12), Constraint::Length(18), Constraint::Length(25), Constraint::Min(24)],
            )
            .header(
                Row::new(vec!["", "Note", "Shielded Balance", "Progress", "Status"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .block(panel(&title)),
            notes,
        );
    }

    // Render integrated Exit Prover progress bar if active/run
    if let Some(j) = job {
        let lines = j.lines.lock().unwrap();
        let has = |s: &str| lines.iter().any(|l| l.contains(s));
        let all_settled = !app.notes.is_empty() && app.notes.iter().all(|n| n.settled);
        let stage = if has("settled ") || all_settled {
            4
        } else if has("terminal proof ready") {
            2
        } else if has("proving pair") {
            1
        } else {
            0
        };
        let running = j.running();
        let elapsed = j.elapsed();
        let ratio = match stage {
            0 => 0.05,
            1 => 0.1 + 0.75 * (elapsed.as_secs_f64() / RECURSION_SECS).min(0.95),
            2 => 0.9,
            _ => 1.0,
        };
        let label = PROVER_STAGES
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i < stage {
                    format!("[x] {s}")
                } else if i == stage && running {
                    format!("[>] {s}")
                } else {
                    format!("[ ] {s}")
                }
            })
            .collect::<Vec<_>>()
            .join("   ");
        let failed = !running && stage < 4;
        f.render_widget(
            ratatui::widgets::Gauge::default()
                .block(panel(&format!("Exit Prover (Settlement)  {}", duration(elapsed))))
                .gauge_style(Style::default().fg(if failed { BAD } else { AMBER }).bg(SURFACE))
                .ratio(ratio)
                .label(Span::styled(label, Style::default().fg(FG))),
            prover_bar,
        );
    }

    // Command panel with clear instructions
    let mut lines: Vec<Line> = COMMANDS
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!(" [{k}] "), Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                Span::styled(*d, Style::default().fg(MUTED)),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        " Workflow: [n] Note  ──→  [d] Deposit  ──→  [w] Witness  ──→  [p] Prove  ──→  [s] Send  ──→  [r] Settle",
        Style::default().fg(PALE).add_modifier(Modifier::BOLD),
    )));
    f.render_widget(Paragraph::new(lines).block(panel(&format!("wallet actions   {}", mempool_status(app)))), help);
}

/// One line on the alternative mempool, for the actions panel's title.
fn mempool_status(app: &App) -> String {
    match (&app.mempool, app.mempool_running()) {
        (Some(s), _) => {
            let c = &s["counters"];
            let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0);
            let rejected: u64 = c["rejected"].as_object().map(|m| m.values().filter_map(|v| v.as_u64()).sum()).unwrap_or(0);
            format!(
                "alt mempool on: {} pending, {} in flight, {} mined, {} rejected",
                n(&s["pending"]),
                n(&s["inflight"]),
                n(&c["mined"]),
                rejected
            )
        }
        (None, true) => "alt mempool starting".into(),
        (None, false) => "alt mempool off: settles go straight to the chain".into(),
    }
}
