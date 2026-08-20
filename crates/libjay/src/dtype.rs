//! Element types. The set is deliberately small for now; nothing here may
//! assume it stays small (bigints, rationals arrive later).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    Bool,
    I64,
    F64,
    /// A complex number, held as an interleaved `[re, im]` pair.
    Complex,
    Char,
    /// A box: every element is itself an array (J `<`, APL `⊂`).
    Box,
}

impl DType {
    pub fn name(self) -> &'static str {
        match self {
            DType::Bool => "boolean",
            DType::I64 => "integer",
            DType::F64 => "float",
            DType::Complex => "complex",
            DType::Char => "character",
            DType::Box => "boxed",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, DType::Bool | DType::I64 | DType::F64 | DType::Complex)
    }

    /// Common type two numeric operands widen to. None if incompatible.
    pub fn promote(a: DType, b: DType) -> Option<DType> {
        use DType::*;
        match (a, b) {
            (Box, Box) => Some(Box),
            (Box, _) | (_, Box) => None,
            (Char, Char) => Some(Char),
            (Char, _) | (_, Char) => None,
            (Complex, _) | (_, Complex) => Some(Complex),
            (F64, _) | (_, F64) => Some(F64),
            (I64, _) | (_, I64) => Some(I64),
            (Bool, Bool) => Some(Bool),
        }
    }
}
