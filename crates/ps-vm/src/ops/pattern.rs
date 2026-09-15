// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Tiling patterns (PLRM3 §4.9): the shape check the `Pattern` category
//! and `makepattern` share, the instance `makepattern` makes, the pattern
//! as a colour (`setpattern`, and `setcolor` in a pattern space), and the
//! capture of a cell at the first paint with it.
//!
//! An instance is a read-only copy of the prototype dictionary with an
//! `Implementation` entry holding the instance id; the interpreter keeps
//! the id's value part (`PatternInfo`, whose matrix is the operand
//! matrix concatenated with the CTM at `makepattern`, so pattern space to
//! default user space). The backend is told the instance when it becomes
//! the colour, and asked to capture its cell when a painting operator
//! first uses that colour: the operator's operands stay on the operand
//! stack, the paint procedure runs as a `PatternCell` frame with the
//! dictionary as its operand, and the operator runs again afterwards.

use crate::error::VmError;
use crate::graphics::{Bounds, PatternInfo, SpaceSpec};
use crate::interp::{Frame, Interp, LoopFrame, PatternInstance};
use crate::object::{Access, Object, Type};
use crate::ops::graphics::{drop, is_array, num_at, read_bounds, read_matrix};

op_table! { OPS {
    "makepattern" => makepattern, [Dict, Array];
}}

op_table! { graphics PAINT_OPS {
    "setpattern" => setpattern, [Any];
}}

/// The entry `key` of `dict`; `missing` when there is none.
pub(crate) fn required(
    i: &mut Interp,
    dict: Object,
    key: &str,
    missing: VmError,
) -> Result<Object, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)?.ok_or(missing)
}

/// An integer entry within `range`: `typecheck` for another type,
/// `rangecheck` outside the range.
pub(crate) fn int_in(
    i: &mut Interp,
    dict: Object,
    key: &str,
    range: std::ops::RangeInclusive<i32>,
    missing: VmError,
) -> Result<i32, VmError> {
    let value = required(i, dict, key, missing)?
        .as_i32()
        .ok_or(VmError::TypeCheck)?;
    if !range.contains(&value) {
        return Err(VmError::RangeCheck);
    }
    Ok(value)
}

/// The `PaintProc` entry, which must be a procedure.
pub(crate) fn paint_proc(
    i: &mut Interp,
    dict: Object,
    missing: VmError,
) -> Result<Object, VmError> {
    let proc = required(i, dict, "PaintProc", missing)?;
    if !is_array(proc) || !proc.is_executable() {
        return Err(VmError::TypeCheck);
    }
    Ok(proc)
}

/// The value part of a type 1 pattern dictionary, in pattern space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shape {
    pub bbox: Bounds,
    pub xstep: f32,
    pub ystep: f32,
    pub paint_type: u8,
    pub tiling_type: u8,
}

/// Reads and checks a type 1 pattern dictionary: `PatternType` 1,
/// `PaintType` 1 or 2, `TilingType` 1 to 3, a `BBox` of four numbers
/// enclosing an area, non-zero `XStep` and `YStep`, and a `PaintProc`
/// procedure. A wrongly typed entry is `typecheck`, a value outside its
/// range `rangecheck`, and an absent entry `missing`.
pub(crate) fn shape(i: &mut Interp, dict: Object, missing: VmError) -> Result<Shape, VmError> {
    if dict.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    int_in(i, dict, "PatternType", 1..=1, missing)?;
    let paint_type = int_in(i, dict, "PaintType", 1..=2, missing)? as u8;
    let tiling_type = int_in(i, dict, "TilingType", 1..=3, missing)? as u8;
    let bbox = required(i, dict, "BBox", missing)?;
    let bbox = read_bounds(i, bbox)?;
    let mut steps = [0.0; 2];
    for (slot, key) in steps.iter_mut().zip(["XStep", "YStep"]) {
        let step = required(i, dict, key, missing)?
            .as_number()
            .ok_or(VmError::TypeCheck)?;
        if step == 0.0 || !step.is_finite() {
            return Err(VmError::RangeCheck);
        }
        *slot = step;
    }
    paint_proc(i, dict, missing)?;
    Ok(Shape {
        bbox,
        xstep: steps[0],
        ystep: steps[1],
        paint_type,
        tiling_type,
    })
}

