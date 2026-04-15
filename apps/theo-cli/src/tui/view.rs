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

use super::app::{TuiState, TranscriptEntry, ToolCardStatus, SessionPickerState, ToastLevel};

/// Draw the full TUI layout.
pub fn draw(frame: &mut Frame, state: &TuiState) {
    // Split into main + sidebar if sidebar is visible
    let (main_area, sidebar_area) = if state.show_sidebar {
        let h_chunks = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(40),
        ])
        .split(frame.area());
        (h_chunks[0], Some(h_chunks[1]))
    } else {
        (frame.area(), None)
    };

    let chunks = Layout::vertical([
        Constraint::Length(1),  // header
        Constraint::Min(1),    // transcript
        Constraint::Length(3), // input
        Constraint::Length(1), // status line
    ])
    .split(main_area);

    render_header(frame, chunks[0], state);
    render_transcript(frame, chunks[1], state);
    render_input(frame, chunks[2], state);
    render_status_line(frame, chunks[3], state);

    // Sidebar
    if let Some(sb_area) = sidebar_area {
        super::widgets::sidebar::render_sidebar(frame, sb_area, state, state.sidebar_tab);
    }

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

    // Toast notifications (top-right)
    if !state.toasts.is_empty() {
        render_toasts(frame, state);
    }

    // Model picker overlay
    if state.show_model_picker {
        render_model_picker(frame, state);
    }

    // Autocomplete dropdown (above input)
    if state.autocomplete.active && !state.autocomplete.candidates.is_empty() {
        let max_shown = 6.min(state.autocomplete.candidates.len());
        let dropdown_height = max_shown as u16 + 2; // +2 for border
        let input_area = chunks[2];
        let dropdown_area = Rect::new(
            input_area.x,
            input_area.y.saturating_sub(dropdown_height),
            input_area.width.min(50),
            dropdown_height,
        );

        let mut lines: Vec<Line> = Vec::new();
        for (i, candidate) in state.autocomplete.candidates.iter().take(max_shown).enumerate() {
            let is_selected = i == state.autocomplete.selected;
            let prefix = if is_selected { "▸ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let desc = if candidate.description.is_empty() {
                String::new()
            } else {
                format!("  {}", candidate.description)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{prefix}{}", candidate.text), style),
                Span::styled(desc, Style::default().fg(Color::DarkGray)),
            ]));
        }

        let dropdown = Paragraph::new(lines)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)));
        frame.render_widget(dropdown, dropdown_area);
    }

    // Timeline overlay (F4-T04)
    if state.show_timeline && !state.tool_chain.is_empty() {
        render_timeline(frame, state);
    }

    // Approval modal (highest priority overlay)
    if let Some(ref approval) = state.pending_approval {
        render_approval_modal(frame, approval);
    }

    // Help overlay (on top of everything except approval)
    if state.show_help && state.pending_approval.is_none() {
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

    // Budget bar (if tokens used)
    let budget_pct = if state.budget_tokens_limit > 0 {
        (state.budget_tokens_used as f64 / state.budget_tokens_limit as f64 * 100.0) as u8
    } else {
        0
    };
    let budget_str = if budget_pct > 0 {
        format!(" {budget_pct}%")
    } else {
        String::new()
    };

    // Tab indicators
    let tabs_str = if state.tabs.len() > 1 {
        let tabs: Vec<String> = state.tabs.iter().enumerate().map(|(i, t)| {
            if i == state.active_tab {
                format!("[{}]", t.name)
            } else {
                format!(" {} ", t.name)
            }
        }).collect();
        format!(" {} │", tabs.join(""))
    } else {
        String::new()
    };

    let left = format!(
        " theo · {} · {}{}",
        state.status.mode.to_lowercase(),
        state.status.model,
        tabs_str,
    );

    let right = format!(" {}{} ", tokens_display, budget_str);

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

    // Phase indicator with color coding
    let phase = &state.status.phase;
    let phase_color = match phase.as_str() {
        "Initialized" | "READY" => Color::DarkGray,
        "Planning" => Color::Blue,
        "Executing" => Color::Yellow,
        "Evaluating" => Color::Cyan,
        "Converged" => Color::Green,
        "Aborted" => Color::Red,
        _ if phase.starts_with("SubAgent") => Color::Magenta,
        _ => Color::DarkGray,
    };

    let ctrl_c_hint = if state.agent_running { "Ctrl+C interrupt" } else { "Ctrl+C sair" };

    let status_line = Line::from(vec![
        Span::styled(format!(" {} ", state.status.mode), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("│ ", Style::default().fg(Color::DarkGray)),
        Span::styled(phase.as_str(), Style::default().fg(phase_color)),
        Span::styled(format!(" │ {}/{} iter", state.status.iteration, state.status.max_iterations), Style::default().fg(Color::DarkGray)),
        Span::styled(&tools_str, Style::default().fg(Color::Yellow)),
        Span::styled(format!(" │ Esc ajuda  {ctrl_c_hint}"), Style::default().fg(Color::DarkGray)),
    ]);

    let status = Paragraph::new(status_line)
        .style(Style::default().bg(Color::Black));

    frame.render_widget(status, area);
}

fn render_help_overlay(frame: &mut Frame) {
    let area = frame.area();
    let help_width = 50u16.min(area.width.saturating_sub(4));
    let help_height = 36u16.min(area.height.saturating_sub(4));
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
        Line::from(vec![Span::styled(" Ctrl+C     ", Style::default().fg(Color::Yellow)), Span::raw("Interrupt agent / Quit")]),
        Line::from(vec![Span::styled(" Ctrl+D     ", Style::default().fg(Color::Yellow)), Span::raw("Force quit")]),
        Line::from(vec![Span::styled(" Enter      ", Style::default().fg(Color::Yellow)), Span::raw("Submit prompt")]),
        Line::from(vec![Span::styled(" Shift+Tab  ", Style::default().fg(Color::Yellow)), Span::raw("Cycle mode (Agent/Plan/Ask)")]),
        Line::from(vec![Span::styled(" Esc        ", Style::default().fg(Color::Yellow)), Span::raw("Toggle help / dismiss overlays")]),
        Line::from(vec![Span::styled(" Tab        ", Style::default().fg(Color::Yellow)), Span::raw("Toggle sidebar")]),
        Line::from(vec![Span::styled(" Ctrl+F     ", Style::default().fg(Color::Yellow)), Span::raw("Search transcript")]),
        Line::from(vec![Span::styled(" Ctrl+M     ", Style::default().fg(Color::Yellow)), Span::raw("Switch model")]),
        Line::from(vec![Span::styled(" Ctrl+T     ", Style::default().fg(Color::Yellow)), Span::raw("New session tab")]),
        Line::from(vec![Span::styled(" Ctrl+W     ", Style::default().fg(Color::Yellow)), Span::raw("Close tab")]),
        Line::from(vec![Span::styled(" Ctrl+Up    ", Style::default().fg(Color::Yellow)), Span::raw("Restore last prompt")]),
        Line::from(vec![Span::styled(" PgUp/PgDn  ", Style::default().fg(Color::Yellow)), Span::raw("Scroll transcript")]),
        Line::from(""),
        Line::from(Span::styled(" Commands", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![Span::styled(" /login     ", Style::default().fg(Color::Yellow)), Span::raw("Authenticate with LLM provider")]),
        Line::from(vec![Span::styled(" /logout    ", Style::default().fg(Color::Yellow)), Span::raw("Clear auth tokens")]),
        Line::from(vec![Span::styled(" /status    ", Style::default().fg(Color::Yellow)), Span::raw("Show provider/model/tokens")]),
        Line::from(vec![Span::styled(" /mode      ", Style::default().fg(Color::Yellow)), Span::raw("Change mode (agent/plan/ask)")]),
        Line::from(vec![Span::styled(" /model     ", Style::default().fg(Color::Yellow)), Span::raw("Switch model")]),
        Line::from(vec![Span::styled(" /memory    ", Style::default().fg(Color::Yellow)), Span::raw("Manage memories (list/search/delete)")]),
        Line::from(vec![Span::styled(" /skills    ", Style::default().fg(Color::Yellow)), Span::raw("List available skills")]),
        Line::from(vec![Span::styled(" /export    ", Style::default().fg(Color::Yellow)), Span::raw("Export session as markdown")]),
        Line::from(vec![Span::styled(" /theme     ", Style::default().fg(Color::Yellow)), Span::raw("Change color theme")]),
        Line::from(vec![Span::styled(" /timeline  ", Style::default().fg(Color::Yellow)), Span::raw("Show tool chain causality")]),
        Line::from(vec![Span::styled(" /history   ", Style::default().fg(Color::Yellow)), Span::raw("Search across all sessions")]),
        Line::from(vec![Span::styled(" /clear     ", Style::default().fg(Color::Yellow)), Span::raw("Clear transcript")]),
        Line::from(vec![Span::styled(" /quit      ", Style::default().fg(Color::Yellow)), Span::raw("Exit")]),
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

fn render_model_picker(frame: &mut Frame, state: &TuiState) {
    let area = frame.area();
    let width = 45u16.min(area.width.saturating_sub(4));
    let height = (state.available_models.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let picker_area = Rect::new(x, y, width, height);

    let clear = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(clear, picker_area);

    let mut lines = vec![
        Line::from(Span::styled(
            " Select model (Ctrl+M to close)",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (i, model) in state.available_models.iter().enumerate() {
        let is_selected = i == state.model_picker_selected;
        let is_current = *model == state.status.model;
        let prefix = if is_selected { "▸ " } else { "  " };
        let suffix = if is_current { " (current)" } else { "" };
        let style = if is_selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(Span::styled(
            format!("{prefix}{model}{suffix}"),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " j/k: navigate  Enter: select  Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    let picker = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Model "));

    frame.render_widget(picker, picker_area);
}

fn render_toasts(frame: &mut Frame, state: &TuiState) {
    let area = frame.area();
    let max_width = 50u16.min(area.width.saturating_sub(2));

    for (i, toast) in state.toasts.iter().rev().take(3).enumerate() {
        let y = 1 + (i as u16 * 2);
        if y >= area.height.saturating_sub(2) {
            break;
        }
        let x = area.width.saturating_sub(max_width + 1);
        let toast_area = Rect::new(x, y, max_width, 1);

        let (fg, prefix) = match toast.level {
            ToastLevel::Info => (Color::Cyan, "ℹ"),
            ToastLevel::Warning => (Color::Yellow, "⚠"),
            ToastLevel::Error => (Color::Red, "✗"),
        };

        let text = format!(" {prefix} {} ", toast.message);
        let truncated = if text.len() > max_width as usize {
            format!("{}…", &text[..max_width as usize - 1])
        } else {
            text
        };

        let toast_widget = Paragraph::new(truncated)
            .style(Style::default().fg(fg).bg(Color::Black));
        frame.render_widget(toast_widget, toast_area);
    }
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

fn render_timeline(frame: &mut Frame, state: &TuiState) {
    let area = frame.area();
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = (state.tool_chain.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let timeline_area = Rect::new(x, y, width, height);

    let clear = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(clear, timeline_area);

    let mut lines = vec![
        Line::from(Span::styled(
            " Tool Chain (causality)",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (i, entry) in state.tool_chain.iter().enumerate() {
        let (icon, color) = match entry.status {
            super::app::ToolCardStatus::Succeeded => ("✓", Color::Green),
            super::app::ToolCardStatus::Failed => ("✗", Color::Red),
            super::app::ToolCardStatus::Running => ("⠋", Color::Yellow),
        };
        let duration = entry.duration_ms
            .map(|ms| format!(" ({ms}ms)"))
            .unwrap_or_default();
        let arrow = if i > 0 { " → " } else { "   " };
        lines.push(Line::from(vec![
            Span::styled(arrow, Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{icon} "), Style::default().fg(color)),
            Span::styled(entry.tool_name.clone(), Style::default().fg(Color::White)),
            Span::styled(duration, Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Press t to close",
        Style::default().fg(Color::DarkGray),
    )));

    let timeline = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Timeline "));

    frame.render_widget(timeline, timeline_area);
}

fn render_approval_modal(frame: &mut Frame, approval: &super::app::PendingApproval) {
    let area = frame.area();
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 12u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    let clear = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(clear, modal_area);

    let risk_color = match approval.risk_level.as_str() {
        "Low" => Color::Green,
        "Medium" => Color::Yellow,
        "High" => Color::Red,
        "Critical" => Color::LightRed,
        _ => Color::Yellow,
    };

    let args_display = if approval.args_preview.len() > 45 {
        format!("{}...", &approval.args_preview[..45])
    } else {
        approval.args_preview.clone()
    };

    let lines = vec![
        Line::from(Span::styled(
            " Approval Required",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Tool:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&approval.tool_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" Risk:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&approval.risk_level, Style::default().fg(risk_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" Args:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(args_display, Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [a]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("pprove   ", Style::default().fg(Color::DarkGray)),
            Span::styled("[r]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("eject", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let modal = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" ⚠ Governance "));

    frame.render_widget(modal, modal_area);
}
