//! Drawing: the frame around every view, and one module per tab.

mod chain;
#[allow(dead_code)]
mod cloud;
mod network;
#[allow(dead_code)]
mod overview;
#[allow(dead_code)]
mod proofs;
#[allow(dead_code)]
mod prover;
mod wallet;
mod world;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Tabs, Wrap};

use crate::app::{AWS_HOURLY, App, Mode, TABS, now_secs};
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &mut App) {
    f.render_widget(Block::default().style(Style::default().bg(NIGHT).fg(FG)), f.area());
    let jobs_height = if app.show_jobs { if f.area().height >= 44 { 10 } else { 7 } } else { 0 };
    let [head, tabs, body, jobs, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(10),
        Constraint::Length(jobs_height),
        Constraint::Length(1),
    ])
    .areas(f.area());
    header(f, app, head);
    tab_bar(f, app, tabs);
    match app.tab {
        0 => wallet::draw(f, app, body),
        1 => network::draw(f, app, body),
        _ => chain::draw(f, app, body),
    }
    if jobs_height > 0 {
        jobs_pane(f, app, jobs);
    }
    footer(f, app, foot);
    if let Some((text, _)) = &app.confirm {
        let area = centered(f.area(), 66, 7);
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(text.clone().fg(FG)),
                Line::from(""),
                Line::from(vec!["y".fg(OK).bold(), " confirm     ".fg(MUTED), "any other key".fg(BAD).bold(), " cancel".fg(MUTED)]),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(panel("confirm").border_style(Style::default().fg(RED)).style(Style::default().bg(SURFACE))),
            area,
        );
    }
    if let Some(prompt) = &app.prompt {
        let (title, prompt_label, input_text) = match prompt {
            crate::app::InputPrompt::DepositAmount { note, buffer } => {
                (format!("Deposit Note [{note}]"), "Enter amount in ETH to fund privacy note:".to_string(), buffer.clone())
            }
            crate::app::InputPrompt::WitnessRecipient { note, buffer } => {
                (format!("Withdraw Note [{note}] - Set Recipient"), "Enter Ethereum recipient address (0x...):".to_string(), buffer.clone())
            }
        };
        let area = centered(f.area(), 68, 9);
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(prompt_label.fg(FG)),
                Line::from(""),
                Line::from(vec![
                    Span::styled(" > ", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                    Span::styled(input_text, Style::default().fg(OK).add_modifier(Modifier::BOLD)),
                    Span::styled("█", Style::default().fg(PALE)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Enter", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
                    Span::styled(" confirm     ", Style::default().fg(MUTED)),
                    Span::styled("Esc", Style::default().fg(BAD).add_modifier(Modifier::BOLD)),
                    Span::styled(" cancel", Style::default().fg(MUTED)),
                ]),
            ])
            .alignment(Alignment::Center)
            .block(panel(&title).border_style(Style::default().fg(RED)).style(Style::default().bg(SURFACE))),
            area,
        );
    }
}

fn centered(r: Rect, w: u16, h: u16) -> Rect {
    let (w, h) = (w.min(r.width), h.min(r.height));
    Rect::new(r.x + (r.width - w) / 2, r.y + (r.height - h) / 2, w, h)
}

