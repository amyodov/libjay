//! libjay core: compiles J/APL expressions to a shared IR and executes them.
//!
//! The public surface is deliberately PCRE-shaped: [`compile`] turns a
//! string into a reusable [`Program`]; running it with data is a separate
//! step owned by the caller.

pub mod array;
pub mod dtype;
pub mod error;
pub mod fmt;
pub mod frontend;
pub mod ir;
pub mod verb;

pub use array::{Array, Data};
pub use dtype::DType;
pub use error::{Error, ErrorKind, Result, Span};
pub use frontend::{compile, compile_parts, Dialect, Lang};
pub use ir::{ParamSpec, Program};
