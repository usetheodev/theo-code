//! CodeModel intermediate representation (T2.4 split, D5).

mod location;
mod misc;
mod model;
mod symbol;

pub use location::*;
pub use misc::*;
pub use model::*;
pub use symbol::*;

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
