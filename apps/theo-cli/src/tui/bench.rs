//! Render latency benchmark — measures frame draw time with TestBackend.
//!
//! Target: p99 < 16ms (60fps capable)

#[cfg(test)]
mod tests {
    use ratatui::prelude::*;
    use ratatui::Terminal;
    use std::time::Instant;

    use crate::tui::app::{TuiState, Msg, TranscriptEntry, ToolCardState, ToolCardStatus};
    use crate::tui::view;
    use crate::tui::app;

    fn populate_state(state: &mut TuiState, n_messages: usize) {
        for i in 0..n_messages {
            if i % 3 == 0 {
                state.transcript.push(TranscriptEntry::User(format!("User message {i} with some text")));
            } else if i % 3 == 1 {
                state.transcript.push(TranscriptEntry::Assistant(format!(
                    "## Response {i}\n\nThis is a **markdown** response with `inline code` and:\n\n```rust\nfn example() {{\n    println!(\"hello {i}\");\n}}\n```\n\nSome more text here."
                )));
            } else {
                state.transcript.push(TranscriptEntry::ToolCard(ToolCardState {
                    call_id: format!("c-{i}"),
                    tool_name: "bash".to_string(),
                    status: ToolCardStatus::Succeeded,
                    started_at: Instant::now(),
                    duration_ms: Some(1500),
                    stdout_lines: vec![
                        "Compiling theo v0.1.0".to_string(),
                        "Finished in 1.5s".to_string(),
                    ],
                }));
            }
        }
    }

    #[test]
    fn benchmark_render_100_messages() {
        let backend = backend::TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = TuiState::new("test".into(), "gpt-4o".into(), 40, 120, 40);
        populate_state(&mut state, 100);

        let mut times: Vec<u128> = Vec::new();
        for _ in 0..100 {
            let start = Instant::now();
            terminal.draw(|f| view::draw(f, &state)).unwrap();
            times.push(start.elapsed().as_micros());
        }

        times.sort();
        let avg = times.iter().sum::<u128>() / times.len() as u128;
        let p99 = times[98]; // 99th percentile
        let max = *times.last().unwrap();

        eprintln!("Render 100 msgs: avg={avg}µs p99={p99}µs max={max}µs");
        // CI environments are slower than real hardware. Target 50ms for CI, 16ms for production.
        assert!(p99 < 50_000, "p99 render should be < 50ms in CI (was {p99}µs)");
    }

    #[test]
    fn benchmark_render_1000_messages() {
        let backend = backend::TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = TuiState::new("test".into(), "gpt-4o".into(), 40, 120, 40);
        populate_state(&mut state, 1000);

        let mut times: Vec<u128> = Vec::new();
        for _ in 0..50 {
            let start = Instant::now();
            terminal.draw(|f| view::draw(f, &state)).unwrap();
            times.push(start.elapsed().as_micros());
        }

        times.sort();
        let avg = times.iter().sum::<u128>() / times.len() as u128;
        let p99 = times[48];
        let max = *times.last().unwrap();

        eprintln!("Render 1000 msgs: avg={avg}µs p99={p99}µs max={max}µs");
        // 1000 messages with markdown parsing is heavy. CI target 200ms, production target 32ms.
        assert!(p99 < 200_000, "p99 render for 1000 msgs should be < 200ms in CI (was {p99}µs)");
    }

    #[test]
    fn benchmark_update_throughput() {
        let mut state = TuiState::new("test".into(), "gpt-4o".into(), 40, 120, 40);

        let start = Instant::now();
        for i in 0..10_000 {
            let event = theo_domain::event::DomainEvent::new(
                theo_domain::event::EventType::ContentDelta,
                "r-1",
                serde_json::json!({"text": format!("token{i} ")}),
            );
            app::update(&mut state, Msg::DomainEvent(event));
        }
        let elapsed = start.elapsed();

        eprintln!("10k updates: {}ms ({}/s)", elapsed.as_millis(), 10_000_000 / elapsed.as_millis().max(1));
        assert!(elapsed.as_millis() < 1000, "10k updates should take < 1s (was {}ms)", elapsed.as_millis());
    }
}
