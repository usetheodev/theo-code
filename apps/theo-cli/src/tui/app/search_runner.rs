//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn run_search(state: &mut TuiState) {
    state.search_results.clear();
    state.search_current = 0;
    if state.search_query.is_empty() {
        return;
    }
    let query_lower = state.search_query.to_lowercase();
    for (i, entry) in state.transcript.iter().enumerate() {
        let text = match entry {
            TranscriptEntry::User(t) | TranscriptEntry::Assistant(t) | TranscriptEntry::SystemMessage(t) => t,
            TranscriptEntry::ToolCard(card) => &card.tool_name,
        };
        if text.to_lowercase().contains(&query_lower) {
            state.search_results.push(i);
        }
    }
}

