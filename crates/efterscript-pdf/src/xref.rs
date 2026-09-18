// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Object table and the classic cross-reference section (ISO 32000-1 §7.5.4):
//! one subsection covering ids 0..size, fixed 20-byte entries, free-list head
//! only.

use crate::doc::Error;

/// Allocated ids receive their offset when the object is written; a `None`
/// still present at finish time is a dangling allocation.
pub(crate) struct ObjectTable {
    offsets: Vec<Option<u64>>,
}

/// Offsets above this cannot be written in a 10-digit entry.
const MAX_OFFSET: u64 = 9_999_999_999;

impl ObjectTable {
    pub(crate) fn new() -> Self {
        ObjectTable {
            offsets: Vec::new(),
        }
    }

    /// Ids are consecutive from 1 in allocation order.
    pub(crate) fn alloc(&mut self) -> u32 {
        self.offsets.push(None);
        self.offsets.len() as u32
    }

    pub(crate) fn record(&mut self, id: u32, offset: u64) -> Result<(), Error> {
        let slot = self
            .offsets
            .get_mut(id as usize - 1)
            .ok_or(Error::UnallocatedObject(id))?;
        if slot.is_some() {
            return Err(Error::ObjectAlreadyWritten(id));
        }
        if offset > MAX_OFFSET {
            return Err(Error::FileTooLarge);
        }
        *slot = Some(offset);
        Ok(())
    }

    pub(crate) fn unwritten(&self) -> Vec<u32> {
        self.offsets
            .iter()
            .enumerate()
            .filter(|(_, o)| o.is_none())
            .map(|(i, _)| i as u32 + 1)
            .collect()
    }

    /// Trailer `Size`: number of entries including the free-list head.
    pub(crate) fn size(&self) -> u32 {
        self.offsets.len() as u32 + 1
    }

    /// The section, from the `xref` keyword through the last entry. Callers
    /// check [`Self::unwritten`] first; a dangling id here is a logic error.
    pub(crate) fn section_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + 20 * self.offsets.len());
        out.extend_from_slice(format!("xref\n0 {}\n", self.size()).as_bytes());
        // Head of the (empty) free list, per ISO 32000-1 §7.5.4.
        out.extend_from_slice(b"0000000000 65535 f\r\n");
        for (i, offset) in self.offsets.iter().enumerate() {
            let offset = offset.unwrap_or_else(|| panic!("object {} never written", i + 1));
            // Exactly 20 bytes: 10-digit offset, 5-digit generation, keyword,
            // two-byte line end.
            out.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_20_bytes() {
        let mut t = ObjectTable::new();
        let a = t.alloc();
        let b = t.alloc();
        assert_eq!((a, b), (1, 2));
        t.record(a, 15).unwrap();
        t.record(b, 1234).unwrap();
        let s = t.section_bytes();
        assert!(s.starts_with(b"xref\n0 3\n"));
        let entries = &s[b"xref\n0 3\n".len()..];
        assert_eq!(entries.len(), 3 * 20);
        assert_eq!(&entries[..20], b"0000000000 65535 f\r\n");
        assert_eq!(&entries[20..40], b"0000000015 00000 n\r\n");
        assert_eq!(&entries[40..60], b"0000001234 00000 n\r\n");
    }

    #[test]
    fn double_write_and_unallocated_are_errors() {
        let mut t = ObjectTable::new();
        let a = t.alloc();
        t.record(a, 0).unwrap();
        assert!(matches!(
            t.record(a, 5),
            Err(Error::ObjectAlreadyWritten(1))
        ));
        assert!(matches!(t.record(9, 5), Err(Error::UnallocatedObject(9))));
    }

    #[test]
    fn unwritten_lists_dangling_ids() {
        let mut t = ObjectTable::new();
        let a = t.alloc();
        let _b = t.alloc();
        let _c = t.alloc();
        t.record(a, 0).unwrap();
        assert_eq!(t.unwritten(), [2, 3]);
    }
}
