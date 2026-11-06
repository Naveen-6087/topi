//! Network: the map, the node table, packet rate, what a packet goes through, and the nodes' logs.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, List, ListItem, Paragraph, Row, Sparkline, Table};

use crate::app::{App, Mode, now_secs};
use crate::map::city;
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.mode {
        Mode::Aws => "mixnet on AWS  |  every dot is one 8 KB packet",
        Mode::Local => "mixnet on this machine",
        Mode::Off => "mixnet",
    };
    if app.zoom {
        return super::world::draw(f, app, area, title, false);
    }
    // Responsive horizontal split: if terminal width is very wide, allocate more to map,
    // ensure side panel has at least 48 chars width if available.
    let side_width = if area.width >= 160 {
        Constraint::Length(54)
    } else if area.width >= 120 {
        Constraint::Percentage(42)
    } else {
        Constraint::Percentage(45)
    };
    let [map, side] = Layout::horizontal([Constraint::Min(40), side_width]).areas(area);
    super::world::draw(f, app, map, title, false);

    let target_count = app.targets.lock().unwrap().len();
    let table_height = if target_count > 4 { 11 } else { 7 };
    let hops_height = if side.height >= 38 { 9 } else { 0 };
    let rate_height = if side.height >= 26 { 5 } else { 4 };
    let [table, hops, rate, log] = Layout::vertical([
        Constraint::Length(table_height),
        Constraint::Length(hops_height),
        Constraint::Length(rate_height),
        Constraint::Min(6),
    ])
    .areas(side);

    nodes(f, app, table);
    if hops_height > 0 {
        inspector(f, app, hops);
    }
    let data: Vec<u64> = app.rate.iter().copied().collect();
    let peak = data.iter().max().copied().unwrap_or(0);
    f.render_widget(
        Sparkline::default().data(&data).style(Style::default().fg(AMBER)).block(panel(&format!("traffic (pkt/s) · peak {peak}"))),
        rate,
    );
    node_log(f, app, log);
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, c) in chars.into_iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn nodes(f: &mut Frame, app: &App, area: Rect) {
    let stats = app.stats.lock().unwrap().clone();
    let targets = app.targets.lock().unwrap().clone();
    let rows: Vec<Row> = targets
        .iter()
        .map(|t| {
            let st = stats.get(&t.role).cloned().flatten();
            let state = match &st {
                Some(_) => Span::styled("● UP", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
                None => Span::styled("○ OFF", Style::default().fg(BAD)),
            };
            let role_name = crate::map::node_name(&t.role);
            let detail = match &st {
                Some(s) if t.role.starts_with("mix") => format!("{} btc", fmt_num(s.batches)),
                Some(s) if t.role.starts_with("exit") => format!("{} spd", fmt_num(s.spends)),
                Some(s) => format!("up {}", duration(Duration::from_secs(now_secs().saturating_sub(s.started)))),
                None => String::new(),
            };
            let pkts = st.map(|s| fmt_num(s.received)).unwrap_or_default();
            Row::new(vec![
                Cell::from(state),
                Cell::from(role_name).style(Style::default().fg(FG).add_modifier(Modifier::BOLD)),
                Cell::from(city(&t.role)).style(Style::default().fg(MUTED)),
                Cell::from(pkts).style(Style::default().fg(AMBER)),
                Cell::from(detail).style(Style::default().fg(DIM)),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [Constraint::Length(6), Constraint::Length(14), Constraint::Length(12), Constraint::Length(9), Constraint::Min(8)],
        )
        .header(
            Row::new(vec!["status", "node", "region", "in pkts", "metrics"]).style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
        )
        .block(panel("mixnet nodes")),
        area,
    );
}

/// What each hop does to a packet; the highlighted hop moves along the route.
fn inspector(f: &mut Frame, app: &App, area: Rect) {
    const HOPS: [(&str, &str); 6] = [
        ("sender", "3x X-Wing layers, rate token"),
        ("gateway", "verify token, peel layer 1"),
        ("mix", "peel layer 2, batch shuffle"),
        ("exit", "peel layer 3, queue spend"),
        ("reply-rel", "route reply on sender SURB"),
        ("gateway", "reply held in reply store"),
    ];
    let current = (app.tick / 9 % HOPS.len() as u64) as usize;
    let mut lines: Vec<Line> = HOPS
        .iter()
        .enumerate()
        .map(|(i, (who, what))| {
            let on = i == current;
            let dot = if on { "▶ " } else { "· " };
            let color = if on { if i >= 4 { PALE } else { RED } } else { DIM };
            Line::from(vec![
                Span::styled(dot, Style::default().fg(color)),
                Span::styled(
                    format!("{who:<10}"),
                    if on { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { Style::default().fg(MUTED) },
                ),
                Span::styled(*what, Style::default().fg(if on { FG } else { DIM })),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled("  fixed 8,192-byte uniform Sphinx packets", Style::default().fg(DIM))));
    f.render_widget(Paragraph::new(lines).block(panel("packet journey")), area);
}

fn node_log(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<(u64, String, String)> = vec![];
    for (role, st) in app.stats.lock().unwrap().iter() {
        for l in st.iter().flat_map(|s| s.log.iter()) {
            if let Some((t, m)) = l.split_once(' ') {
                items.push((t.parse().unwrap_or(0), role.clone(), m.to_string()));
            }
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.0));

    // Deduplicate consecutive identical messages from the same role
    let mut deduped: Vec<(String, String, usize)> = vec![];
    for (_, role, msg) in items {
        if let Some(last) = deduped.last_mut()
            && last.0 == role
            && last.1 == msg
        {
            last.2 += 1;
            continue;
        }
        deduped.push((role, msg, 1));
    }

    let rows: Vec<ListItem> = deduped
        .iter()
        .take(area.height.saturating_sub(2) as usize)
        .map(|(role, m, count)| {
            let tag = crate::map::node_tag(role);
            let tag_color = match tag {
                "GW1" | "GW2" | "GW " => PALE,
                "MIX1" | "MIX2" | "MIX" => RED,
                "EXT1" | "EXT2" | "EXT" => OK,
                "REL1" | "REL2" | "REL" => AMBER,
                _ => DIM,
            };
            let color = if m.starts_with("rejected") {
                BAD
            } else if m.contains("queued") || m.contains("settled") {
                OK
            } else if m.contains("batch") {
                AMBER
            } else {
                MUTED
            };
            let count_str = if *count > 1 { format!(" (×{count})") } else { String::new() };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{tag}] "), Style::default().fg(tag_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{m}{count_str}"), Style::default().fg(color)),
            ]))
        })
        .collect();
    f.render_widget(List::new(rows).block(panel("node logs")), area);
}
