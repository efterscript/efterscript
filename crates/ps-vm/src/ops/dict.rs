// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Dictionary and dictionary-stack operators (PLRM3 §8.2), the polymorphic
//! `get`, `put`, and `length`, and `bind` (PLRM3 §3.11).

use std::collections::HashSet;

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Access, Object, Type};

op_table! { OPS {
    "dict" => dict, [Int];
    "begin" => begin, [Dict];
    "end" => end;
    "def" => def, [Any, Any];
    "load" => load, [Any];
    "store" => store, [Any, Any];
    "known" => known, [Dict, Any];
    "where" => where_, [Any];
    "get" => get, [Any, Any];
    "put" => put, [Any, Any, Any];
    "undef" => undef, [Dict, Any];
    "length" => length, [Any];
    "maxlength" => maxlength, [Dict];
    "currentdict" => currentdict;
    "countdictstack" => countdictstack;
    "dictstack" => dictstack, [Array];
    "cleardictstack" => cleardictstack;
    "<<" => dict_start;
    ">>" => dict_end;
    "bind" => bind, [Array];
}}

fn dict(i: &mut Interp) -> Result<(), VmError> {
    let n = i.pop_int()?;
    let n = u32::try_from(n).map_err(|_| VmError::RangeCheck)?;
    let dict = i.mem.new_dict(n);
    i.push(dict)
}

fn begin(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.peek(0)?;
    i.push_dict(dict)?;
    i.pop()?;
    Ok(())
}

fn end(i: &mut Interp) -> Result<(), VmError> {
    i.end_dict()?;
    Ok(())
}

