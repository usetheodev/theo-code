//! Sidebar widget — toggleable right panel showing context info.
//!
//! Currently the sidebar only shows a Status view. Tools/Context tabs were
//! planned but never wired up, so they were removed along with the tab enum.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::TuiState;

/// Render the sidebar in the given area.
pub fn render_sidebar(frame: &mut Frame, area: Rect, state: &TuiState) {
    let content = render_status_tab(state);
    let panel = Paragraph::new(content)
        .block(Block::default().borders(Borders::LEFT).title(" Status "));
    frame.render_widget(panel, area);
}

fn render_status_tab(state: &TuiState) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(" Provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.status.provider.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Model:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.status.model.clone(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Mode:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.status.mode.clone(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Tokens in:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", state.status.tokens_in), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Tokens out: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", state.status.tokens_out), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Total:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", state.status.tokens_in + state.status.tokens_out),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Phase: ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.status.phase.clone(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled(" Iteration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{}", state.status.iteration, state.status.max_iterations),
                Style::default().fg(Color::White),
            ),
        ]),
    ]
}
