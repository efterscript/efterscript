// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `pdfmark` operator (the pdfmark reference): the objects down to
//! the nearest mark become a kind — the last object, a name — and a list
//! of values the backend pairs and interprets. Like the painting
//! operators it is defined only while a backend is installed, so a job's
//! `/pdfmark where` guard selects `cleartomark` otherwise.

use crate::error::VmError;
use crate::graphics::MarkValue;
use crate::interp::Interp;
use crate::object::{Object, Type};
use crate::ops::array::{bytes, items};

op_table! { graphics OPS {
    "pdfmark" => pdfmark;
}}

/// Arrays and dictionaries nested deeper than this are `limitcheck`; it
/// also bounds the recursion of the conversion.
const MAX_DEPTH: usize = 32;

fn pdfmark(i: &mut Interp) -> Result<(), VmError> {
    let at = i
        .ostack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let objects: Vec<Object> = i.ostack[at + 1..].to_vec();
    let Some((kind, rest)) = objects.split_last() else {
        return Err(VmError::TypeCheck);
    };
    let kind = kind.as_name().ok_or(VmError::TypeCheck)?;
    let kind = i.mem.name_text(kind).to_vec();
    let entries = rest
        .iter()
        .map(|&object| value(i, object, 0))
        .collect::<Result<Vec<_>, _>>()?;
    i.backend()?.pdfmark(&kind, &entries)?;
    i.ostack.truncate(at);
    Ok(())
}

/// The value of one object in a mark; procedures and every type without
/// a PDF counterpart are `typecheck`.
pub(crate) fn value(i: &Interp, object: Object, depth: usize) -> Result<MarkValue, VmError> {
    if depth > MAX_DEPTH {
        return Err(VmError::LimitCheck);
    }
    match object.ty() {
        Type::Name => {
            let atom = object.as_name().expect("a name");
            Ok(MarkValue::Name(i.mem.name_text(atom).to_vec()))
        }
        Type::String => Ok(MarkValue::String(bytes(i, object)?)),
        Type::Integer => Ok(MarkValue::Int(object.as_i32().expect("an integer"))),
        Type::Real => Ok(MarkValue::Real(object.as_f32().expect("a real"))),
        Type::Boolean => Ok(MarkValue::Bool(object.as_bool().expect("a boolean"))),
        Type::Null => Ok(MarkValue::Null),
        Type::Array | Type::PackedArray if !object.is_executable() => {
            let elements = items(i, object)?
                .into_iter()
                .map(|element| value(i, element, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(MarkValue::Array(elements))
        }
        Type::Dict => {
            let mut entries = Vec::new();
            for (key, entry) in i.mem.dict_entries(object)? {
                let key = key.as_name().ok_or(VmError::TypeCheck)?;
                let key = i.mem.name_text(key).to_vec();
                entries.push((key, value(i, entry, depth + 1)?));
            }
            Ok(MarkValue::Dict(entries))
        }
        _ => Err(VmError::TypeCheck),
    }
}
