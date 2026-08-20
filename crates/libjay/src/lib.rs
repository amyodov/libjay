//! libjay core: compiles J/APL expressions to a shared IR and executes them.
//!
//! The public surface is deliberately PCRE-shaped: [`compile`] turns a
//! string into a reusable [`Program`]; running it with data is a separate
//! step owned by the caller.

pub mod array;
pub mod complex;
pub mod device;
pub mod dtype;
pub mod error;
pub mod exact;
mod explain;
pub mod fmt;
pub mod frontend;
pub mod fuse;
pub mod ir;
mod par;
mod rng;
pub mod simd;
pub mod verb;

pub use array::{Array, Buf, Data, Owner};
pub use complex::Cx;
pub use device::{Device, Precision};
pub use dtype::DType;
pub use error::{Error, ErrorKind, Result, Span};
pub use exact::{Ext, Rat};
pub use frontend::{compile, compile_parts, Dialect, Lang};
pub use ir::{ParamSpec, Program};
