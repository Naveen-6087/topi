//! Overview: the stack, live numbers, the map, and the end-to-end run tracker.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Gauge, Paragraph, Row, Table, Wrap};

use crate::app::{App, Mode, STAGES, now_secs};
use crate::links;
use crate::theme::*;

const WORDMARK: [&str; 5] = ["▀█▀ █▀█ █▀█ █", " █  █ █ █▀▀ █", " █  █▄█ █   █", "", "leave no trace"];
/// Ticks (about 60 ms each) the wordmark takes to decode.
const DECODE: u64 = 30;
const NOISE: [char; 4] = ['▚', '▞', '▖', '▝'];

const STACK: [(&str, &str); 5] = [
    ("transport", "KEM-Sphinx packets, X-Wing (ML-KEM-768 + X25519) per hop"),
    ("rate limit", "Binius64 zero-knowledge epoch tickets over SHA-256"),
    ("spend proofs", "Spartan-WHIR, leanVM recursion, WHIR verifier on chain"),
    ("signatures", "leanSPHINCS for the paymaster and accounts, no ECDSA"),
    ("rules", "Lean 4 model of validation and settle, tested against the chain"),
];

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let tracker = if area.height >= 30 { 9 } else { 0 };
    let [top, middle, bottom] = Layout::vertical([Constraint::Length(8), Constraint::Min(8), Constraint::Length(tracker)]).areas(area);
    let [brand, stack] = Layout::horizontal([Constraint::Length(34), Constraint::Min(30)]).areas(top);

    let mut lines: Vec<Line> = vec![Line::from("")];
    lines.extend(WORDMARK.iter().enumerate().map(|(row, s)| {
        if row >= 3 {
            return Line::from(Span::styled(*s, Style::default().fg(DIM)));
        }
        // The letters decode out of pixel noise as the console opens; the O is red, as in the mark.
        Line::from(
            s.chars()
                .enumerate()
                .map(|(col, ch)| {
                    let settled = app.tick > DECODE || ch == ' ' || noise(row as u64, col as u64, app.tick) % DECODE < app.tick;
                    let glyph = if settled { ch } else { NOISE[(noise(col as u64, row as u64, app.tick) % 4) as usize] };
                    let color = if (4..7).contains(&col) { RED } else { FG };
                    Span::styled(glyph.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD))
                })
                .collect::<Vec<_>>(),
        )
    }));
    lines.push(Line::from(Span::styled(links::short(links::SITE), Style::default().fg(AMBER))));
    lines.push(Line::from(Span::styled(links::short(links::REPO), Style::default().fg(MUTED))));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), brand);

    let rows: Vec<Row> = STACK
        .iter()
        .map(|(k, v)| Row::new(vec![Cell::from(*k).style(Style::default().fg(AMBER)), Cell::from(*v).style(Style::default().fg(FG))]))
        .collect();
    f.render_widget(Table::new(rows, [Constraint::Length(14), Constraint::Min(30)]).block(panel("post-quantum at every layer")), stack);

    let [map, numbers] = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(middle);
    super::world::draw(f, app, map, "a private spend's path", true);
    live(f, app, numbers);
    if tracker > 0 {
        pipeline(f, app, bottom);
    }
}

