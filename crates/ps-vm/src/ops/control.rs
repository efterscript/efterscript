// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Control operators (PLRM3 §8.2). Every one that runs a procedure pushes a
//! frame and returns; none re-enters the execution loop.

use crate::error::VmError;
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::Object;
use crate::ops::Num;

op_table! { OPS {
    "exec" => exec, [Any];
    "if" => if_, [Bool, Array];
    "ifelse" => ifelse, [Bool, Array, Array];
    "for" => for_, [Num, Num, Num, Array];
    "repeat" => repeat, [Int, Array];
    "loop" => loop_, [Array];
    "exit" => exit;
    "stop" => stop;
    "stopped" => stopped, [Any];
    "countexecstack" => countexecstack;
    "execstack" => execstack, [Array];
    "quit" => quit;
}}

fn exec(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    i.exec_indirect(object)
}

fn if_(i: &mut Interp) -> Result<(), VmError> {
    let body = i.pop()?;
    if i.pop_bool()? {
        i.push_proc(body)
    } else {
        Ok(())
    }
}

fn ifelse(i: &mut Interp) -> Result<(), VmError> {
    let otherwise = i.pop()?;
    let body = i.pop()?;
    if i.pop_bool()? {
        i.push_proc(body)
    } else {
        i.push_proc(otherwise)
    }
}

// Any real control operand makes the whole loop real.
fn for_(i: &mut Interp) -> Result<(), VmError> {
    let body = i.pop()?;
    let limit = i.pop_num()?;
    let increment = i.pop_num()?;
    let initial = i.pop_num()?;
    let real = initial.is_real() || increment.is_real() || limit.is_real();
    let widen = |n: Num| if real { Num::Real(n.as_f32()) } else { n };
    i.push_frame(Frame::Loop(LoopFrame::For {
        body,
        current: widen(initial),
        increment: widen(increment),
        limit: widen(limit),
        done: false,
    }))
}

fn repeat(i: &mut Interp) -> Result<(), VmError> {
    let body = i.peek(0)?;
    let count = i.peek(1)?.as_i32().expect("integer");
    if count < 0 {
        return Err(VmError::RangeCheck);
    }
    i.pop()?;
    i.pop()?;
    if count == 0 {
        return Ok(());
    }
    i.push_frame(Frame::Loop(LoopFrame::Repeat {
        body,
        remaining: count,
    }))
}

fn loop_(i: &mut Interp) -> Result<(), VmError> {
    let body = i.pop()?;
    i.push_frame(Frame::Loop(LoopFrame::Loop { body }))
}

fn exit(i: &mut Interp) -> Result<(), VmError> {
    i.exit()
}

fn stop(i: &mut Interp) -> Result<(), VmError> {
    i.stop();
    Ok(())
}

fn stopped(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    i.push_frame(Frame::Stopped)?;
    i.exec_indirect(object)
}

fn countexecstack(i: &mut Interp) -> Result<(), VmError> {
    let n = i32::try_from(i.exec_objects().len()).map_err(|_| VmError::LimitCheck)?;
    i.push(Object::integer(n))
}

fn execstack(i: &mut Interp) -> Result<(), VmError> {
    let array = i.peek(0)?;
    let objects = i.exec_objects();
    i.mem.array_put_items(array, 0, &objects)?;
    let filled = array
        .with_interval(0, u32::try_from(objects.len()).expect("checked by put"))
        .expect("checked by put");
    i.pop()?;
    i.push(filled)
}

fn quit(i: &mut Interp) -> Result<(), VmError> {
    i.quit();
    Ok(())
}
