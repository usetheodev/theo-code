//! Per-language data-model extraction (T2.4 split, D5).

mod csharp;
mod go;
mod helpers;
mod java;
mod python;
mod rust;
mod shared;
mod typescript;

pub use shared::*;

#[cfg(test)]
#[path = "data_models_tests.rs"]
mod tests;
