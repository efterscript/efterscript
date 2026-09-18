// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Operand-stack operators (PLRM3 §8.2, "Operand Stack Manipulation").

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Type};

op_table! { OPS {
    "pop" => pop, [Any];
    "dup" => dup, [Any];
    "exch" => exch, [Any, Any];
    "copy" => copy, [Any];
    "index" => index, [Int];
    "roll" => roll, [Int, Int];
    "clear" => clear;
    "count" => count;
    "mark" => mark;
    "cleartomark" => cleartomark;
    "counttomark" => counttomark;
}}

fn pop(i: &mut Interp) -> Result<(), VmError> {
    i.pop()?;
    Ok(())
}

fn dup(i: &mut Interp) -> Result<(), VmError> {
    let top = i.peek(0)?;
    i.push(top)
}

fn exch(i: &mut Interp) -> Result<(), VmError> {
    let a = i.pop()?;
    let b = i.pop()?;
    i.push(a)?;
    i.push(b)
}

fn copy(i: &mut Interp) -> Result<(), VmError> {
    let top = i.peek(0)?;
    match top.ty() {
        Type::Integer => {
            let n =
                usize::try_from(top.as_i32().expect("integer")).map_err(|_| VmError::RangeCheck)?;
            i.pop()?;
            let len = i.ostack.len();
            if n > len {
                return Err(VmError::StackUnderflow);
            }
            for k in len - n..len {
                let object = i.ostack[k];
                i.push(object)?;
            }
            Ok(())
        }
        Type::Array | Type::PackedArray | Type::String | Type::Dict => {
            let source = i.peek(1)?;
            let same = source.ty() == top.ty()
                || matches!(
                    (source.ty(), top.ty()),
                    (
                        Type::Array | Type::PackedArray,
                        Type::Array | Type::PackedArray
                    )
                );
            if !same {
                return Err(VmError::TypeCheck);
            }
            let result = match top.ty() {
                Type::Array | Type::PackedArray => i.mem.array_copy(source, top)?,
                Type::String => i.mem.string_copy(source, top)?,
                _ => i.mem.dict_copy(source, top)?,
            };
            i.pop()?;
            i.pop()?;
            i.push(result)
        }
        _ => Err(VmError::TypeCheck),
    }
}

fn index(i: &mut Interp) -> Result<(), VmError> {
    let n = i.peek(0)?.as_i32().expect("integer");
    let n = usize::try_from(n).map_err(|_| VmError::RangeCheck)?;
    let object = i.peek(n + 1)?;
    i.pop()?;
    i.push(object)
}

fn roll(i: &mut Interp) -> Result<(), VmError> {
    let j = i.peek(0)?.as_i32().expect("integer");
    let n = i.peek(1)?.as_i32().expect("integer");
    let n = usize::try_from(n).map_err(|_| VmError::RangeCheck)?;
    let len = i.ostack.len() - 2;
    if n > len {
        return Err(VmError::StackUnderflow);
    }
    i.pop()?;
    i.pop()?;
    if n > 0 {
        let shift = j.rem_euclid(i32::try_from(n).expect("n fits")) as usize;
        i.ostack[len - n..].rotate_right(shift);
    }
    Ok(())
}

fn clear(i: &mut Interp) -> Result<(), VmError> {
    i.ostack.clear();
    Ok(())
}

fn count(i: &mut Interp) -> Result<(), VmError> {
    let n = i32::try_from(i.ostack.len()).map_err(|_| VmError::LimitCheck)?;
    i.push(Object::integer(n))
}

fn mark(i: &mut Interp) -> Result<(), VmError> {
    i.push(Object::mark())
}

fn find_mark(i: &Interp) -> Result<usize, VmError> {
    i.ostack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)
}

fn cleartomark(i: &mut Interp) -> Result<(), VmError> {
    let at = find_mark(i)?;
    i.ostack.truncate(at);
    Ok(())
}

fn counttomark(i: &mut Interp) -> Result<(), VmError> {
    let at = find_mark(i)?;
    let n = i32::try_from(i.ostack.len() - 1 - at).map_err(|_| VmError::LimitCheck)?;
    i.push(Object::integer(n))
}
