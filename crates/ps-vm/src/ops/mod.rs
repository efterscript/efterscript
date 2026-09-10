// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The operator table and the helpers operators share.
//!
//! Each module below contributes a static slice of [`OpEntry`]; [`table`]
//! chains them once per process, and an operator object's index is its
//! position in that chain. `systemdict` lists the public entries; internal
//! entries (the default error handlers) are reachable only through
//! `errordict`; graphics entries enter `systemdict` only when a graphics
//! backend is installed.

use std::sync::OnceLock;

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Type};

/// An operator implementation. It pops its own operands and pushes its
/// results; [`check_sig`] has already verified the operand count and types
/// the entry declares.
pub type OpFn = fn(&mut Interp) -> Result<(), VmError>;

/// Operand classes a signature can declare, deepest operand first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sig {
    Any,
    Int,
    Num,
    Bool,
    Dict,
    /// An array or a packed array.
    Array,
    String,
    Name,
}

impl Sig {
    pub fn accepts(self, object: Object) -> bool {
        match self {
            Sig::Any => true,
            Sig::Int => object.ty() == Type::Integer,
            Sig::Num => object.is_number(),
            Sig::Bool => object.ty() == Type::Boolean,
            Sig::Dict => object.ty() == Type::Dict,
            Sig::Array => matches!(object.ty(), Type::Array | Type::PackedArray),
            Sig::String => object.ty() == Type::String,
            Sig::Name => object.ty() == Type::Name,
        }
    }
}

/// Where an operator's name is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// In `systemdict` from the start.
    Public,
    /// Reachable only through `errordict`.
    Internal,
    /// Defined in `systemdict` by `Interp::set_graphics_backend`.
    Graphics,
    /// Reachable only through a built-in `ProcSet` resource dictionary.
    ProcSet,
    /// Reachable only through `serverdict`.
    Server,
}

#[derive(Clone, Copy, Debug)]
pub struct OpEntry {
    pub name: &'static str,
    pub func: OpFn,
    pub sig: &'static [Sig],
    pub visibility: Visibility,
}

/// Declares a module's operator table:
///
/// ```ignore
/// op_table! { OPS {
///     "add" => add, [Num, Num];
///     "clear" => clear;
/// }}
/// ```
///
/// `internal OPS { … }` marks every entry internal, `graphics OPS { … }`
/// marks every entry as belonging to the graphics group, `procset OPS
/// { … }` as belonging to a built-in procedure set, and `server OPS
/// { … }` as an entry of `serverdict`.
macro_rules! op_table {
    ($table:ident { $($name:expr => $func:expr $(, [$($sig:ident),* $(,)?])?;)* }) => {
        op_table!(@build $table, Public, { $($name => $func $(, [$($sig),*])?;)* });
    };
    (internal $table:ident { $($name:expr => $func:expr $(, [$($sig:ident),* $(,)?])?;)* }) => {
        op_table!(@build $table, Internal, { $($name => $func $(, [$($sig),*])?;)* });
    };
    (graphics $table:ident { $($name:expr => $func:expr $(, [$($sig:ident),* $(,)?])?;)* }) => {
        op_table!(@build $table, Graphics, { $($name => $func $(, [$($sig),*])?;)* });
    };
    (procset $table:ident { $($name:expr => $func:expr $(, [$($sig:ident),* $(,)?])?;)* }) => {
        op_table!(@build $table, ProcSet, { $($name => $func $(, [$($sig),*])?;)* });
    };
    (server $table:ident { $($name:expr => $func:expr $(, [$($sig:ident),* $(,)?])?;)* }) => {
        op_table!(@build $table, Server, { $($name => $func $(, [$($sig),*])?;)* });
    };
    (@build $table:ident, $visibility:ident, { $($name:expr => $func:expr $(, [$($sig:ident),*])?;)* }) => {
        pub(crate) static $table: &[$crate::ops::OpEntry] = &[
            $($crate::ops::OpEntry {
                name: $name,
                func: $func,
                sig: &[$($($crate::ops::Sig::$sig),*)?],
                visibility: $crate::ops::Visibility::$visibility,
            }),*
        ];
    };
}

pub mod arith;
pub mod array;
pub mod cidinit;
pub mod control;
pub mod dict;
pub mod distiller;
pub mod embedded;
pub mod errors;
pub mod file;
pub mod filter;
pub mod font;
pub mod fontset;
pub mod graphics;
pub mod image;
pub mod output;
pub mod pagedevice;
pub mod pdfmark;
pub mod resource;
pub mod screen;
pub mod show;
pub mod stack;
pub mod status;
pub mod types;
pub mod vm;

// Later groups append here; indices of earlier entries never move.
const MODULES: &[&[OpEntry]] = &[
    stack::OPS,
    arith::OPS,
    dict::OPS,
    control::OPS,
    output::OPS,
    errors::OPS,
    errors::HANDLERS,
    array::OPS,
    types::OPS,
    vm::OPS,
    file::OPS,
    pagedevice::OPS,
    graphics::OPS,
    font::OPS,
    font::PAINT_OPS,
    resource::OPS,
    fontset::OPS,
    cidinit::OPS,
    pdfmark::OPS,
    distiller::OPS,
    status::OPS,
    status::SERVER_OPS,
    screen::OPS,
    graphics::LATER_OPS,
    filter::OPS,
];

