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
    InvalidAccess,
    InvalidFileAccess,
    InvalidRestore,
    IoError,
    LimitCheck,
    RangeCheck,
    StackOverflow,
    StackUnderflow,
    TypeCheck,
    Undefined,
    UndefinedFileName,
    UndefinedResult,
    VmFull,
}

impl VmError {
    /// The PostScript error name.
    pub const fn name(self) -> &'static str {
        match self {
            VmError::DictFull => "dictfull",
            VmError::InvalidAccess => "invalidaccess",
            VmError::InvalidFileAccess => "invalidfileaccess",
            VmError::InvalidRestore => "invalidrestore",
            VmError::IoError => "ioerror",
            VmError::LimitCheck => "limitcheck",
            VmError::RangeCheck => "rangecheck",
            VmError::StackOverflow => "stackoverflow",
            VmError::StackUnderflow => "stackunderflow",
            VmError::TypeCheck => "typecheck",
            VmError::Undefined => "undefined",
            VmError::UndefinedFileName => "undefinedfilename",
            VmError::UndefinedResult => "undefinedresult",
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
    fn long_names_map_to_limitcheck() {
        let e: VmError = NameTooLong { len: 200 }.into();
        assert_eq!(e, VmError::LimitCheck);
    }
}