fn live(f: &mut Frame, app: &App, area: Rect) {
    let stats = app.stats.lock().unwrap().clone();
    let chain = app.chain.lock().unwrap().clone();
    let total_targets = app.targets.lock().unwrap().len().max(1);
    let all = || stats.values().flatten();
    let deposits = chain.events.iter().filter(|e| e.kind == "deposit").count();
    let settles = chain.events.iter().filter(|e| e.kind == "settle").count();
    let total_batches: u64 = stats.iter().filter(|(k, _)| k.starts_with("mix")).filter_map(|(_, s)| s.as_ref()).map(|s| s.batches).sum();
    let total_cover: u64 = stats.iter().filter(|(k, _)| k.starts_with("exit")).filter_map(|(_, s)| s.as_ref()).map(|s| s.cover).sum();
    let lines = vec![
        kv(
            "network",
            match app.mode {
                Mode::Aws => "AWS, four regions",
                Mode::Local if total_targets > 4 => "this machine (8 nodes)",
                Mode::Local => "this machine",
                Mode::Off => "off",
            },
        ),
        kv("nodes up", format!("{}/{}", all().count(), total_targets)),
        kv("packets relayed", all().map(|s| s.received).sum::<u64>().to_string()),
        kv("batches", total_batches.to_string()),
        kv("cover packets", total_cover.to_string()),
        kv("pool balance", eth(chain.pool_wei)),
        kv("PQ account", eth(chain.account_wei)),
        kv("pool events", format!("{deposits} deposits, {settles} settles")),
        kv("Lean", format!("{} main theorems, all checked", app.theorem_count())),
    ];
    f.render_widget(Paragraph::new(lines).block(panel("live")), area);
}

/// Stages of the latest end-to-end run, from logs/pipeline.log (whoever ran the steps).
fn pipeline(f: &mut Frame, app: &App, area: Rect) {
    if app.pipeline.is_empty() {
        let text = vec![
            Line::from(Span::styled(" No run yet. An end-to-end run, in order:", Style::default().fg(MUTED))),
            Line::from(Span::styled(
                " Cloud L (or u, p)  >  Network p  >  Wallet n, d, w, p, s (two notes)  >  Prover f, r  >  Chain  >  Cloud D",
                Style::default().fg(FG),
            )),
            Line::from(Span::styled(" Each step's progress appears here, whichever terminal runs it.", Style::default().fg(DIM))),
        ];
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(panel("end-to-end run")), area);
        return;
    }
    let start = app.pipeline.first().map(|p| p.0).unwrap_or(0);
    let done: Vec<Option<u64>> = STAGES.iter().map(|(_, key)| app.pipeline.iter().find(|p| p.2.contains(key)).map(|p| p.0)).collect();
    let finished = done.iter().filter(|d| d.is_some()).count();
    let current = done.iter().position(Option::is_none);
    let mut spans = vec![];
    for (i, (name, _)) in STAGES.iter().enumerate() {
        let (mark, style) = match (done[i], Some(i) == current) {
            (Some(_), _) => ("[x]", Style::default().fg(OK)),
            (None, true) => ("[>]", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            _ => ("[ ]", Style::default().fg(DIM)),
        };
        spans.push(Span::styled(format!("{mark} {name}"), style));
        if let Some(t) = done[i] {
            spans.push(Span::styled(format!(" {}", duration(Duration::from_secs(t - start))), Style::default().fg(DIM)));
        }
        spans.push(Span::raw("   "));
    }
    let elapsed = duration(Duration::from_secs(now_secs().saturating_sub(start)));
    let outer = panel("end-to-end run");
    let inner = outer.inner(area);
    f.render_widget(
        outer.title_bottom(Span::styled(format!(" {finished}/{} stages, {elapsed} ", STAGES.len()), Style::default().fg(DIM))),
        area,
    );
    let [stages, gauge, log] = Layout::vertical([Constraint::Length(3), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new(Line::from(spans)).wrap(Wrap { trim: true }), stages);
    f.render_widget(
        Gauge::default().gauge_style(Style::default().fg(RED).bg(SURFACE)).ratio(finished as f64 / STAGES.len() as f64).label(""),
        gauge,
    );
    let tail: Vec<Line> = app
        .pipeline
        .iter()
        .rev()
        .take(log.height as usize)
        .map(|(t, source, message)| {
            Line::from(vec![
                Span::styled(format!(" +{:<8}", duration(Duration::from_secs(t - start))), Style::default().fg(DIM)),
                Span::styled(format!("{source:<12}"), Style::default().fg(AMBER)),
                Span::styled(message.clone(), Style::default().fg(MUTED)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(tail), log);
}
