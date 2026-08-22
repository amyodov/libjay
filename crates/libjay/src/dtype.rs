//! Element types. The set is deliberately small; nothing here may assume it
//! stays small.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    Bool,
    I64,
    /// An arbitrary-precision integer (J's "extended", `123x`).
    Ext,
    /// An exact ratio of two arbitrary-precision integers (J `1r3`).
    Rat,
    F64,
    /// A complex number, held as an interleaved `[re, im]` pair.
    Complex,
    Char,
    /// An interned name (J `s:`). The element is an index into the
    /// process-wide symbol table, not the text itself.
    Symbol,
    /// A box: every element is itself an array (J `<`, APL `⊂`).
    Box,
}

impl DType {
    pub fn name(self) -> &'static str {
        match self {
            DType::Bool => "boolean",
            DType::I64 => "integer",
            DType::Ext => "extended",
            DType::Rat => "rational",
            DType::F64 => "float",
            DType::Complex => "complex",
            DType::Char => "character",
            DType::Symbol => "symbol",
            DType::Box => "boxed",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            DType::Bool | DType::I64 | DType::Ext | DType::Rat | DType::F64 | DType::Complex
        )
    }

    /// True for the two types that never round.
    pub fn is_exact(self) -> bool {
        matches!(self, DType::Ext | DType::Rat)
    }

    /// Common type two numeric operands widen to. None if incompatible.
    ///
    /// The order is J's numeric tower: an exact type sits above the machine
    /// integers and below the floats, so `1x + 1r2` stays exact while
    /// `1x + 1.5` rounds.
    pub fn promote(a: DType, b: DType) -> Option<DType> {
        use DType::*;
        match (a, b) {
            (Box, Box) => Some(Box),
            (Box, _) | (_, Box) => None,
            (Char, Char) => Some(Char),
            (Char, _) | (_, Char) => None,
            (Symbol, Symbol) => Some(Symbol),
            (Symbol, _) | (_, Symbol) => None,
            (Complex, _) | (_, Complex) => Some(Complex),
            (F64, _) | (_, F64) => Some(F64),
            (Rat, _) | (_, Rat) => Some(Rat),
            (Ext, _) | (_, Ext) => Some(Ext),
            (I64, _) | (_, I64) => Some(I64),
            (Bool, Bool) => Some(Bool),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DType::*;

    #[test]
    fn the_numeric_tower_climbs_bool_integer_extended_rational_float_complex() {
        let tower = [Bool, I64, Ext, Rat, F64, Complex];
        for (i, &a) in tower.iter().enumerate() {
            for (j, &b) in tower.iter().enumerate() {
                let want = tower[i.max(j)];
                // Two booleans are the one pair that stays boolean.
                assert_eq!(super::DType::promote(a, b), Some(want), "{a:?} with {b:?}");
            }
        }
    }

    #[test]
    fn characters_and_boxes_mix_with_nothing() {
        assert_eq!(super::DType::promote(Char, Ext), None);
        assert_eq!(super::DType::promote(Box, Rat), None);
        assert_eq!(super::DType::promote(Symbol, Char), None);
        assert_eq!(super::DType::promote(Symbol, I64), None);
        assert_eq!(super::DType::promote(Symbol, Symbol), Some(Symbol));
    }
}
