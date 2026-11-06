//! Chain: balances and the pool's deposits and settles.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let chain = app.chain.lock().unwrap().clone();
    let [top, list] = Layout::vertical([Constraint::Length(8), Constraint::Min(8)]).areas(area);
    let addr = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("—").to_string();

    let pq_account_str = if let Some(acc) = app.accounts.get(app.active_account_idx) {
        format!("{} ({})", acc.meta.account, acc.meta.name)
    } else {
        "none (create in Wallet tab with 'c')".to_string()
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(" current block        ", Style::default().fg(DIM)),
            Span::styled(format!("#{}", chain.block), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" pool contract        ", Style::default().fg(DIM)),
            Span::styled(format!("{:<46}", addr(&app.pool, "pool")), Style::default().fg(PALE)),
            Span::styled(eth(chain.pool_wei), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" PQ paymaster         ", Style::default().fg(DIM)),
            Span::styled(format!("{:<46}", addr(&app.pool, "paymaster")), Style::default().fg(PALE)),
            Span::styled(eth(chain.paymaster_wei), Style::default().fg(AMBER)),
        ]),
        Line::from(vec![
            Span::styled(" active PQ account    ", Style::default().fg(DIM)),
            Span::styled(format!("{:<46}", pq_account_str), Style::default().fg(FG)),
            Span::styled(eth(chain.account_wei), Style::default().fg(OK).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" leanSPHINCS verifier ", Style::default().fg(DIM)),
            Span::styled(addr(&app.pool, "verifier"), Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled(" explorer             ", Style::default().fg(DIM)),
            Span::styled(
                crate::links::short(&crate::links::explorer_for(app.pool["chainId"].as_u64().unwrap_or(8141))).to_string(),
                Style::default().fg(OK),
            ),
            Span::styled("  (press 'o' to open)", Style::default().fg(MUTED)),
        ]),
    ];

    let title = match &chain.error {
        Some(e) => format!("chain {} (rpc err: {})", app.pool["chainId"].as_u64().unwrap_or(8141), e.chars().take(40).collect::<String>()),
        None => format!("chain {}", app.pool["chainId"].as_u64().unwrap_or(8141)),
    };
    f.render_widget(Paragraph::new(lines).block(panel(&title)), top);

    let rows: Vec<Row> = chain
        .events
        .iter()
        .map(|e| {
            let (kind_badge, color) = if e.kind == "settle" { ("● SETTLE ", OK) } else { ("◆ DEPOSIT", AMBER) };
            Row::new(vec![
                Cell::from(format!("#{}", e.block)).style(Style::default().fg(MUTED)),
                Cell::from(kind_badge).style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{}...", &e.tx[..e.tx.len().min(18)])).style(Style::default().fg(DIM)),
                Cell::from(e.detail.clone()).style(Style::default().fg(FG)),
            ])
        })
        .collect();

    f.render_widget(
        Table::new(rows, [Constraint::Length(10), Constraint::Length(12), Constraint::Length(22), Constraint::Min(30)])
            .header(
                Row::new(vec!["block", "action", "transaction", "settlement / nullifier details"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .block(panel("pool deposits and settles (newest first)")),
        list,
    );
}
