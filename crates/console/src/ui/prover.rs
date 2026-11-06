//! Prover: the exit's spend queues and the exit prover's progress.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph, Wrap};

use crate::app::{App, Queue};
use crate::theme::*;

const STAGES: [&str; 4] = ["check spends", "recursion", "settle", "mined"];
/// Typical recursion time, used only to pace the progress bar.
const RECURSION_SECS: f64 = 900.0;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let [queues, bar, latest] = Layout::vertical([Constraint::Length(8), Constraint::Length(4), Constraint::Min(5)]).areas(area);
    let [aws, local, pool] =
        Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(33), Constraint::Percentage(33)]).areas(queues);
    queue(f, &app.aws_queue, "AWS exit queue (fetched)", aws);
    queue(f, &app.local_queue, "local exit queue", local);
    mempool(f, app, pool);

    let job = app.jobs.iter().rev().find(|j| j.name == "exit prover");
    let (stage, detail, elapsed, running) = match job {
        Some(j) => {
            let lines = j.lines.lock().unwrap();
            let has = |s: &str| lines.iter().any(|l| l.contains(s));
            let stage = if has("settled ") {
                4
            } else if has("terminal proof ready") {
                2
            } else if has("proving pair") {
                1
            } else {
                0
            };
            (stage, lines.last().cloned().unwrap_or_default(), j.elapsed(), j.running())
        }
        None => (0, "r runs the exit prover on the queue; it needs two spends".into(), Duration::ZERO, false),
    };
    let ratio = match stage {
        0 => 0.05,
        1 => 0.1 + 0.75 * (elapsed.as_secs_f64() / RECURSION_SECS).min(0.95),
        2 => 0.9,
        _ => 1.0,
    };
    let label = STAGES
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
    let failed = job.is_some() && !running && stage < 4;
    f.render_widget(
        Gauge::default()
            .block(panel(&format!("exit prover  {}", duration(elapsed))))
            .gauge_style(Style::default().fg(if failed { BAD } else { AMBER }).bg(SURFACE))
            .ratio(if job.is_some() { ratio } else { 0.0 })
            .label(Span::styled(label, Style::default().fg(FG))),
        bar,
    );
    let text = vec![
        Line::from(Span::styled(detail, Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(Span::styled(
            "The exit learns nothing secret: client proofs are zero knowledge and the mixnet hides their senders.",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "Recursion takes 12 to 20 minutes and about 12 GB. The settle runs as [VERIFY pool, VERIFY PQ paymaster, SENDER settle].",
            Style::default().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(panel("latest")), latest);
}

/// The alternative mempool the settle goes through, when it runs.
fn mempool(f: &mut Frame, app: &App, area: Rect) {
    let lines = match (&app.mempool, app.mempool_running()) {
        (Some(s), _) => {
            let c = &s["counters"];
            let n = |v: &serde_json::Value| v.as_u64().unwrap_or(0).to_string();
            let rejected: u64 = c["rejected"].as_object().map(|m| m.values().filter_map(|v| v.as_u64()).sum()).unwrap_or(0);
            vec![
                kv("pending", format!("{}  ({} in flight)", n(&s["pending"]), n(&s["inflight"]))),
                kv("admitted", n(&c["admitted"])),
                kv("rejected", rejected.to_string()),
                kv("released / mined", format!("{} / {}", n(&c["released"]), n(&c["mined"]))),
                kv("peers", s["peers"].as_array().map(|p| p.len()).unwrap_or(0).to_string()),
            ]
        }
        (None, true) => vec![Line::from(Span::styled(" starting...", Style::default().fg(MUTED)))],
        (None, false) => vec![
            Line::from(Span::styled(" off: settles go straight to the chain", Style::default().fg(DIM))),
            Line::from(Span::styled(" m starts it; r then settles through it", Style::default().fg(DIM))),
        ],
    };
    f.render_widget(Paragraph::new(lines).block(panel("alt mempool")), area);
}

fn queue(f: &mut Frame, q: &Queue, title: &str, area: Rect) {
    let mut lines =
        vec![kv("waiting", q.pending.len().to_string()), kv("settled", q.done.to_string()), kv("rejected", q.rejected.to_string())];
    lines.extend(q.pending.iter().take(3).map(|p| Line::from(Span::styled(format!("   {p}"), Style::default().fg(AMBER)))));
    f.render_widget(Paragraph::new(lines).block(panel(title)), area);
}
