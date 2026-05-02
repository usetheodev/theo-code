//! Single-purpose slice extracted from `tui/app.rs` (T5.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::time::Instant;

use theo_domain::event::{DomainEvent, EventType};

use super::*;
use super::state_types::*;

pub fn update_autocomplete(state: &mut TuiState) {
    use super::super::autocomplete::{self, AutocompleteTrigger};

    let input = &state.input_text;

    if let Some(query) = input.strip_prefix('/') {
        // Slash command autocomplete
        let all = autocomplete::slash_commands();
        let filtered = autocomplete::filter_candidates(&all, query);
        state.autocomplete.active = !filtered.is_empty();
        state.autocomplete.trigger = AutocompleteTrigger::Slash;
        state.autocomplete.query = query.to_string();
        state.autocomplete.candidates = filtered;
        state.autocomplete.selected = 0;
    } else if let Some(at_pos) = input.rfind('@') {
        // @file autocomplete
        let query = &input[at_pos + 1..];
        let candidates = autocomplete::file_candidates(&state.project_dir, query);
        state.autocomplete.active = !candidates.is_empty();
        state.autocomplete.trigger = AutocompleteTrigger::AtFile;
        state.autocomplete.query = query.to_string();
        state.autocomplete.candidates = candidates;
        state.autocomplete.selected = 0;
    } else {
        state.autocomplete.active = false;
    }
}