fn def(i: &mut Interp) -> Result<(), VmError> {
    let value = i.peek(0)?;
    let key = i.peek(1)?;
    let dict = i.current_dict();
    i.mem.dict_put(dict, key, value)?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn load(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let key = i.mem.dict_key(key)?;
    let value = i.lookup(key).ok_or(VmError::Undefined)?;
    i.pop()?;
    i.push(value)
}

fn store(i: &mut Interp) -> Result<(), VmError> {
    let value = i.peek(0)?;
    let key = i.peek(1)?;
    let key = i.mem.dict_key(key)?;
    let dict = i.find_dict(key).unwrap_or_else(|| i.current_dict());
    i.mem.dict_put(dict, key, value)?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn known(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let dict = i.peek(1)?;
    let found = i.mem.dict_known(dict, key)?;
    i.pop()?;
    i.pop()?;
    i.push(Object::boolean(found))
}

fn where_(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let key = i.mem.dict_key(key)?;
    let found = i.find_dict(key);
    i.pop()?;
    match found {
        Some(dict) => {
            i.push(dict)?;
            i.push(Object::boolean(true))
        }
        None => i.push(Object::boolean(false)),
    }
}

fn index_of(key: Object) -> Result<usize, VmError> {
    let n = key.as_i32().ok_or(VmError::TypeCheck)?;
    usize::try_from(n).map_err(|_| VmError::RangeCheck)
}

fn get(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let container = i.peek(1)?;
    let value = match container.ty() {
        Type::Dict => i.mem.dict_get(container, key)?.ok_or(VmError::Undefined)?,
        Type::Array | Type::PackedArray => i.mem.array_get(container, index_of(key)?)?,
        Type::String => {
            let byte = i.mem.string_get(container, index_of(key)?)?;
            Object::integer(i32::from(byte))
        }
        _ => return Err(VmError::TypeCheck),
    };
    i.pop()?;
    i.pop()?;
    i.push(value)
}

fn put(i: &mut Interp) -> Result<(), VmError> {
    let value = i.peek(0)?;
    let key = i.peek(1)?;
    let container = i.peek(2)?;
    match container.ty() {
        Type::Dict => i.mem.dict_put(container, key, value)?,
        Type::Array | Type::PackedArray => i.mem.array_put(container, index_of(key)?, value)?,
        Type::String => {
            let byte = value.as_i32().ok_or(VmError::TypeCheck)?;
            let byte = u8::try_from(byte).map_err(|_| VmError::RangeCheck)?;
            i.mem.string_put(container, index_of(key)?, byte)?;
        }
        _ => return Err(VmError::TypeCheck),
    }
    i.pop()?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn undef(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let dict = i.peek(1)?;
    i.mem.dict_undef(dict, key)?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn length(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(0)?;
    let n = match object.ty() {
        Type::Dict => i.mem.dict_len(object)?,
        Type::Array | Type::PackedArray | Type::String => {
            object.length().expect("composite") as usize
        }
        Type::Name => i.mem.name_text(object.as_name().expect("name")).len(),
        _ => return Err(VmError::TypeCheck),
    };
    let n = i32::try_from(n).map_err(|_| VmError::LimitCheck)?;
    i.pop()?;
    i.push(Object::integer(n))
}

fn maxlength(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.peek(0)?;
    let n = i.mem.dict_maxlength(dict)?;
    let n = i32::try_from(n).map_err(|_| VmError::LimitCheck)?;
    i.pop()?;
    i.push(Object::integer(n))
}

fn currentdict(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.current_dict();
    i.push(dict)
}

fn countdictstack(i: &mut Interp) -> Result<(), VmError> {
    let n = i32::try_from(i.dstack.len()).map_err(|_| VmError::LimitCheck)?;
    i.push(Object::integer(n))
}

fn dictstack(i: &mut Interp) -> Result<(), VmError> {
    let array = i.peek(0)?;
    let stack = i.dstack.clone();
    i.mem.array_put_items(array, 0, &stack)?;
    let filled = array
        .with_interval(0, u32::try_from(stack.len()).expect("checked by put"))
        .expect("checked by put");
    i.pop()?;
    i.push(filled)
}

fn cleardictstack(i: &mut Interp) -> Result<(), VmError> {
    let floor = i.dstack_floor();
    i.dstack.truncate(floor);
    Ok(())
}

fn dict_start(i: &mut Interp) -> Result<(), VmError> {
    i.push(Object::mark())
}

fn dict_end(i: &mut Interp) -> Result<(), VmError> {
    let at = i
        .ostack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let items = &i.ostack[at + 1..];
    if !items.len().is_multiple_of(2) {
        return Err(VmError::RangeCheck);
    }
    let pairs: Vec<(Object, Object)> = items.chunks(2).map(|p| (p[0], p[1])).collect();
    let dict = i
        .mem
        .new_dict(u32::try_from(pairs.len()).map_err(|_| VmError::LimitCheck)?);
    for (key, value) in pairs {
        i.mem.dict_put(dict, key, value)?;
    }
    i.ostack.truncate(at);
    i.push(dict)
}

// Names resolving to operators are replaced, in the procedure and in every
// executable array nested in it; a read-only procedure is left as it is.
// The walk uses its own work list, so nesting depth never touches the host
// stack, and a visited set stops self-containing procedures from looping.
fn bind(i: &mut Interp) -> Result<(), VmError> {
    let root = i.peek(0)?;
    let mut pending = vec![root];
    let mut visited = HashSet::new();
    while let Some(array) = pending.pop() {
        let Some(id) = array.composite_ref() else {
            continue;
        };
        if !visited.insert(id) {
            continue;
        }
        let Some(items) = i.mem.array(array).map(<[Object]>::to_vec) else {
            continue;
        };
        let writable = array.access().unwrap_or_default() == Access::Unlimited;
        for (index, item) in items.into_iter().enumerate() {
            if !item.is_executable() {
                continue;
            }
            match item.ty() {
                Type::Name => {
                    if writable
                        && let Some(value) = i.lookup(item)
                        && value.ty() == Type::Operator
                    {
                        i.mem.array_put(array, index, value)?;
                    }
                }
                Type::Array | Type::PackedArray => pending.push(item),
                _ => {}
            }
        }
    }
    Ok(())
}
