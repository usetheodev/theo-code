//! Language behavior trait + per-language implementations (T2.2 split, D3).

mod constants;
mod csharp;
mod dispatch;
mod generic;
mod go;
mod java;
mod php;
mod python;
mod ruby;
mod rust;
mod trait_def;
mod typescript;

pub use trait_def::*;
pub(crate) use csharp::*;
pub use dispatch::*;
pub(crate) use generic::*;
pub(crate) use go::*;
pub(crate) use java::*;
pub(crate) use php::*;
pub(crate) use python::*;
pub(crate) use ruby::*;
pub(crate) use typescript::*;

#[cfg(test)]
#[path = "language_behavior_tests.rs"]
mod tests;
