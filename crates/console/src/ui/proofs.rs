//! Proofs: the Lean theorems about the pool.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};

use crate::app::App;
use crate::theme::*;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let [table, notes] = Layout::vertical([Constraint::Min(6), Constraint::Length(5)]).areas(area);
    let rows: Vec<Row> = app
        .theorems
        .iter()
        .map(|(group, names, statement)| {
            Row::new(vec![
                Cell::from(group.clone()).style(Style::default().fg(DIM)),
                Cell::from(names.clone()).style(Style::default().fg(AMBER)),
                Cell::from(statement.clone()).style(Style::default().fg(FG)),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(rows, [Constraint::Length(4), Constraint::Length(46), Constraint::Min(30)])
            .header(Row::new(vec!["", "theorem", "statement"]).style(Style::default().fg(DIM)))
            .block(panel(&format!("Lean 4 model, {} theorems, axioms propext and Quot.sound only, no sorry", app.theorem_count()))),
        table,
    );
    let text = vec![
        Line::from(Span::styled(
            "Groups: 1 frame policy, 2 no double spend, 3 a failed settle changes nothing, 4 no inflation, 5 post-quantum authorization.",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Assumed: soundness of the proof system and of leanSPHINCS. That the model matches the bytecode is tested differentially against the chain.",
            Style::default().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(panel("scope")), notes);
}
