//! Non-standard extensions: behaviours libjay offers that no reference
//! implementation answers, each switched on by name and off by default.
//!
//! An extension is not a dialect. A dialect setting chooses between
//! readings that reference implementations disagree about, and every arm of
//! one is somebody's specification; libjay picks the arm its oracle
//! verifies and the host may pick the other. An extension has no such
//! defence: switching one on makes libjay answer something the references
//! do not, so it is off unless a host asks for it, it is never recorded
//! against the oracle corpus, and it is documented apart, in
//! `docs/extensions.md`.
//!
//! Flags combine with `|`. The set a program compiles under comes from the
//! cascade every other setting follows — the environment names the
//! process's default, [`crate::Dialect::extensions`] overrides it for one
//! compiler, and the surfaces (Python's `extensions=`, the CLI's
//! `--extension`, the C ABI's `jay_compile_ext`) pass that override
//! through. A library that embeds libjay is therefore never at the mercy of
//! the environment its host process happens to carry.
//!
//! Environment names are `LIBJAY_{LANG}_*` for a flag that belongs to one
//! language (`LIBJAY_J_*`, `LIBJAY_APL_*`); a bare `LIBJAY_*` name is
//! reserved for a system- or IR-level flag, and there are none yet.

use std::sync::OnceLock;

/// A set of extension flags. Empty by default; combine with `|`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Extensions(u32);

/// One flag: the name the API and the CLI take, the environment variable
/// that switches it on, the bit, and one line about what it does.
struct Flag {
    name: &'static str,
    env: &'static str,
    bit: Extensions,
    what: &'static str,
}

const FLAGS: &[Flag] = &[Flag {
    name: "j_unicode_strings",
    env: "LIBJAY_J_UNICODE_STRINGS",
    bit: Extensions::J_UNICODE_STRINGS,
    what: "a J quoted literal holds Unicode characters instead of bytes",
}];

impl Extensions {
    /// No extension at all: the languages as the references answer them.
    pub const NONE: Extensions = Extensions(0);

    /// `LIBJAY_J_UNICODE_STRINGS`: a J quoted literal is a vector of
    /// Unicode characters rather than of the bytes that spell them.
    ///
    /// J's literal type is one byte per item, so `# 'é'` is 2 in the
    /// reference and every text verb — `#`, `$`, indexing, `a.`, `e.`,
    /// `i.`, `":`, `u:` — counts bytes. Under this flag a literal holds one
    /// item per character instead, `# 'é'` is 1, and the display encodes
    /// the characters rather than writing the bytes. Convenient for text
    /// that is Unicode throughout; not what J answers.
    pub const J_UNICODE_STRINGS: Extensions = Extensions(1);

    /// Whether every flag in `flags` is in this set.
    pub fn has(self, flags: Extensions) -> bool {
        self.0 & flags.0 == flags.0
    }

    /// The set as a bit mask, which is what the C ABI passes.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// A bit mask back to a set. Unknown bits are refused rather than
    /// ignored: a host that names a flag this build does not have is told.
    pub fn from_bits(bits: u32) -> Result<Extensions, String> {
        let known: u32 = FLAGS.iter().fold(0, |acc, f| acc | f.bit.0);
        if bits & !known != 0 {
            return Err(format!(
                "unknown extension bits: {:#x} (this build has: {})",
                bits & !known,
                Extensions::names().join(", ")
            ));
        }
        Ok(Extensions(bits))
    }

    /// The flag one name spells, or None. Both spellings are accepted: the
    /// environment variable (`LIBJAY_J_UNICODE_STRINGS`) and the short name
    /// the API and the CLI use (`j_unicode_strings`), in any case.
    pub fn by_name(name: &str) -> Option<Extensions> {
        let want = name.trim().to_ascii_lowercase();
        FLAGS
            .iter()
            .find(|f| want == f.name || want == f.env.to_ascii_lowercase())
            .map(|f| f.bit)
    }

