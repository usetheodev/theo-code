//! OpenAI Chat Completions API conversion (T5.2 split, D5).

mod request;
mod response;
mod streaming;

pub use request::*;
pub use response::*;
pub use streaming::*;

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
