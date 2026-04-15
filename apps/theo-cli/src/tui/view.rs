//! TUI view — pure rendering from TuiState to ratatui Frame.
//!
//! Layout:
//! ┌─ header (1 line) ───────────────────────────────────┐
//! │ transcript (flex, scrollable)                        │
//! ├─ input (3 lines) ──────────────────────────────────-─┤
//! ├─ status line (1 line) ──────────────────────────────-─┤
//! └──────────────────────────────────────────────────────┘

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::app::{TuiState, TranscriptEntry, ToolCardStatus, SessionPickerState};

/// Draw the full TUI layout.
pub fn draw(frame: &mut Frame, state: &TuiState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),  // header
        Constraint::Min(1),    // transcript
        Constraint::Length(3), // input
        Constraint::Length(1), // status line
    ])
    .split(frame.area());

    render_header(frame, chunks[0], state);
    render_transcript(frame, chunks[1], state);
    render_input(frame, chunks[2], state);
    render_status_line(frame, chunks[3], state);

    // Search bar overlay (above input)
    if state.search_mode {
        let search_area = Rect::new(
            chunks[2].x,
            chunks[2].y.saturating_sub(1),
            chunks[2].width,
            1,
        );
        let results_info = if state.search_results.is_empty() {
            if state.search_query.is_empty() { String::new() } else { " (no matches)".to_string() }
        } else {
            format!(" ({}/{})", state.search_current + 1, state.search_results.len())
        };
        let search_text = format!("/{}{}", state.search_query, results_info);
        let search = Paragraph::new(search_text)
            .style(Style::default().fg(Color::Yellow).bg(Color::Black));
        frame.render_widget(search, search_area);
    }

    // Session picker overlay
    if let Some(ref picker) = state.session_picker {
        render_session_picker(frame, picker);
    }

    // Help overlay (on top of everything)
    if state.show_help {
        render_help_overlay(frame);
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &TuiState) {
    let total_tokens = state.status.tokens_in + state.status.tokens_out;
    let tokens_display = if total_tokens > 1000 {
        format!("{:.1}k tok", total_tokens as f64 / 1000.0)
    } else {
        format!("{total_tokens} tok")
    };

    let left = format!(
        " theo · {} · {} ",
        state.status.mode.to_lowercase(),
        state.status.model,
    );

    let right = format!(" {} ", tokens_display);

    let header_width = area.width as usize;
    let fill_len = header_width.saturating_sub(left.len() + right.len());
    let fill = "─".repeat(fill_len);

    let header_text = format!("{left}{fill}{right}");

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    frame.render_widget(header, area);
}

