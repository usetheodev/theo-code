//! Anthropic Messages API conversion.
//!
//! Decomposed during T5.2 of god-files-2026-07-23-plan.md (ADR D5):
//!   - request.rs   — from_request, to_request
//!   - response.rs  — from_response, to_response
//!   - streaming.rs — from_chunk, to_chunk, normalize_usage
//!   - image.rs     — convert_anthropic_image_source, convert_url_to_anthropic_source

mod image;
mod request;
mod response;
mod streaming;

pub use request::*;
pub use response::*;
pub use streaming::*;

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
