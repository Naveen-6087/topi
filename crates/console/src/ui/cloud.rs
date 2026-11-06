//! Cloud: the AWS account, the running cost, the nodes, and anything in the account that bills.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use crate::app::{AWS_HOURLY, App, Mode, now_secs};
use crate::map::city;
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(area);
    let [account, nodes] = Layout::vertical([Constraint::Length(8), Constraint::Min(6)]).areas(left);
    let deployed = app.aws_deployed();
    let hours = app.aws_started.map(|t| now_secs().saturating_sub(t) as f64 / 3600.0).unwrap_or(0.0);
    let lines = vec![
        kv("identity", app.identity.clone().unwrap_or_else(|| "press w".into())),
        kv("deployment", if deployed { "four nodes running, billing" } else { "nothing deployed" }),
        kv("rate", format!("${AWS_HOURLY:.3} per hour while up")),
        kv(
            "this run",
            if deployed {
                format!("{}, ${:.3}", duration(Duration::from_secs_f64(hours * 3600.0)), hours * AWS_HOURLY)
            } else {
                "-".into()
            },
        ),
        kv("node binary", if app.paths.p("data/aws/bin/topi-node").exists() { "built for Linux" } else { "not built: press b" }),
        kv("account audit", if app.audit_done { format!("{} billable item(s)", app.audit.len()) } else { "press a".into() }),
    ];
    f.render_widget(Paragraph::new(lines).block(panel("AWS")), account);

    let rows: Vec<Row> = app
        .targets
        .lock()
        .unwrap()
        .iter()
        .filter(|_| app.mode == Mode::Aws)
        .map(|t| {
            Row::new(vec![
                Cell::from(t.role.clone()).style(Style::default().fg(FG)),
                Cell::from(format!("{} ({})", city(&t.role), t.region)).style(Style::default().fg(MUTED)),
                Cell::from(t.host.clone()).style(Style::default().fg(AMBER)),
                Cell::from(t.instance.clone()).style(Style::default().fg(DIM)),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(rows, [Constraint::Length(12), Constraint::Length(28), Constraint::Length(16), Constraint::Min(10)])
            .header(Row::new(vec!["role", "region", "address", "instance"]).style(Style::default().fg(DIM)))
            .block(panel("nodes (t3.micro, terraform)")),
        nodes,
    );

    let rows: Vec<Row> = if app.audit.is_empty() {
        let text = if app.audit_done { "nothing billable found" } else { "a scans every enabled region" };
        vec![Row::new(vec![Cell::from(text).style(Style::default().fg(if app.audit_done { OK } else { DIM }))])]
    } else {
        app.audit.iter().map(|a| Row::new(a.iter().map(|x| Cell::from(x.clone())).collect::<Vec<_>>())).collect()
    };
    f.render_widget(
        Table::new(rows, [Constraint::Length(15), Constraint::Length(14), Constraint::Length(22), Constraint::Min(8)])
            .block(panel("what can bill")),
        right,
    );
}