fn render_transcript(frame: &mut Frame, area: Rect, state: &TuiState) {
    let mut lines: Vec<Line> = Vec::new();

    for entry in &state.transcript {
        match entry {
            TranscriptEntry::User(text) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(text.as_str(), Style::default().fg(Color::Cyan)),
                ]));
            }
            TranscriptEntry::Assistant(text) => {
                // Render markdown for assistant messages
                let md_lines = super::markdown::markdown_to_lines(text);
                lines.extend(md_lines);
                // Show cursor if still streaming
                if state.streaming_assistant && state.cursor_visible {
                    if let Some(last) = lines.last_mut() {
                        last.spans.push(Span::styled("▋", Style::default().fg(Color::White)));
                    }
                }
            }
            TranscriptEntry::ToolCard(card) => {
                lines.push(Line::from(""));

                // Tool card header
                let (status_icon, status_color) = match card.status {
                    ToolCardStatus::Running => ("⠋", Color::Yellow),
                    ToolCardStatus::Succeeded => ("✓", Color::Green),
                    ToolCardStatus::Failed => ("✗", Color::Red),
                };

                let duration_str = match card.duration_ms {
                    Some(ms) if ms > 1000 => format!(" {:.1}s", ms as f64 / 1000.0),
                    Some(ms) => format!(" {}ms", ms),
                    None => {
                        let elapsed = card.started_at.elapsed().as_millis();
                        format!(" {:.1}s...", elapsed as f64 / 1000.0)
                    }
                };

                lines.push(Line::from(vec![
                    Span::styled("  ┌─ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(card.tool_name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(duration_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(format!(" {status_icon}"), Style::default().fg(status_color)),
                    Span::styled(" ─", Style::default().fg(Color::DarkGray)),
                ]));

                // Stdout lines (last 5)
                for stdout_line in &card.stdout_lines {
                    let display = if stdout_line.len() > (area.width as usize).saturating_sub(6) {
                        let max = (area.width as usize).saturating_sub(7);
                        format!("{}…", &stdout_line[..max.min(stdout_line.len())])
                    } else {
                        stdout_line.clone()
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(display, Style::default().fg(Color::DarkGray)),
                    ]));
                }

                // Close card
                lines.push(Line::from(Span::styled(
                    "  └─────",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            TranscriptEntry::SystemMessage(text) => {
                lines.push(Line::from(Span::styled(
                    format!("  {text}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }
    }

    // Apply scroll offset (from bottom)
    let visible_height = area.height as usize;
    let total_lines = lines.len();
    let start = if state.scroll_locked_to_bottom {
        total_lines.saturating_sub(visible_height)
    } else {
        total_lines.saturating_sub(visible_height + state.scroll_offset)
    };

    let visible_lines: Vec<Line> = lines.into_iter().skip(start).take(visible_height).collect();

    let transcript = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
    frame.render_widget(transcript, area);
}

fn render_input(frame: &mut Frame, area: Rect, state: &TuiState) {
    let placeholder = if state.agent_running {
        "aguardando agente..."
    } else {
        "Digite uma tarefa ou /help"
    };

    let display_text = if state.input_text.is_empty() {
        placeholder.to_string()
    } else {
        state.input_text.clone()
    };

    let style = if state.input_text.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    let input = Paragraph::new(display_text)
        .style(style)
        .block(Block::default().borders(Borders::TOP));

    frame.render_widget(input, area);

    // Position cursor
    if !state.input_text.is_empty() && !state.agent_running {
        let cursor_x = area.x + state.input_cursor as u16;
        let cursor_y = area.y + 1; // +1 for border
        if cursor_x < area.x + area.width {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_status_line(frame: &mut Frame, area: Rect, state: &TuiState) {
    let tools_str = if state.status.tools_running > 0 {
        format!(" │ {} tools", state.status.tools_running)
    } else {
        String::new()
    };

    let status_text = format!(
        " {} │ {} │ {}/{} iter{} │ ? ajuda  Ctrl+C sair",
        state.status.mode,
        state.status.phase,
        state.status.iteration,
        state.status.max_iterations,
        tools_str,
    );

    let status = Paragraph::new(status_text)
        .style(Style::default().fg(Color::DarkGray).bg(Color::Black));

    frame.render_widget(status, area);
}

fn render_help_overlay(frame: &mut Frame) {
    let area = frame.area();
    let help_width = 50u16.min(area.width.saturating_sub(4));
    let help_height = 18u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(help_width)) / 2;
    let y = (area.height.saturating_sub(help_height)) / 2;
    let help_area = Rect::new(x, y, help_width, help_height);

    // Clear background
    let clear = Block::default()
        .style(Style::default().bg(Color::Black));
    frame.render_widget(clear, help_area);

    let help_lines = vec![
        Line::from(Span::styled(" Keybindings", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Ctrl+C     ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled(" Enter      ", Style::default().fg(Color::Yellow)),
            Span::raw("Submit prompt"),
        ]),
        Line::from(vec![
            Span::styled(" Shift+Tab  ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle mode (Agent/Plan/Ask)"),
        ]),
        Line::from(vec![
            Span::styled(" Esc        ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle this help / dismiss"),
        ]),
        Line::from(vec![
            Span::styled(" PgUp/PgDn  ", Style::default().fg(Color::Yellow)),
            Span::raw("Scroll transcript"),
        ]),
        Line::from(vec![
            Span::styled(" Mouse      ", Style::default().fg(Color::Yellow)),
            Span::raw("Scroll with wheel"),
        ]),
        Line::from(vec![
            Span::styled(" Ctrl+L     ", Style::default().fg(Color::Yellow)),
            Span::raw("Scroll to bottom"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" /help      ", Style::default().fg(Color::Yellow)),
            Span::raw("Show available commands"),
        ]),
        Line::from(vec![
            Span::styled(" /mode      ", Style::default().fg(Color::Yellow)),
            Span::raw("Change agent mode"),
        ]),
        Line::from(vec![
            Span::styled(" /status    ", Style::default().fg(Color::Yellow)),
            Span::raw("Show provider/model info"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Press Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(help_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Help "));

    frame.render_widget(help, help_area);
}

fn render_session_picker(frame: &mut Frame, picker: &SessionPickerState) {
    let area = frame.area();
    let picker_width = 60u16.min(area.width.saturating_sub(4));
    let picker_height = (picker.sessions.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(picker_width)) / 2;
    let y = (area.height.saturating_sub(picker_height)) / 2;
    let picker_area = Rect::new(x, y, picker_width, picker_height);

    let clear = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(clear, picker_area);

    let mut lines = vec![
        Line::from(Span::styled(
            " Resume session or start new",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (i, session) in picker.sessions.iter().enumerate() {
        let is_selected = i == picker.selected;
        let prefix = if is_selected { "▸ " } else { "  " };
        let style = if is_selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            format!("{prefix}{} · {} msgs · {}", session.modified, session.message_count, session.preview),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter: resume  Esc: new session",
        Style::default().fg(Color::DarkGray),
    )));

    let picker_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Sessions "));

    frame.render_widget(picker_widget, picker_area);
}
