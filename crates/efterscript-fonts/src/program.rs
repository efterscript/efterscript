// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The glyph engine's interface: a font program answers a glyph name with
//! an outline and an advance, cached per program.

use std::fmt;
use std::rc::Rc;

use crate::cff::CffProgram;
use crate::cidfont::Type1CidProgram;
use crate::outline::Glyph;
use crate::truetype::TrueTypeProgram;
use crate::type1::Type1Program;

/// Why a program could not yield a glyph. Every variant is the
/// interpreter's `invalidfont`; the detail is for diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontError {
    /// A charstring or table ended before what was being read.
    Truncated(&'static str),
    /// A charstring operator the interpreter does not know.
    UnknownOperator(u8, Option<u8>),
    /// A charstring operator found fewer operands than it takes.
    Operands(&'static str),
    /// A `Subrs` index outside the array.
    SubrIndex(i32),
    /// Subroutine calls nested deeper than the interpreter allows.
    CallDepth,
    /// A `seac` component with no charstring of that name.
    MissingComponent(Vec<u8>),
    /// A required table is absent from the TrueType program.
    MissingTable([u8; 4]),
    /// A glyph index past the program's glyph count.
    GlyphIndex(u16),
    /// Any other structural fault.
    Malformed(&'static str),
    /// A structure the format allows that this reader does not handle.
    Unsupported(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Truncated(what) => write!(f, "{what} ends early"),
            FontError::UnknownOperator(op, None) => write!(f, "unknown charstring operator {op}"),
            FontError::UnknownOperator(op, Some(esc)) => {
                write!(f, "unknown charstring operator {op} {esc}")
            }
            FontError::Operands(op) => write!(f, "too few operands for {op}"),
            FontError::SubrIndex(index) => write!(f, "subroutine {index} does not exist"),
            FontError::CallDepth => write!(f, "subroutine calls nested too deep"),
            FontError::MissingComponent(name) => {
                write!(
                    f,
                    "seac component /{} missing",
                    String::from_utf8_lossy(name)
                )
            }
            FontError::MissingTable(tag) => {
                write!(f, "table {} missing", String::from_utf8_lossy(tag))
            }
            FontError::GlyphIndex(index) => write!(f, "glyph index {index} out of range"),
            FontError::Malformed(what) => write!(f, "malformed {what}"),
            FontError::Unsupported(what) => write!(f, "unsupported {what}"),
        }
    }
}

impl std::error::Error for FontError {}

/// The kind of program, as the PDF side names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProgramKind {
    Type1,
    TrueType,
    Cff,
    /// A CID-keyed font with Type 1 charstrings, addressed by CID only.
    Type1Cid,
}

/// A font program snapshot: immutable, shared by `Rc`, answering glyph
/// names with outlines and advances.
///
/// Glyph space is the program's own: a Type 1 or CFF program's charstring
/// units (under the font's `FontMatrix`, conventionally thousandths of
/// the em); a TrueType program's font units, with
/// [`Program::units_per_em`] giving the scale a Type 42 font's identity
/// matrix expects the caller to divide by.
#[derive(Debug)]
pub enum Program {
    Type1(Type1Program),
    TrueType(TrueTypeProgram),
    Cff(CffProgram),
    Type1Cid(Type1CidProgram),
}

impl Program {
    pub fn kind(&self) -> ProgramKind {
        match self {
            Program::Type1(_) => ProgramKind::Type1,
            Program::TrueType(_) => ProgramKind::TrueType,
            Program::Cff(_) => ProgramKind::Cff,
            Program::Type1Cid(_) => ProgramKind::Type1Cid,
        }
    }

    /// The glyph of a CID: through a CID-keyed CFF program's charset, a
    /// TrueType program's CID map, or a Type 1 CID program's glyph data
    /// map; `Ok(None)` when the program has no glyph for the CID or is
    /// not addressed by CID at all.
    pub fn glyph_by_cid(&self, cid: u16) -> Result<Option<Rc<Glyph>>, FontError> {
        match self {
            Program::Type1(_) => Ok(None),
            Program::TrueType(program) => program.glyph_by_cid(cid),
            Program::Cff(program) => program.glyph_by_cid(cid),
            Program::Type1Cid(program) => program.glyph_by_cid(cid),
        }
    }

    /// Whether glyphs are found by CID: a CID-keyed CFF program, a
    /// TrueType program with a CID map, or a Type 1 CID program.
    pub fn is_cid_keyed(&self) -> bool {
        match self {
            Program::Type1(_) => false,
            Program::TrueType(program) => program.cid_map().is_some(),
            Program::Cff(program) => program.is_cid_keyed(),
            Program::Type1Cid(_) => true,
        }
    }

    /// The number of CIDs a CID-keyed program is declared over, `None`
    /// for a program addressed by name.
    pub fn cid_count(&self) -> Option<u32> {
        match self {
            Program::Type1(_) => None,
            Program::TrueType(program) => program.cid_map().map(|m| m.count()),
            Program::Cff(program) => program.is_cid_keyed().then(|| program.cid_count()),
            Program::Type1Cid(program) => Some(program.cid_count()),
        }
    }

    /// The glyph named `name`: `Ok(None)` when the program has no glyph
    /// of that name, `Err` when it has one that cannot be interpreted.
    pub fn glyph(&self, name: &[u8]) -> Result<Option<Rc<Glyph>>, FontError> {
        match self {
            Program::Type1(program) => program.glyph(name),
            Program::TrueType(program) => program.glyph(name),
            Program::Cff(program) => program.glyph(name),
            Program::Type1Cid(_) => Ok(None),
        }
    }

    pub fn has_glyph(&self, name: &[u8]) -> bool {
        match self {
            Program::Type1(program) => program.charstring(name).is_some(),
            Program::TrueType(program) => program.gid(name).is_some(),
            Program::Cff(program) => program.gid(name).is_some(),
            Program::Type1Cid(_) => false,
        }
    }

    /// The number of named glyphs the program defines; for a CID-keyed
    /// program, the number of glyphs.
    pub fn glyph_count(&self) -> usize {
        match self {
            Program::Type1(program) => program.charstrings().len(),
            Program::TrueType(program) if program.cid_map().is_some() => {
                usize::from(program.num_glyphs())
            }
            Program::TrueType(program) => program.names().len(),
            Program::Cff(program) => usize::from(program.glyph_count()),
            Program::Type1Cid(program) => program.cids().len(),
        }
    }

    /// The glyph names the program defines, in sorted order.
    pub fn glyph_names(&self) -> Vec<&[u8]> {
        match self {
            Program::Type1(program) => program.charstrings().keys().map(Vec::as_slice).collect(),
            Program::TrueType(program) => program.names().keys().map(Vec::as_slice).collect(),
            Program::Cff(program) => program.glyph_names(),
            Program::Type1Cid(_) => Vec::new(),
        }
    }

    /// Font units per em for a TrueType program; `None` for Type 1 and
    /// CFF, whose glyph space is what the font matrix maps.
    pub fn units_per_em(&self) -> Option<u16> {
        match self {
            Program::Type1(_) | Program::Cff(_) | Program::Type1Cid(_) => None,
            Program::TrueType(program) => Some(program.units_per_em()),
        }
    }
}