/// Checks that `dict` is a type 1 pattern dictionary; see [`shape`].
pub(crate) fn check_dict(i: &mut Interp, dict: Object, missing: VmError) -> Result<(), VmError> {
    shape(i, dict, missing).map(|_| ())
}

/// `dict matrix makepattern instance`: the instance is a read-only copy
/// of `dict` in local VM (PLRM3 §8.2) with an `Implementation` entry
/// holding the instance id, and its pattern space is `matrix` followed
/// by the CTM in effect; without a graphics backend the CTM is the
/// identity. The prototype is left as it is.
fn makepattern(i: &mut Interp) -> Result<(), VmError> {
    let matrix = read_matrix(i, i.peek(0)?)?;
    let dict = i.peek(1)?;
    let shape = shape(i, dict, VmError::Undefined)?;
    let ctm = i
        .graphics_backend()
        .map_or_else(Default::default, |backend| backend.current_matrix());
    let id = i.next_pattern_id();
    let instance = instance_dict(i, dict, id)?;
    let info = PatternInfo {
        id,
        matrix: matrix.then(ctm),
        bbox: shape.bbox,
        xstep: shape.xstep,
        ystep: shape.ystep,
        paint_type: shape.paint_type,
        tiling_type: shape.tiling_type,
    };
    i.register_pattern(PatternInstance {
        dict: instance,
        info,
    });
    drop(i, 2)?;
    i.push(instance)
}

/// A read-only dictionary in local VM with `source`'s entries and
/// `Implementation` set to `id`.
fn instance_dict(i: &mut Interp, source: Object, id: u64) -> Result<Object, VmError> {
    let entries = i.mem.dict_entries(source)?;
    let id = i32::try_from(id).map_err(|_| VmError::LimitCheck)?;
    let capacity = u32::try_from(entries.len() + 1).map_err(|_| VmError::LimitCheck)?;
    let global = i.mem.current_global();
    i.mem.set_global(false);
    let instance = i.mem.new_dict(capacity);
    i.mem.set_global(global);
    for (key, value) in entries {
        i.mem.dict_put(instance, key, value)?;
    }
    let key = i.intern("Implementation");
    i.mem.dict_put(instance, key, Object::integer(id))?;
    i.mem.dict_set_access(instance, Access::ReadOnly)?;
    Ok(instance)
}

/// The instance a `setpattern` or `setcolor` operand names: a
/// dictionary whose `Implementation` entry is a live instance id and
/// which is that instance's own dictionary. No `Implementation` entry
/// is `undefined` (a prototype was given, as other interpreters
/// answer); an entry that is not an instance of this interpreter's, or
/// another dictionary carrying one, is `typecheck`.
pub(crate) fn instance_of(i: &mut Interp, candidate: Object) -> Result<PatternInstance, VmError> {
    if candidate.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    let key = i.intern("Implementation");
    let id = i.mem.dict_get(candidate, key)?.ok_or(VmError::Undefined)?;
    let instance = id
        .as_i32()
        .and_then(|id| u64::try_from(id).ok())
        .and_then(|id| i.pattern_instance(id))
        .ok_or(VmError::TypeCheck)?;
    if instance.dict.composite_ref() != candidate.composite_ref() {
        return Err(VmError::TypeCheck);
    }
    Ok(instance)
}

/// `[comp… ] instance setpattern`: the equivalent of `setcolorspace` to
/// `[/Pattern current-space]` — unless the current space is already a
/// pattern space, which stays — followed by `setcolor` (PLRM3 §8.2).
fn setpattern(i: &mut Interp) -> Result<(), VmError> {
    if i.in_uncoloured_cell() {
        return Err(VmError::Undefined);
    }
    let instance = instance_of(i, i.peek(0)?)?;
    let backend = i.backend()?;
    let current = backend.current_color_space();
    let space = match current {
        SpaceSpec::Pattern { .. } => current,
        base => {
            let space = SpaceSpec::Pattern {
                base: Some(Box::new(base)),
            };
            backend.set_color_space(&space)?;
            space
        }
    };
    set_instance(i, instance, &space)
}

