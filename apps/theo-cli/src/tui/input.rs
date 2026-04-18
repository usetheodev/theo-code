//! Input task — reads crossterm terminal events and converts to Msg.
//!
//! Runs on a dedicated tokio task. Sends Msg::Quit on Ctrl+C,
//! Msg::InputChar/Backspace/etc for text editing, Msg::Submit on Enter.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, KeyEventKind};
use futures::StreamExt;
use tokio::sync::mpsc;

use super::app::Msg;

/// Read terminal events and forward as Msg.
///
/// This function blocks its task waiting for crossterm events.
/// Exits when the mpsc sender is dropped (render loop closed).
pub async fn input_loop(tx: mpsc::Sender<Msg>) {
    let mut reader = crossterm::event::EventStream::new();

    while let Some(Ok(evt)) = reader.next().await {
        let msg = match evt {
            Event::Key(KeyEvent { code, modifiers, kind: KeyEventKind::Press, .. }) => {
                match (code, modifiers) {
                    // Quit or interrupt
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::InterruptAgent),
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::Quit),

                    // Submit
                    (KeyCode::Enter, m) if !m.contains(KeyModifiers::SHIFT) => {
                        // Submit is handled specially — we need the current input text
                        // Send a marker that mod.rs will convert to Msg::Submit with text
                        Some(Msg::Submit(String::new())) // placeholder, mod.rs handles
                    }

                    // Navigation
                    (KeyCode::Left, _) => Some(Msg::InputLeft),
                    (KeyCode::Right, _) => Some(Msg::InputRight),
                    (KeyCode::Home, _) => Some(Msg::InputHome),
                    (KeyCode::End, _) => Some(Msg::InputEnd),
                    (KeyCode::Backspace, _) => Some(Msg::InputBackspace),
                    (KeyCode::Delete, _) => Some(Msg::InputDelete),

                    // Scroll
                    (KeyCode::PageUp, _) => Some(Msg::ScrollUp(10)),
                    (KeyCode::PageDown, _) => Some(Msg::ScrollDown(10)),

                    // Help overlay
                    (KeyCode::Esc, _) => Some(Msg::ToggleHelp), // dismiss overlay or no-op

                    // Mode cycling
                    (KeyCode::BackTab, _) => Some(Msg::CycleMode),

                    // Sidebar toggle and tab
                    (KeyCode::Tab, _) => Some(Msg::ToggleSidebar),

                    // Clear screen
                    (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::ScrollToBottom),

                    // Search (Ctrl+F)
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::SearchStart),

                    // Restore last prompt (Ctrl+Up or Ctrl+P)
                    (KeyCode::Up, m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::RestoreLastPrompt),
                    (KeyCode::Char('p'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::RestoreLastPrompt),

                    // Model picker
                    (KeyCode::Char('m'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::ToggleModelPicker),

                    // Copy mode (toggle native mouse selection)
                    (KeyCode::Char('y'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::ToggleCopyMode),

                    // Timeline toggle (only when input is empty)
                    // Note: 't' with no modifier goes to InputChar normally,
                    // timeline toggles via /timeline command or sidebar

                    // Session tabs
                    (KeyCode::Char('t'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::NewTab),
                    (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::CloseTab),
                    (KeyCode::Char('1'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::SwitchTab(0)),
                    (KeyCode::Char('2'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::SwitchTab(1)),
                    (KeyCode::Char('3'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::SwitchTab(2)),
                    (KeyCode::Char('4'), m) if m.contains(KeyModifiers::CONTROL) => Some(Msg::SwitchTab(3)),

                    // Character input
                    (KeyCode::Char(c), _) => Some(Msg::InputChar(c)),

                    _ => None,
                }
            }
            Event::Resize(w, h) => Some(Msg::Resize(w, h)),
            Event::Mouse(mouse_event) => {
                match mouse_event.kind {
                    event::MouseEventKind::ScrollUp => Some(Msg::ScrollUp(3)),
                    event::MouseEventKind::ScrollDown => Some(Msg::ScrollDown(3)),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(msg) = msg {
            if tx.send(msg).await.is_err() {
                break; // render loop closed
            }
        }
    }
}
