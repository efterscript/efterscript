// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! PostScript error values.
//!
//! Only the values: the interpreter's `errordict` handling is layered on top.

use std::fmt;

use crate::names::NameTooLong;

/// A PostScript error, identified by the name the program sees in `$error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VmError {
    DictFull,
    DictStackOverflow,
    DictStackUnderflow,
    ExecStackOverflow,
    InvalidAccess,
    InvalidExit,
    InvalidFileAccess,
    InvalidRestore,
    IoError,
    LimitCheck,
    NoCurrentPoint,
    RangeCheck,
    StackOverflow,
    StackUnderflow,
    SyntaxError,
    TypeCheck,
    Undefined,
    UndefinedFileName,
    UndefinedResult,
    UnmatchedMark,
    VmFull,
}

impl VmError {
    /// Every error, in the order `errordict` lists them.
    pub const ALL: &[VmError] = &[
        VmError::DictFull,
        VmError::DictStackOverflow,
        VmError::DictStackUnderflow,
        VmError::ExecStackOverflow,
        VmError::InvalidAccess,
        VmError::InvalidExit,
        VmError::InvalidFileAccess,
        VmError::InvalidRestore,
        VmError::IoError,
        VmError::LimitCheck,
        VmError::NoCurrentPoint,
        VmError::RangeCheck,
        VmError::StackOverflow,
        VmError::StackUnderflow,
        VmError::SyntaxError,
        VmError::TypeCheck,
        VmError::Undefined,
        VmError::UndefinedFileName,
        VmError::UndefinedResult,
        VmError::UnmatchedMark,
        VmError::VmFull,
    ];

    /// The PostScript error name.
    pub const fn name(self) -> &'static str {
        match self {
            VmError::DictFull => "dictfull",
            VmError::DictStackOverflow => "dictstackoverflow",
            VmError::DictStackUnderflow => "dictstackunderflow",
            VmError::ExecStackOverflow => "execstackoverflow",
            VmError::InvalidAccess => "invalidaccess",
            VmError::InvalidExit => "invalidexit",
            VmError::InvalidFileAccess => "invalidfileaccess",
            VmError::InvalidRestore => "invalidrestore",
            VmError::IoError => "ioerror",
            VmError::LimitCheck => "limitcheck",
            VmError::NoCurrentPoint => "nocurrentpoint",
            VmError::RangeCheck => "rangecheck",
            VmError::StackOverflow => "stackoverflow",
            VmError::StackUnderflow => "stackunderflow",
            VmError::SyntaxError => "syntaxerror",
            VmError::TypeCheck => "typecheck",
            VmError::Undefined => "undefined",
            VmError::UndefinedFileName => "undefinedfilename",
            VmError::UndefinedResult => "undefinedresult",
            VmError::UnmatchedMark => "unmatchedmark",
            VmError::VmFull => "VMerror",
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::error::Error for VmError {}

impl From<NameTooLong> for VmError {
    fn from(_: NameTooLong) -> Self {
        VmError::LimitCheck
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_postscript_error_names() {
        assert_eq!(VmError::InvalidAccess.name(), "invalidaccess");
        assert_eq!(VmError::VmFull.to_string(), "VMerror");
        assert_eq!(VmError::UndefinedFileName.to_string(), "undefinedfilename");
    }

    #[test]
    fn all_lists_every_error_once() {
        let names: std::collections::HashSet<_> = VmError::ALL.iter().map(|e| e.name()).collect();
        assert_eq!(names.len(), VmError::ALL.len());
        assert!(names.contains("execstackoverflow"));
        assert!(names.contains("VMerror"));
    }

    #[test]
    fn long_names_map_to_limitcheck() {
        let e: VmError = NameTooLong { len: 200 }.into();
        assert_eq!(e, VmError::LimitCheck);
    }
}
