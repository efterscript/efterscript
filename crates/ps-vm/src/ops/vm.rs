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
const VM_MAXIMUM: i32 = 1 << 30;

fn save(i: &mut Interp) -> Result<(), VmError> {
    let depth = match i.graphics_backend() {
        Some(backend) => {
            let depth = backend.gstate_depth();
            backend.gsave()?;
            Some(depth)
        }
        None => None,
    };
    let save = match i.mem.save(depth.unwrap_or(0)) {
        Ok(save) => save,
        Err(e) => {
            if depth.is_some() {
                let _ = i.backend()?.grestore();
            }
            return Err(e);
        }
    };
    i.push_gstate_floor(depth.map_or(0, |d| d + 1));
    i.push(save)
}

fn restore(i: &mut Interp) -> Result<(), VmError> {
    let save = i.peek(0)?;
    if save.ty() != Type::Save {
        return Err(VmError::TypeCheck);
    }
    let references = i.exec_references();
    let (ostack, dstack) = (i.ostack.clone(), i.dstack.clone());
    let depth = i.mem.restore(save, &[&ostack, &dstack, &references])?;
    i.truncate_gstate_floors();
    if let Some(backend) = i.graphics_backend() {
        backend.grestore_to(depth)?;
    }
    i.pop()?;
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
