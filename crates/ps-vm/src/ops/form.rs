// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Forms (PLRM3 §4.7): the shape check the `Form` category and
//! `execform` share, and `execform` itself. A form is identified by its
//! dictionary: the backend captures the body the first time the
//! dictionary is executed on a page and places the captured body for
//! every execution, the first included. The body runs as a `FormBody`
//! frame with the dictionary as its operand, inside a saved graphics
//! state the backend prepares (the form space as the CTM, the clip cut
//! to `BBox`, an empty path); the operand is consumed before the body
//! runs and the placement follows the body's return.

use crate::error::VmError;
use crate::graphics::{Bounds, FormInfo, Matrix};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Access, Object, Type};
use crate::ops::graphics::{read_bounds, read_matrix};
use crate::ops::pattern::{int_in, paint_proc, required};

op_table! { graphics OPS {
    "execform" => execform, [Dict];
}}

/// The value part of a type 1 form dictionary, with its procedure.
struct Shape {
    bbox: Bounds,
    matrix: Matrix,
    body: Object,
}

/// Reads and checks a type 1 form dictionary: `FormType` 1, a `BBox` of
/// four numbers enclosing an area, a `Matrix` of six numbers, and a
/// `PaintProc` procedure. A wrongly typed entry is `typecheck`, a value
/// outside its range `rangecheck`, and an absent entry `missing`.
fn shape(i: &mut Interp, dict: Object, missing: VmError) -> Result<Shape, VmError> {
    if dict.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    int_in(i, dict, "FormType", 1..=1, missing)?;
    let bbox = required(i, dict, "BBox", missing)?;
    let bbox = read_bounds(i, bbox)?;
    let matrix = required(i, dict, "Matrix", missing)?;
    // A matrix of the wrong length is a shape error here, not a range
    // error as `read_matrix` reports it for matrix operands.
    let matrix = read_matrix(i, matrix).map_err(|e| match e {
        VmError::RangeCheck => VmError::TypeCheck,
        other => other,
    })?;
    let body = paint_proc(i, dict, missing)?;
    Ok(Shape { bbox, matrix, body })
}

/// Checks that `dict` is a type 1 form dictionary; see [`shape`].
pub(crate) fn check_dict(i: &mut Interp, dict: Object, missing: VmError) -> Result<(), VmError> {
    shape(i, dict, missing).map(|_| ())
}

/// The identity of a form: its dictionary's storage, which `restore`
/// never reissues.
fn form_id(dict: Object) -> Option<u64> {
    let reference = dict.composite_ref()?;
    let space = match reference.space {
        crate::object::Space::Local => 0u64,
        crate::object::Space::Global => 1u64 << 32,
    };
    Some(space | u64::from(reference.handle.0))
}

/// `form execform`: validates the dictionary, gives it an
/// `Implementation` entry and makes it read-only (PLRM3 §8.2, whatever
/// its access was), and paints it: the body is captured when the
/// backend does not hold it yet, and the form is placed in every case.
fn execform(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.peek(0)?;
    let Shape { bbox, matrix, body } = shape(i, dict, VmError::Undefined)?;
    let id = form_id(dict).ok_or(VmError::TypeCheck)?;
    mark_executed(i, dict)?;
    let backend = i.backend()?;
    let info = FormInfo {
        id,
        bbox,
        matrix: matrix.then(backend.current_matrix()),
    };
    let depth = backend.gstate_depth();
    let captured = backend.begin_form(&info)?;
    i.align_vm_gstates();
    i.pop()?;
    if captured {
        i.push_frame_unchecked(Frame::Loop(LoopFrame::FormBody {
            body,
            dict,
            depth,
            info,
            started: false,
        }));
        Ok(())
    } else {
        i.backend()?.place_form(&info)
    }
}

/// The alterations `execform` makes to its operand: an `Implementation`
/// entry (the form's id, unused here) written past the dictionary's
/// access, and read-only access from then on.
fn mark_executed(i: &mut Interp, dict: Object) -> Result<(), VmError> {
    let key = i.intern("Implementation");
    let known = i
        .mem
        .dict(dict)
        .ok_or(VmError::InvalidAccess)?
        .contains(key);
    if !known {
        let id = i32::try_from(form_id(dict).unwrap_or(0) & 0x7FFF_FFFF).unwrap_or(0);
        i.mem
            .dict_mut(dict)
            .ok_or(VmError::InvalidAccess)?
            .insert(key, Object::integer(id));
    }
    if i.mem.dict_access(dict)? < Access::ReadOnly {
        i.mem.dict_set_access(dict, Access::ReadOnly)?;
    }
    Ok(())
}

/// The body has returned: the capture ends, the graphics state returns
/// to `depth`, and the form is placed. An error is raised on `execform`.
pub(crate) fn finish_body(i: &mut Interp, depth: usize, info: &FormInfo) {
    let ended = i.backend().and_then(|backend| backend.end_form());
    let restored = i.grestore_to(depth);
    let result = ended
        .and(restored)
        .and_then(|()| i.backend()?.place_form(info));
    if let Err(e) = result {
        let command = i.operator("execform").unwrap_or(Object::null());
        i.raise(e, command);
    }
}

/// A body frame discarded while its capture was open: the capture is
/// closed and the graphics state returns to what it was before the
/// form; nothing is placed.
pub(crate) fn abandon_body(i: &mut Interp, depth: usize) {
    if i.has_graphics_backend() {
        let _ = i.backend().and_then(|backend| backend.end_form());
        let _ = i.grestore_to(depth);
    }
}
