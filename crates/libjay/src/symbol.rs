//! The symbol table: J's `s:` names, held once each and referred to by a
//! small integer.
//!
//! A symbol is an atom whose value is a name. Two symbols made from the
//! same text ARE the same atom — `(s: <'a') = (s: <'a')` is 1 however far
//! apart the two sentences stand — so the text lives in one process-wide
//! table and an array of symbols is an array of indices into it. The table
//! is append-only: an index, once handed out, names the same text for the
//! life of the process, which is what lets [`Data::Symbol`] hold plain
//! `u32`s and copy, slice and index like any other flat buffer.
//!
//! Index 0 is the empty name. It is the fill element, so overtaking an
//! array of symbols needs no table lookup.
//!
//! Symbols order by their TEXT, not by the order they were interned in, so
//! every comparison resolves its operands here first.
//!
//! [`Data::Symbol`]: crate::array::Data::Symbol

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// A symbol's index into the table. `u32` is the width J's own type code
/// implies and leaves an array of symbols half the size of one of boxes.
pub type Id = u32;

/// The empty name, which is also the fill element.
pub const EMPTY: Id = 0;

struct Table {
    names: Vec<Arc<str>>,
    ids: HashMap<Arc<str>, Id>,
}

fn table() -> &'static RwLock<Table> {
    static TABLE: OnceLock<RwLock<Table>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let empty: Arc<str> = Arc::from("");
        RwLock::new(Table {
            names: vec![empty.clone()],
            ids: HashMap::from([(empty, EMPTY)]),
        })
    })
}

/// The id of `name`, adding it to the table if it is new.
///
/// Panics only if the table is exhausted, which needs four billion distinct
/// names in one process.
pub fn intern(name: &str) -> Id {
    let lock = table();
    if let Some(&id) = lock.read().expect("symbol table").ids.get(name) {
        return id;
    }
    let mut t = lock.write().expect("symbol table");
    // Another thread may have added it between the two locks.
    if let Some(&id) = t.ids.get(name) {
        return id;
    }
    let id = Id::try_from(t.names.len()).expect("symbol table index");
    let shared: Arc<str> = Arc::from(name);
    t.names.push(shared.clone());
    t.ids.insert(shared, id);
    id
}

/// The name `id` stands for. An id this process never handed out gives the
/// empty name rather than panicking; no caller can produce one.
pub fn name(id: Id) -> Arc<str> {
    table()
        .read()
        .expect("symbol table")
        .names
        .get(id as usize)
        .cloned()
        .unwrap_or_else(|| Arc::from(""))
}

/// The names of many ids under one lock, which is what a comparison, a sort
/// or a display of a whole array wants.
pub fn names(ids: &[Id]) -> Vec<Arc<str>> {
    let t = table().read().expect("symbol table");
    ids.iter()
        .map(|&id| t.names.get(id as usize).cloned().unwrap_or_else(|| Arc::from("")))
        .collect()
}

/// How many distinct names the process has interned, the empty one
/// included. It only ever grows.
pub fn interned() -> usize {
    table().read().expect("symbol table").names.len()
}

/// Order two symbols by their names.
pub fn cmp(a: Id, b: Id) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    let t = table().read().expect("symbol table");
    let of = |id: Id| t.names.get(id as usize).map(Arc::as_ref).unwrap_or("");
    of(a).cmp(of(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_name_is_id_zero() {
        assert_eq!(intern(""), EMPTY);
        assert_eq!(&*name(EMPTY), "");
    }

    #[test]
    fn the_same_text_interns_to_the_same_id() {
        let a = intern("libjay-test-alpha");
        let b = intern("libjay-test-alpha");
        assert_eq!(a, b);
        assert_ne!(a, intern("libjay-test-beta"));
        assert_eq!(&*name(a), "libjay-test-alpha");
    }

    #[test]
    fn symbols_order_by_text_not_by_when_they_were_interned() {
        // "zzz" is interned first and still sorts last.
        let z = intern("libjay-test-zzz");
        let a = intern("libjay-test-aaa");
        assert!(z > a || a > z);
        assert_eq!(cmp(z, a), std::cmp::Ordering::Greater);
    }
}