/// A status: a coloured square and a label, like the legends on the website.
fn badge(text: String, color: Color) -> Vec<Span<'static>> {
    vec![Span::styled("■ ", Style::default().fg(color)), Span::styled(format!("{text}   "), Style::default().fg(MUTED))]
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let chain = app.chain.lock().unwrap().clone();
    let up = app.stats.lock().unwrap().values().filter(|s| s.is_some()).count();
    let total = app.targets.lock().unwrap().len().max(1);
    let mut spans = vec![
        Span::styled(" ▀█▀ ", Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        Span::styled("topi", Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        Span::styled("  leave no trace      ", Style::default().fg(DIM)),
    ];
    spans.extend(match app.mode {
        Mode::Aws => badge(format!("AWS {up}/{total}"), if up == total { OK } else { RED }),
        Mode::Local => badge(
            format!("local {up}/{total}"),
            if up == total {
                OK
            } else if up > 0 {
                AMBER
            } else {
                DIM
            },
        ),
        Mode::Off => badge("network off".into(), DIM),
    });
    spans.extend(badge(if chain.block == 0 { "chain 8141 connecting".into() } else { format!("block {}", chain.block) }, AMBER));
    if let Some(start) = app.aws_started {
        let hours = now_secs().saturating_sub(start) as f64 / 3600.0;
        spans.extend(badge(format!("${:.3} so far", hours * AWS_HOURLY), AMBER));
    }
    let running = app.jobs.iter().filter(|j| j.running() && !j.service).count();
    if running > 0 {
        spans.push(Span::styled(format!("   {} {running} running", SPINNER[(app.tick % 4) as usize]), Style::default().fg(RED)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = TABS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let on = i == app.tab;
            Line::from(vec![
                Span::styled(format!("{} ", i + 1), Style::default().fg(if on { RED } else { DIM })),
                Span::styled(
                    t.to_lowercase(),
                    Style::default().fg(if on { FG } else { MUTED }).add_modifier(if on { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ])
        })
        .collect();
    f.render_widget(
        Tabs::new(titles).select(app.tab).highlight_style(Style::default()).divider(Span::raw("  ")).padding(" ", " "),
        Rect { y: area.y + 1, height: 1, ..area },
    );
    seam(f, app, Rect { y: area.y + 2, height: 1, ..area });
}

/// Clean tab bar separator line with active tab indicator and subtle runner dot.
fn seam(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let w = area.width as usize;
    let packet = ((app.tick / 2) % (w as u64 + 20)) as usize;

    let line_chars = vec!['─'; w];
    let spans: Vec<Span> = line_chars
        .into_iter()
        .enumerate()
        .map(|(x, ch)| {
            if x == packet {
                Span::styled("●", Style::default().fg(RED))
            } else {
                Span::styled(ch.to_string(), Style::default().fg(BORDER))
            }
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some((at, text, color)) = &app.toast
        && at.elapsed() < Duration::from_secs(5)
    {
        f.render_widget(Paragraph::new(Span::styled(format!(" {text}"), Style::default().fg(*color))), area);
        return;
    }
    let keys = match app.tab {
        0 => {
            "D full demo   F fund   c new account   a switch account   X delete   n new note   d deposit   w witness   p prove   s send   m alt mempool   e mempool explorer   r run prover"
        }
        1 => "p ping   z zoom map   l start local   k stop local",
        2 => "o dora explorer",
        _ => "",
    };
    // Each hint is "key label"; keys are red, labels quiet.
    let hint = |text: &str| -> Vec<Span<'static>> {
        text.split("   ")
            .filter(|h| !h.is_empty())
            .flat_map(|h| {
                let (k, label) = h.split_once(' ').unwrap_or((h, ""));
                [
                    Span::styled(format!(" {k}"), Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" {label}  "), Style::default().fg(MUTED)),
                ]
            })
            .collect()
    };
    let mut spans = hint(keys);
    spans.extend(hint("tab switch   o site   g github   j jobs   q quit").into_iter().map(|s| s.patch_style(Style::default().fg(DIM))));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn jobs_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let [list, out] = Layout::horizontal([Constraint::Length(36), Constraint::Min(20)]).areas(area);
    let rows = list.height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .jobs
        .iter()
        .enumerate()
        .rev()
        .take(rows)
        .map(|(i, j)| {
            let (mark, color) = if j.running() {
                (SPINNER[(app.tick % 4) as usize], RED)
            } else if j.code() == Some(0) {
                ("ok", OK)
            } else {
                ("x ", BAD)
            };
            let name: String = j.name.chars().take(22).collect();
            let style = if i == app.selected_job { Style::default().fg(Color::Black).bg(RED) } else { Style::default().fg(FG) };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark:<3}"), Style::default().fg(color)),
                Span::styled(format!("{name:<22}"), style),
                Span::styled(format!(" {}", duration(j.elapsed())), Style::default().fg(DIM)),
            ]))
        })
        .collect();
    f.render_widget(List::new(items).block(panel("jobs")), list);

    let (title, lines) = match app.jobs.get(app.selected_job) {
        Some(j) => (j.name.clone(), j.lines.lock().unwrap().clone()),
        None => ("output".into(), vec![]),
    };
    let height = out.height.saturating_sub(2) as usize;
    let max = lines.len().saturating_sub(height);
    if app.follow {
        app.scroll = max;
    }
    app.scroll = app.scroll.min(max);
    let shown: Vec<Line> = lines
        .iter()
        .skip(app.scroll)
        .take(height)
        .map(|l| {
            let color = if l.starts_with("$ ") {
                RED
            } else if l.contains("error") || l.contains("FAILED") || l.contains("panicked") {
                BAD
            } else {
                MUTED
            };
            Line::from(Span::styled(l.clone(), Style::default().fg(color)))
        })
        .collect();
    f.render_widget(Paragraph::new(Text::from(shown)).block(panel(&title)), out);
}