/// `setcolor` with a pattern instance on top: the operand of a coloured
/// pattern is the instance alone, whatever the base; an uncoloured one
/// takes the base's components under it, and without a base is
/// `rangecheck` (PLRM3 §4.9.2, §8.2 `setpattern`).
pub(crate) fn set_instance(
    i: &mut Interp,
    instance: PatternInstance,
    space: &SpaceSpec,
) -> Result<(), VmError> {
    let count = if instance.info.paint_type == 2 {
        space
            .component_space()
            .map(SpaceSpec::components)
            .ok_or(VmError::RangeCheck)?
    } else {
        0
    };
    let mut components = Vec::with_capacity(count);
    for k in 0..count {
        components.push(num_at(i, count - k)?);
    }
    i.backend()?.set_pattern(&instance.info, &components)?;
    drop(i, count + 1)
}

/// The colour operators are undefined inside an uncoloured cell.
pub(crate) fn colour_allowed(i: &Interp) -> Result<(), VmError> {
    if i.in_uncoloured_cell() {
        Err(VmError::Undefined)
    } else {
        Ok(())
    }
}

/// The instance behind the current colour, when it is a pattern:
/// `invalidaccess` when its dictionary was discarded by `restore`.
fn current_instance(i: &mut Interp) -> Result<Option<PatternInstance>, VmError> {
    let Some(info) = i.graphics_backend().and_then(|b| b.current_pattern()) else {
        return Ok(None);
    };
    let instance = i.pattern_instance(info.id).ok_or(VmError::TypeCheck)?;
    if i.mem.dict(instance.dict).is_none() {
        return Err(VmError::InvalidAccess);
    }
    Ok(Some(instance))
}

/// The instance dictionary `currentcolor` reports above the components,
/// when the current colour is a pattern instance.
pub(crate) fn current_instance_dict(i: &mut Interp) -> Result<Option<Object>, VmError> {
    Ok(current_instance(i)?.map(|instance| instance.dict))
}

/// Called by a painting operator before it paints, with its operands
/// still on the operand stack. When the current colour is a pattern
/// whose cell the backend has not captured for this page, the paint
/// procedure is arranged to run as a `PatternCell` frame and the
/// operator to run again afterwards, and `true` is returned: the
/// operator then returns without painting.
pub(crate) fn capture_cell(i: &mut Interp, operator: &'static str) -> Result<bool, VmError> {
    let Some(instance) = current_instance(i)? else {
        return Ok(false);
    };
    let body = paint_proc(i, instance.dict, VmError::Undefined)?;
    let backend = i.backend()?;
    let depth = backend.gstate_depth();
    if !backend.begin_pattern_cell(&instance.info)? {
        return Ok(false);
    }
    i.push_frame_unchecked(Frame::Loop(LoopFrame::PatternCell {
        body,
        dict: instance.dict,
        depth,
        operator,
        uncoloured: instance.info.paint_type == 2,
        started: false,
    }));
    Ok(true)
}

/// The paint procedure has returned: the capture ends, the graphics
/// state returns to `depth`, and the painting operator runs again over
/// its operands. An error is raised on the operator.
pub(crate) fn finish_cell(i: &mut Interp, depth: usize, operator: &'static str) {
    let command = i.operator(operator).unwrap_or(Object::null());
    let result = match i.backend() {
        Ok(backend) => {
            let ended = backend.end_pattern_cell();
            let restored = backend.grestore_to(depth);
            ended.and(restored)
        }
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => i.push_frame_unchecked(Frame::Object(command)),
        Err(e) => i.raise(e, command),
    }
}

/// A cell frame discarded while its capture was open: the capture is
/// closed and the graphics state returns to what it was before the
/// paint; the operator does not run again.
pub(crate) fn abandon_cell(i: &mut Interp, depth: usize) {
    if let Some(backend) = i.graphics_backend() {
        let _ = backend.end_pattern_cell();
        let _ = backend.grestore_to(depth);
    }
}
