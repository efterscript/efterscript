// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Virtual-memory operators (PLRM3 §3.7, §8.2). `save` performs the
//! implicit graphics save and records the depth to return to; `restore`
//! pops the graphics-state stack back to it.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Space, Type};

op_table! { OPS {
    "save" => save;
    "restore" => restore, [Any];
    "setglobal" => setglobal, [Bool];
    "currentglobal" => currentglobal;
    "vmstatus" => vmstatus;
    "gcheck" => gcheck, [Any];
}}

/// What `vmstatus` reports as the maximum, until memory is metered.
pub(crate) const VM_MAXIMUM: i32 = 1 << 30;

/// At the outermost level of an unencapsulated job the save covers
/// global VM too (PLRM3 §3.7.7).
fn save(i: &mut Interp) -> Result<(), VmError> {
    let global = i.save_covers_global();
    let save = i.vm_save(global)?;
    i.push(save)
}

fn restore(i: &mut Interp) -> Result<(), VmError> {
    let save = i.peek(0)?;
    if save.ty() != Type::Save {
        return Err(VmError::TypeCheck);
    }
    // The operand itself is not an object `restore` can outlive.
    i.pop()?;
    if let Err(e) = i.vm_restore(save) {
        i.push(save)?;
        return Err(e);
    }
    Ok(())
}

fn setglobal(i: &mut Interp) -> Result<(), VmError> {
    let global = i.pop_bool()?;
    i.mem.set_global(global);
    Ok(())
}

fn currentglobal(i: &mut Interp) -> Result<(), VmError> {
    let global = i.mem.current_global();
    i.push(Object::boolean(global))
}

fn vmstatus(i: &mut Interp) -> Result<(), VmError> {
    let level = i32::try_from(i.mem.save_depth()).map_err(|_| VmError::LimitCheck)?;
    let slots = i.mem.arena(Space::Local).slot_count() + i.mem.arena(Space::Global).slot_count();
    let used = i32::try_from(slots).unwrap_or(i32::MAX);
    i.push(Object::integer(level))?;
    i.push(Object::integer(used))?;
    i.push(Object::integer(VM_MAXIMUM))
}

fn gcheck(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    let global = object.space() != Some(Space::Local);
    i.push(Object::boolean(global))
}
