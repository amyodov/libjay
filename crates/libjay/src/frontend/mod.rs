//! Language frontends. Each parses its own syntax into the shared IR.

pub mod apl;
pub mod j;

use crate::error::Result;
use crate::fmt::FmtOpts;
use crate::ir::{ParamSpec, Program};
use crate::verb::Agreement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    J,
    Apl,
}

impl Lang {
    pub fn from_name(name: &str) -> Option<Lang> {
        match name.to_ascii_lowercase().as_str() {
            "j" => Some(Lang::J),
            "apl" => Some(Lang::Apl),
            _ => None,
        }
    }
}

/// Dialect settings supplied by the host. `None` means the language default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dialect {
    /// APL `⎕IO`. J's index origin is 0 and is not configurable.
    pub index_origin: Option<i64>,
}

/// A source text with interpolation holes split out. Spans in every token
/// and error refer to `display`, where hole `i` reads `{name_i}`.
#[derive(Clone, Debug)]
pub struct SourceParts {
    pub display: String,
    pub segments: Vec<Segment>,
    pub param_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Segment {
    /// Literal source text starting at `offset` in `display`.
    Text { text: String, offset: usize },
    /// Interpolation hole: parameter `index`, shown as `{name}` in `display`.
    Param { index: usize, offset: usize, len: usize },
}

impl SourceParts {
    /// Build from pre-split literal parts with holes between them
    /// (the t-string path). `names[i]` sits between `parts[i]` and
    /// `parts[i+1]`; repeated names share one parameter.
    pub fn from_parts(parts: &[&str], names: &[&str]) -> SourceParts {
        assert_eq!(parts.len(), names.len() + 1, "N parts need N-1 holes");
        let mut display = String::new();
        let mut segments = Vec::new();
        let mut param_names: Vec<String> = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if !part.is_empty() {
                segments.push(Segment::Text { text: (*part).to_string(), offset: display.len() });
                display.push_str(part);
            }
            if i < names.len() {
                let name = names[i];
                let index = param_names
                    .iter()
                    .position(|n| n == name)
                    .unwrap_or_else(|| {
                        param_names.push(name.to_string());
                        param_names.len() - 1
                    });
                let shown = format!("{{{name}}}");
                segments.push(Segment::Param { index, offset: display.len(), len: shown.len() });
                display.push_str(&shown);
            }
        }
        SourceParts { display, segments, param_names }
    }

    /// Build from a plain string where `{identifier}` outside quotes is an
    /// interpolation hole (the pre-3.14 and Rust runtime path).
    pub fn from_source(src: &str) -> Result<SourceParts> {
        let bytes = src.as_bytes();
        let mut parts: Vec<String> = vec![String::new()];
        let mut names: Vec<String> = Vec::new();
        let mut in_quote = false;
        let mut i = 0;
        while i < src.len() {
            let ch = src[i..].chars().next().unwrap();
            if ch == '\'' {
                in_quote = !in_quote;
                parts.last_mut().unwrap().push(ch);
                i += 1;
                continue;
            }
            if ch == '{' && !in_quote {
                // Exactly `{identifier}` is an interpolation hole. Any other
                // `{` is literal program text: J spells take as `{.`, drop as
                // `}.`, so the brace itself belongs to the language.
                let rest = &src[i + 1..];
                if let Some(end) = rest.find('}') {
                    let name = &rest[..end];
                    if is_identifier(name) {
                        names.push(name.to_string());
                        parts.push(String::new());
                        i += 2 + end;
                        continue;
                    }
                }
            }
            parts.last_mut().unwrap().push(ch);
            i += ch.len_utf8();
        }
        let _ = bytes;
        let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        Ok(SourceParts::from_parts(&part_refs, &name_refs))
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Compile a plain source string (with `{name}` holes) in the given language.
pub fn compile(lang: Lang, source: &str, dialect: &Dialect) -> Result<Program> {
    let sp = SourceParts::from_source(source)?;
    compile_source_parts(lang, sp, dialect)
}

/// Compile pre-split parts (the t-string path).
pub fn compile_parts(
    lang: Lang,
    parts: &[&str],
    names: &[&str],
    dialect: &Dialect,
) -> Result<Program> {
    compile_source_parts(lang, SourceParts::from_parts(parts, names), dialect)
}

fn compile_source_parts(lang: Lang, sp: SourceParts, dialect: &Dialect) -> Result<Program> {
    let (stmts, agreement, fmt) = match lang {
        Lang::J => (j::parse(&sp)?, Agreement::LeadingPrefix, FmtOpts::J),
        Lang::Apl => {
            let origin = dialect.index_origin.unwrap_or(1);
            (apl::parse(&sp, origin)?, Agreement::ExactOrScalar, FmtOpts::APL)
        }
    };
    let params = sp.param_names.into_iter().map(|name| ParamSpec { name }).collect();
    Ok(Program { stmts, params, display_src: sp.display, agreement, fmt })
}
