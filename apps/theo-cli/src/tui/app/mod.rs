//! TUI state + update logic (T5.4 split, D5).

mod autocomplete_helpers;
mod base64;
mod events_handler;
mod search_runner;
mod state_impl;
mod state_types;
mod update;

pub use autocomplete_helpers::*;
pub use base64::*;
pub use events_handler::*;
pub use search_runner::*;
pub use state_impl::*;
pub use state_types::*;
pub use update::*;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
