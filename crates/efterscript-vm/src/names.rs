// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Interned names.
//!
//! Names are byte strings, not UTF-8. The table is append-only and outside
//! `save`/`restore`, so an [`Atom`] stays valid for the life of the table.

use std::collections::HashMap;
use std::fmt;

/// Stable identifier of an interned name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(pub u32);

/// Longest name accepted, per PLRM3 Appendix B.
pub const MAX_NAME_LEN: usize = 127;

/// Interning a name longer than [`MAX_NAME_LEN`] bytes; the program sees
/// `limitcheck`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameTooLong {
    pub len: usize,
}

impl fmt::Display for NameTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "name of {} bytes exceeds the limit of {MAX_NAME_LEN}",
            self.len
        )
    }
}

impl std::error::Error for NameTooLong {}

#[derive(Default, Debug)]
pub struct NameTable {
    by_atom: Vec<Box<[u8]>>,
    by_text: HashMap<Box<[u8]>, Atom>,
}

impl NameTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// The atom for `text`, interning it if it is new.
    pub fn intern(&mut self, text: &[u8]) -> Result<Atom, NameTooLong> {
        if text.len() > MAX_NAME_LEN {
            return Err(NameTooLong { len: text.len() });
        }
        if let Some(&atom) = self.by_text.get(text) {
            return Ok(atom);
        }
        let atom = Atom(u32::try_from(self.by_atom.len()).expect("name table exhausted"));
        self.by_atom.push(text.into());
        self.by_text.insert(text.into(), atom);
        Ok(atom)
    }

    /// The atom for `text` if it has been interned.
    pub fn lookup(&self, text: &[u8]) -> Option<Atom> {
        self.by_text.get(text).copied()
    }

    /// The text of an interned name. Panics on an atom this table did not
    /// issue.
    pub fn text(&self, atom: Atom) -> &[u8] {
        &self.by_atom[atom.0 as usize]
    }

    pub fn get(&self, atom: Atom) -> Option<&[u8]> {
        self.by_atom.get(atom.0 as usize).map(Box::as_ref)
    }

    pub fn len(&self) -> usize {
        self.by_atom.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_atom.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_idempotent_and_stable() {
        let mut t = NameTable::new();
        let a = t.intern(b"moveto").unwrap();
        let b = t.intern(b"lineto").unwrap();
        assert_ne!(a, b);
        assert_eq!(t.intern(b"moveto").unwrap(), a);
        assert_eq!(t.intern(b"lineto").unwrap(), b);
        assert_eq!(t.len(), 2);
        for _ in 0..100 {
            t.intern(b"x").unwrap();
        }
        assert_eq!(t.len(), 3);
        assert_eq!(t.text(a), b"moveto");
        assert_eq!(t.text(b), b"lineto");
    }

    #[test]
    fn atoms_are_dense_and_in_interning_order() {
        let mut t = NameTable::new();
        assert!(t.is_empty());
        assert_eq!(t.intern(b"a").unwrap(), Atom(0));
        assert_eq!(t.intern(b"b").unwrap(), Atom(1));
        assert_eq!(t.intern(b"a").unwrap(), Atom(0));
        assert_eq!(t.intern(b"c").unwrap(), Atom(2));
    }

    #[test]
    fn lookup_does_not_intern() {
        let mut t = NameTable::new();
        assert_eq!(t.lookup(b"def"), None);
        let a = t.intern(b"def").unwrap();
        assert_eq!(t.lookup(b"def"), Some(a));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn names_are_bytes_not_utf8() {
        let mut t = NameTable::new();
        let raw = [0xFF, 0x00, 0xC3, 0x28];
        let a = t.intern(&raw).unwrap();
        assert_eq!(t.text(a), &raw);
        assert_ne!(a, t.intern(&raw[..3]).unwrap());
        assert_ne!(t.intern(b"A").unwrap(), t.intern(b"a").unwrap());
    }

    #[test]
    fn empty_name_is_a_name() {
        let mut t = NameTable::new();
        let e = t.intern(b"").unwrap();
        assert_eq!(t.text(e), b"");
        assert_eq!(t.intern(b"").unwrap(), e);
    }

    #[test]
    fn length_limit() {
        let mut t = NameTable::new();
        let ok = vec![b'n'; MAX_NAME_LEN];
        let atom = t.intern(&ok).unwrap();
        assert_eq!(t.text(atom).len(), MAX_NAME_LEN);
        let long = vec![b'n'; MAX_NAME_LEN + 1];
        assert_eq!(
            t.intern(&long),
            Err(NameTooLong {
                len: MAX_NAME_LEN + 1
            })
        );
        assert_eq!(t.len(), 1);
        assert_eq!(t.lookup(&long), None);
    }

    #[test]
    fn get_is_total() {
        let mut t = NameTable::new();
        let a = t.intern(b"x").unwrap();
        assert_eq!(t.get(a), Some(&b"x"[..]));
        assert_eq!(t.get(Atom(99)), None);
    }
}
