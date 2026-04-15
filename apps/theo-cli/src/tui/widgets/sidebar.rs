//! Sidebar widget — toggleable right panel showing context info.
//!
//! Three tabs: Status, Tools, Context.
//! Toggle with Tab key. Auto-shows on terminals > 120 cols.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use crate::tui::app::TuiState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidebarTab {
    Status,
    Tools,
    Context,
}

impl SidebarTab {
    pub fn titles() -> Vec<&'static str> {
        vec!["Status", "Tools", "Context"]
    }

    pub fn index(&self) -> usize {
        match self {
            SidebarTab::Status => 0,
            SidebarTab::Tools => 1,
            SidebarTab::Context => 2,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SidebarTab::Status => SidebarTab::Tools,
            SidebarTab::Tools => SidebarTab::Context,
            SidebarTab::Context => SidebarTab::Status,
        }
    }
}

/// Render the sidebar in the given area.
pub fn render_sidebar(frame: &mut Frame, area: Rect, state: &TuiState, tab: SidebarTab) {
    let chunks = Layout::vertical([
        Constraint::Length(2), // tabs
        Constraint::Min(1),   // content
    ])
    .split(area);

    // Tab bar
    let titles: Vec<Line> = SidebarTab::titles()
        .into_iter()
        .map(|t| Line::from(t))
        .collect();
    let tabs = Tabs::new(titles)
        .select(tab.index())
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .divider("│");
    frame.render_widget(tabs, chunks[0]);

    // Content
    let content = match tab {
        SidebarTab::Status => render_status_tab(state),
        SidebarTab::Tools => render_tools_tab(state),
        SidebarTab::Context => render_context_tab(state),
    };

    let panel = Paragraph::new(content)
        .block(Block::default().borders(Borders::LEFT));
    frame.render_widget(panel, chunks[1]);
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

fn render_tools_tab(state: &TuiState) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {} tools running", state.status.tools_running),
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
    ];

    // Show active tool cards
    for entry in &state.transcript {
        if let crate::tui::app::TranscriptEntry::ToolCard(card) = entry {
            let (icon, color) = match card.status {
                crate::tui::app::ToolCardStatus::Running => ("⠋", Color::Yellow),
                crate::tui::app::ToolCardStatus::Succeeded => ("✓", Color::Green),
                crate::tui::app::ToolCardStatus::Failed => ("✗", Color::Red),
            };
            let duration = card.duration_ms
                .map(|ms| format!(" {ms}ms"))
                .unwrap_or_else(|| {
                    format!(" {:.1}s...", card.started_at.elapsed().as_secs_f64())
                });
            lines.push(Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default().fg(color)),
                Span::styled(card.tool_name.clone(), Style::default().fg(Color::White)),
                Span::styled(duration, Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    if lines.len() <= 2 {
        lines.push(Line::from(Span::styled(
            " No tools executed yet",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

fn render_context_tab(_state: &TuiState) -> Vec<Line<'static>> {
    // Placeholder for GRAPHCTX integration (F5-T03)
    vec![
        Line::from(Span::styled(
            " Context graph",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " (GRAPHCTX integration",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "  coming in F5)",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}
