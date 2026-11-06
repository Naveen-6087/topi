//! The world map: the nodes in their regions, the curved routes, packets in flight.
//!
//! Visual Diagram:
//! ```text
//!              [GATEWAY]          [MIX]          [EXIT]
//!               gw-1 ◉           mix-1 ◉        exit-1 ◉
//!   [ You ] ───                      ───────────         ───→ [ Chain ]
//!               gw-2 ◉           mix-2 ◉        exit-2 ◉
//!                                   ▲              │
//!                                   │  [RELAY]     ▼
//!                                   └── relay-1 ◉ ─┘
//!                                       relay-2 ◉
//! ```

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::canvas::{Canvas, Circle, Line as Segment, Map, MapResolution, Points};

use crate::app::{App, Mode};
use crate::map::{HOPS, STRATIFIED_HOPS, arc, city, node_name, place};
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect, title: &str, _compact: bool) {
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(ratatui::widgets::Block::default().style(Style::default().bg(NIGHT)), area);

    let stats = app.stats.lock().unwrap().clone();
    let live = app.mode != Mode::Off;
    let is_stratified =
        stats.contains_key("gw-1") || stats.contains_key("gw-2") || app.targets.lock().unwrap().iter().any(|t| t.role == "gw-1");

    // One terminal cell in map units, so labels sit beside their node at any panel size.
    let cell_x = 360.0 / area.width.saturating_sub(2).max(1) as f64;
    let cell_y = 145.0 / area.height.saturating_sub(2).max(1) as f64;
    let canvas = Canvas::default()
        .block(panel(title).style(Style::default().bg(NIGHT)))
        .marker(Marker::Braille)
        .x_bounds([-180.0, 180.0])
        .y_bounds([-60.0, 85.0])
        .paint(|ctx| {
            // ── 0. The world ─────────────────────────────────────────────
            ctx.draw(&Map { color: Color::Rgb(46, 54, 68), resolution: MapResolution::High });
            ctx.layer();

            // ── 1. Static Hop Paths ──────────────────────────────────────
            let hops_list: &[(&str, &str, Color)] = if is_stratified { &STRATIFIED_HOPS } else { &HOPS };
            let line_alpha = if live { 0.35 } else { 0.15 };
            for &(a, b, color) in hops_list {
                let pts: Vec<(f64, f64)> = (0..=24).map(|i| arc(a, b, i as f64 / 24.0)).collect();
                for w in pts.windows(2) {
                    ctx.draw(&Segment { x1: w[0].0, y1: w[0].1, x2: w[1].0, y2: w[1].1, color: fade(color, line_alpha) });
                }
            }

            // ── 2. Animated Packet Trails ────────────────────────────────
            ctx.layer();
            for fl in &app.flights {
                for k in (1..=4).rev() {
                    if let Some(p) = fl.position(fl.t - k as f64 * 0.04) {
                        ctx.draw(&Points { coords: &[p], color: fade(fl.color, 1.0 - k as f64 * 0.22) });
                    }
                }
            }

            // ── 3. Animated Packet Heads ─────────────────────────────────
            ctx.layer();
            for fl in &app.flights {
                if let Some((x, y)) = fl.position(fl.t) {
                    let g = if fl.heavy { "◆" } else { "●" };
                    ctx.print(x, y, Span::styled(g, Style::default().fg(fl.color).add_modifier(Modifier::BOLD)));
                }
            }

            // ── 4. Nodes and Dots ────────────────────────────────────────
            let roles: &[&str] = if is_stratified {
                &["gw-1", "gw-2", "mix-1", "mix-2", "exit-1", "exit-2", "relay-1", "relay-2"]
            } else {
                &["gateway", "mix", "exit", "reply-relay"]
            };

            for &role in roles {
                let (x, y) = place(role);
                let st = stats.get(role).cloned().flatten();
                let age = app.tick.saturating_sub(app.pulse.get(role).copied().unwrap_or(0));

                // Pulsing ring when active
                if st.is_some() && age < 8 {
                    ctx.draw(&Circle { x, y, radius: 1.0 + age as f64 * 0.4, color: fade(OK, 1.0 - age as f64 * 0.12) });
                }

                let dot_color = match (&st, live) {
                    (_, false) => DIM,
                    (Some(_), _) if age < 3 => Color::White,
                    (Some(_), _) => OK,
                    (None, _) => BAD,
                };

                ctx.print(x - 1.0, y, Span::styled("◉", Style::default().fg(dot_color).add_modifier(Modifier::BOLD)));

                // Name and city beside the node; the gateway's on its left, clear of "You" next to it.
                let name = node_name(role);
                let place_name = city(role);
                let left = matches!(role, "gateway" | "gw-1" | "gw-2");
                let at = |text: &str| if left { x - (text.chars().count() as f64 + 1.5) * cell_x } else { x + 1.5 * cell_x };
                ctx.print(at(name), y + cell_y, Span::styled(name, Style::default().fg(if st.is_some() { FG } else { MUTED })));
                ctx.print(at(place_name), y - cell_y, Span::styled(place_name, Style::default().fg(DIM)));
            }

            // ── 5. You (Sender) ──────────────────────────────────────────
            {
                let (x, y) = place("you");
                ctx.print(x - 1.0, y, Span::styled("◉", Style::default().fg(RED).add_modifier(Modifier::BOLD)));
                ctx.print(x + 1.5 * cell_x, y - cell_y, Span::styled("You", Style::default().fg(RED).add_modifier(Modifier::BOLD)));
            }

            // ── 6. Chain (Settlement) ────────────────────────────────────
            {
                let (x, y) = place("chain");
                ctx.print(x - 1.0, y, Span::styled("◈", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)));
                ctx.print(x + 1.5 * cell_x, y, Span::styled("Chain", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)));
            }
        });

    f.render_widget(canvas, area);
}