/// The complete operator table, built on first use.
pub fn table() -> &'static [OpEntry] {
    static TABLE: OnceLock<Vec<OpEntry>> = OnceLock::new();
    TABLE.get_or_init(|| MODULES.iter().flat_map(|m| m.iter().copied()).collect())
}

/// The index of the operator named `name` with the given visibility.
pub fn find(name: &str, visibility: Visibility) -> Option<u32> {
    table()
        .iter()
        .position(|e| e.visibility == visibility && e.name == name)
        .map(|i| u32::try_from(i).expect("operator table fits in u32"))
}

/// The shared prologue: `stackunderflow` if fewer operands than the
/// signature names, `typecheck` if one has the wrong type. Nothing is
/// popped.
pub(crate) fn check_sig(interp: &Interp, sig: &[Sig]) -> Result<(), VmError> {
    let stack = interp.ostack();
    let Some(base) = stack.len().checked_sub(sig.len()) else {
        return Err(VmError::StackUnderflow);
    };
    for (class, &operand) in sig.iter().zip(&stack[base..]) {
        if !class.accepts(operand) {
            return Err(VmError::TypeCheck);
        }
    }
    Ok(())
}

/// A number as the arithmetic operators see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    Int(i32),
    Real(f32),
}

impl Num {
    pub fn of(object: Object) -> Option<Self> {
        match object.ty() {
            Type::Integer => object.as_i32().map(Num::Int),
            Type::Real => object.as_f32().map(Num::Real),
            _ => None,
        }
    }

    pub fn to_object(self) -> Object {
        match self {
            Num::Int(i) => Object::integer(i),
            Num::Real(r) => Object::real(r),
        }
    }

    pub fn as_f32(self) -> f32 {
        match self {
            Num::Int(i) => i as f32,
            Num::Real(r) => r,
        }
    }

    pub fn is_real(self) -> bool {
        matches!(self, Num::Real(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_are_unique_per_visibility() {
        let mut seen = std::collections::HashSet::new();
        for e in table() {
            assert!(seen.insert((e.name, e.visibility)), "duplicate {}", e.name);
        }
        assert!(find("add", Visibility::Public).is_some());
        assert!(find("add", Visibility::Internal).is_none());
        assert!(find("typecheck", Visibility::Internal).is_some());
        assert!(find("typecheck", Visibility::Public).is_none());
        assert!(find("moveto", Visibility::Graphics).is_some());
        assert!(find("moveto", Visibility::Public).is_none());
        assert!(find("setpagedevice", Visibility::Public).is_some());
        assert!(find("findfont", Visibility::Public).is_some());
        assert!(find("stringwidth", Visibility::Public).is_some());
        assert!(find("show", Visibility::Graphics).is_some());
        assert!(find("show", Visibility::Public).is_none());
        assert!(find("findresource", Visibility::Public).is_some());
        assert!(find("pdfmark", Visibility::Graphics).is_some());
        assert!(find("pdfmark", Visibility::Public).is_none());
        assert!(find("setdistillerparams", Visibility::Public).is_some());
        assert!(find("currentdistillerparams", Visibility::Public).is_some());
        assert!(find("exitserver", Visibility::Server).is_some());
        assert!(find("exitserver", Visibility::Public).is_none());
        assert!(find("framedevice", Visibility::Public).is_some());
        assert!(find("setscreen", Visibility::Public).is_some());
        assert!(find("cexec", Visibility::Public).is_some());
        assert!(find("pathforall", Visibility::Graphics).is_some());
        assert!(find("colorimage", Visibility::Graphics).is_some());
        assert!(find("filter", Visibility::Public).is_some());
    }

    #[test]
    fn signatures_accept_by_class() {
        assert!(Sig::Any.accepts(Object::null()));
        assert!(Sig::Int.accepts(Object::integer(1)));
        assert!(!Sig::Int.accepts(Object::real(1.0)));
        assert!(Sig::Num.accepts(Object::real(1.0)));
        assert!(Sig::Bool.accepts(Object::boolean(true)));
        assert!(!Sig::Dict.accepts(Object::mark()));
        let a = Object::array(crate::Space::Local, crate::Handle(0), 0);
        assert!(Sig::Array.accepts(a));
        assert!(Sig::Array.accepts(Object::packed_array(
            crate::Space::Local,
            crate::Handle(0),
            0
        )));
        assert!(!Sig::String.accepts(a));
        assert!(Sig::Name.accepts(Object::name(crate::Atom(0))));
    }

    #[test]
    fn num_round_trips() {
        assert_eq!(Num::of(Object::integer(3)), Some(Num::Int(3)));
        assert_eq!(Num::of(Object::real(1.5)), Some(Num::Real(1.5)));
        assert_eq!(Num::of(Object::null()), None);
        assert_eq!(Num::Int(3).to_object().as_i32(), Some(3));
        assert_eq!(Num::Real(1.5).to_object().as_f32(), Some(1.5));
        assert_eq!(Num::Int(2).as_f32(), 2.0);
        assert!(Num::Real(0.0).is_real());
        assert!(!Num::Int(0).is_real());
    }
}
