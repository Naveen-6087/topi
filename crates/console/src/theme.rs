//! Colours, panels and small formatting helpers shared by every view. The palette is the website's: a
//! warm night ground, red for a payment on its way, amber for proofs and settlement, moonlight for
//! replies and everything quiet.

use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

/// A payment on its way, and the brand.
pub const RED: Color = Color::Rgb(255, 74, 36);
/// Proofs, settles, anything that has left the mixnet.
pub const AMBER: Color = Color::Rgb(245, 184, 61);
/// Replies and quiet highlights.
pub const PALE: Color = Color::Rgb(214, 208, 196);
pub const OK: Color = Color::Rgb(125, 211, 176);
pub const BAD: Color = Color::Rgb(255, 111, 145);
pub const FG: Color = Color::Rgb(236, 234, 227);
pub const MUTED: Color = Color::Rgb(162, 157, 148);
pub const DIM: Color = Color::Rgb(100, 95, 88);
pub const BORDER: Color = Color::Rgb(52, 49, 45);
pub const SURFACE: Color = Color::Rgb(26, 25, 23);
pub const BG: (u8, u8, u8) = (14, 13, 12);
pub const NIGHT: Color = Color::Rgb(BG.0, BG.1, BG.2);

pub fn panel(title: &str) -> Block<'_> {
    Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(BORDER)).title(Line::from(
        vec![
            Span::styled(" ■ ", Style::default().fg(RED)),
            Span::styled(format!("{title} "), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        ],
    ))
}

/// A label/value line.
pub fn kv(k: &str, v: impl Into<String>) -> Line<'static> {
    Line::from(vec![Span::styled(format!(" {k:<20}"), Style::default().fg(DIM)), Span::styled(v.into(), Style::default().fg(FG))])
}

pub fn fade(c: Color, k: f64) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let m = |v: u8, bg: u8| (bg as f64 + (v as f64 - bg as f64) * k.clamp(0.0, 1.0)) as u8;
            Color::Rgb(m(r, BG.0), m(g, BG.1), m(b, BG.2))
        }
        c => c,
    }
}

/// A cheap, repeatable hash for pixel noise: the same inputs give the same glyph.
pub fn noise(a: u64, b: u64, c: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ c.wrapping_mul(0x1656_67B1_9E37_79F9);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, s / 60 % 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}.{}s", s, d.subsec_millis() / 100)
    }
}

pub fn eth(wei: u128) -> String {
    format!("{:.4} ETH", wei as f64 / 1e18)
}

pub const SPINNER: [&str; 4] = ["▖", "▘", "▝", "▗"];