    /// Every flag this build has, by name.
    pub fn names() -> Vec<&'static str> {
        FLAGS.iter().map(|f| f.name).collect()
    }

    /// Every flag this build has: name, environment variable, and what
    /// switching it on does. The documented list is generated from this one.
    pub fn catalogue() -> Vec<(&'static str, &'static str, &'static str)> {
        FLAGS.iter().map(|f| (f.name, f.env, f.what)).collect()
    }

    /// The names of the flags in this set.
    pub fn selected(self) -> Vec<&'static str> {
        FLAGS.iter().filter(|f| self.has(f.bit)).map(|f| f.name).collect()
    }

    /// Parse a list of names separated by `|`, a comma or whitespace. An
    /// empty list is [`Extensions::NONE`]; a name no flag has is refused.
    pub fn parse(list: &str) -> Result<Extensions, String> {
        let mut out = Extensions::NONE;
        for name in list.split(['|', ',', ' ', '\t', '\n']).filter(|s| !s.trim().is_empty()) {
            match Extensions::by_name(name) {
                Some(bit) => out |= bit,
                None => {
                    return Err(format!(
                        "unknown extension: {:?} (this build has: {})",
                        name.trim(),
                        Extensions::names().join(", ")
                    ))
                }
            }
        }
        Ok(out)
    }

    /// The process default, from the environment, read once.
    ///
    /// A flag is on where its variable is set to `1`, `true`, `yes` or `on`
    /// (in any case); unset, empty, or any other value leaves it off.
    pub fn from_env() -> Extensions {
        static ENV: OnceLock<Extensions> = OnceLock::new();
        *ENV.get_or_init(|| Extensions::from_lookup(|k| std::env::var(k).ok()))
    }

    /// [`Extensions::from_env`] against any source of values, which is how
    /// the rule is tested without touching the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Extensions {
        let mut out = Extensions::NONE;
        for f in FLAGS {
            let on = lookup(f.env).is_some_and(|v| {
                matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
            });
            if on {
                out |= f.bit;
            }
        }
        out
    }
}

impl std::ops::BitOr for Extensions {
    type Output = Extensions;
    fn bitor(self, rhs: Extensions) -> Extensions {
        Extensions(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Extensions {
    fn bitor_assign(&mut self, rhs: Extensions) {
        self.0 |= rhs.0;
    }
}

impl std::fmt::Debug for Extensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = self.selected();
        if names.is_empty() {
            return f.write_str("Extensions(none)");
        }
        write!(f, "Extensions({})", names.join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::Extensions;

    #[test]
    fn nothing_is_on_by_default() {
        assert_eq!(Extensions::default(), Extensions::NONE);
        assert!(!Extensions::NONE.has(Extensions::J_UNICODE_STRINGS));
    }

    #[test]
    fn names_are_read_in_either_spelling() {
        for spelling in [
            "j_unicode_strings",
            "J_UNICODE_STRINGS",
            "LIBJAY_J_UNICODE_STRINGS",
            " j_unicode_strings ",
        ] {
            assert_eq!(Extensions::by_name(spelling), Some(Extensions::J_UNICODE_STRINGS));
        }
        assert_eq!(Extensions::by_name("j_bytes"), None);
    }

    #[test]
    fn a_list_combines_and_an_unknown_name_is_refused() {
        assert_eq!(Extensions::parse("").unwrap(), Extensions::NONE);
        assert_eq!(
            Extensions::parse("j_unicode_strings|j_unicode_strings").unwrap(),
            Extensions::J_UNICODE_STRINGS
        );
        assert_eq!(
            Extensions::parse("j_unicode_strings, j_unicode_strings").unwrap(),
            Extensions::J_UNICODE_STRINGS
        );
        let e = Extensions::parse("j_unicode_string").unwrap_err();
        assert!(e.contains("unknown extension"), "{e}");
        assert!(e.contains("j_unicode_strings"), "{e}");
    }

    #[test]
    fn the_environment_reads_only_the_true_values() {
        let on = |v: &str| {
            Extensions::from_lookup(|k| {
                (k == "LIBJAY_J_UNICODE_STRINGS").then(|| v.to_string())
            })
            .has(Extensions::J_UNICODE_STRINGS)
        };
        for v in ["1", "true", "TRUE", "yes", "on", " On "] {
            assert!(on(v), "{v:?} should switch the flag on");
        }
        for v in ["", "0", "false", "no", "off", "maybe"] {
            assert!(!on(v), "{v:?} should leave the flag off");
        }
        assert_eq!(Extensions::from_lookup(|_| None), Extensions::NONE);
    }

    #[test]
    fn bits_round_trip_and_unknown_ones_are_refused() {
        let set = Extensions::J_UNICODE_STRINGS;
        assert_eq!(Extensions::from_bits(set.bits()).unwrap(), set);
        assert!(Extensions::from_bits(1 << 31).is_err());
    }

    #[test]
    fn the_debug_form_names_what_is_on() {
        assert_eq!(format!("{:?}", Extensions::NONE), "Extensions(none)");
        assert_eq!(
            format!("{:?}", Extensions::J_UNICODE_STRINGS),
            "Extensions(j_unicode_strings)"
        );
    }
}
