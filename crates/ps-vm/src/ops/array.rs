// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Array, packed-array, and string operators (PLRM3 §8.2). Every store
//! goes through the checked memory API, so access attributes and the
//! global/local rule hold here as they do for `put`.

use crate::error::VmError;
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Access, Object, Type};

op_table! { OPS {
    "array" => array, [Int];
    "[" => open_bracket;
    "]" => close_bracket;
    "astore" => astore, [Array];
    "aload" => aload, [Array];
    "getinterval" => getinterval, [Any, Int, Int];
    "putinterval" => putinterval, [Any, Int, Any];
    "forall" => forall, [Any, Array];
    "string" => string, [Int];
    "search" => search, [String, String];
    "anchorsearch" => anchorsearch, [String, String];
    "packedarray" => packedarray, [Int];
    "setpacking" => setpacking, [Bool];
    "currentpacking" => currentpacking;
}}

/// Longest array or string `array`, `string`, and `packedarray` create,
/// per PLRM3 Appendix B.
pub const MAX_COMPOSITE_LEN: usize = 65535;

fn count(i: &Interp, n: usize) -> Result<usize, VmError> {
    let n =
        usize::try_from(i.peek(n)?.as_i32().expect("integer")).map_err(|_| VmError::RangeCheck)?;
    if n > MAX_COMPOSITE_LEN {
        return Err(VmError::LimitCheck);
    }
    Ok(n)
}

fn readable(object: Object) -> Result<(), VmError> {
    if object.access().unwrap_or_default() > Access::ReadOnly {
        return Err(VmError::InvalidAccess);
    }
    Ok(())
}

/// The elements of a readable array or packed array.
pub(crate) fn items(i: &Interp, array: Object) -> Result<Vec<Object>, VmError> {
    readable(array)?;
    i.mem
        .array(array)
        .map(<[Object]>::to_vec)
        .ok_or(VmError::InvalidAccess)
}

fn array(i: &mut Interp) -> Result<(), VmError> {
    let n = count(i, 0)?;
    let array = i.mem.alloc_array(vec![Object::null(); n])?;
    i.pop()?;
    i.push(array)
}

fn open_bracket(i: &mut Interp) -> Result<(), VmError> {
    i.push(Object::mark())
}

fn close_bracket(i: &mut Interp) -> Result<(), VmError> {
    let at = i
        .ostack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let items = i.ostack[at + 1..].to_vec();
    let array = i.mem.alloc_array(items)?;
    i.ostack.truncate(at);
    i.push(array)
}

fn astore(i: &mut Interp) -> Result<(), VmError> {
    let array = i.peek(0)?;
    let n = array.length().expect("array") as usize;
    let len = i.ostack.len() - 1;
    if n > len {
        return Err(VmError::StackUnderflow);
    }
    let elements = i.ostack[len - n..len].to_vec();
    i.mem.array_put_items(array, 0, &elements)?;
    i.ostack.truncate(len - n);
    i.push(array)
}

fn aload(i: &mut Interp) -> Result<(), VmError> {
    let array = i.peek(0)?;
    let elements = items(i, array)?;
    if i.ostack.len() + elements.len() > i.limits().operand {
        return Err(VmError::StackOverflow);
    }
    i.pop()?;
    for element in elements {
        i.push(element)?;
    }
    i.push(array)
}

fn getinterval(i: &mut Interp) -> Result<(), VmError> {
    let n = i.peek(0)?.as_i32().expect("integer");
    let index = i.peek(1)?.as_i32().expect("integer");
    let object = i.peek(2)?;
    if !matches!(object.ty(), Type::Array | Type::PackedArray | Type::String) {
        return Err(VmError::TypeCheck);
    }
    readable(object)?;
    let (index, n) = (
        u32::try_from(index).map_err(|_| VmError::RangeCheck)?,
        u32::try_from(n).map_err(|_| VmError::RangeCheck)?,
    );
    let interval = object.with_interval(index, n).ok_or(VmError::RangeCheck)?;
    i.pop()?;
    i.pop()?;
    i.pop()?;
    i.push(interval)
}

fn putinterval(i: &mut Interp) -> Result<(), VmError> {
    let source = i.peek(0)?;
    let index = i.peek(1)?.as_i32().expect("integer");
    let target = i.peek(2)?;
    let index = usize::try_from(index).map_err(|_| VmError::RangeCheck)?;
    match (target.ty(), source.ty()) {
        (Type::Array, Type::Array | Type::PackedArray) => {
            i.mem.array_put_interval(target, index, source)?;
        }
        (Type::String, Type::String) => i.mem.string_put_interval(target, index, source)?,
        _ => return Err(VmError::TypeCheck),
    }
    i.pop()?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn forall(i: &mut Interp) -> Result<(), VmError> {
    let body = i.peek(0)?;
    let container = i.peek(1)?;
    match container.ty() {
        Type::Array | Type::PackedArray | Type::String => readable(container)?,
        Type::Dict => {
            i.mem.dict_len(container)?;
        }
        _ => return Err(VmError::TypeCheck),
    }
    i.pop()?;
    i.pop()?;
    i.push_frame(Frame::Loop(LoopFrame::ForAll {
        body,
        container,
        next: 0,
    }))
}

fn string(i: &mut Interp) -> Result<(), VmError> {
    let n = count(i, 0)?;
    let string = i.mem.alloc_string(vec![0; n]);
    i.pop()?;
    i.push(string)
}

/// The bytes of a readable string.
pub(crate) fn bytes(i: &Interp, string: Object) -> Result<Vec<u8>, VmError> {
    if string.ty() != Type::String {
        return Err(VmError::TypeCheck);
    }
    readable(string)?;
    i.mem
        .string(string)
        .map(<[u8]>::to_vec)
        .ok_or(VmError::InvalidAccess)
}

fn find(haystack: &[u8], needle: &[u8], anchored: bool) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    if anchored {
        return haystack.starts_with(needle).then_some(0);
    }
    (0..=haystack.len() - needle.len()).find(|&at| haystack[at..].starts_with(needle))
}

fn search_with(i: &mut Interp, anchored: bool) -> Result<(), VmError> {
    let seek = i.peek(0)?;
    let string = i.peek(1)?;
    let needle = bytes(i, seek)?;
    let haystack = bytes(i, string)?;
    let found = find(&haystack, &needle, anchored);
    i.pop()?;
    i.pop()?;
    let Some(at) = found else {
        i.push(string)?;
        return i.push(Object::boolean(false));
    };
    let (at, n, total) = (
        u32::try_from(at).expect("within a string"),
        u32::try_from(needle.len()).expect("within a string"),
        u32::try_from(haystack.len()).expect("within a string"),
    );
    let interval = |offset, len| {
        string
            .with_interval(offset, len)
            .expect("within the string")
    };
    i.push(interval(at + n, total - at - n))?;
    i.push(interval(at, n))?;
    if !anchored {
        i.push(interval(0, at))?;
    }
    i.push(Object::boolean(true))
}

fn search(i: &mut Interp) -> Result<(), VmError> {
    search_with(i, false)
}

fn anchorsearch(i: &mut Interp) -> Result<(), VmError> {
    search_with(i, true)
}

fn packedarray(i: &mut Interp) -> Result<(), VmError> {
    let n = count(i, 0)?;
    let len = i.ostack.len() - 1;
    if n > len {
        return Err(VmError::StackUnderflow);
    }
    let elements = i.ostack[len - n..len].to_vec();
    let array = i.mem.alloc_packed_array(elements)?;
    i.ostack.truncate(len - n);
    i.push(array)
}

fn setpacking(i: &mut Interp) -> Result<(), VmError> {
    let packing = i.pop_bool()?;
    i.mem.set_packing(packing);
    Ok(())
}

fn currentpacking(i: &mut Interp) -> Result<(), VmError> {
    let packing = i.mem.current_packing();
    i.push(Object::boolean(packing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_locates_the_first_occurrence() {
        assert_eq!(find(b"abcabc", b"bc", false), Some(1));
        assert_eq!(find(b"abcabc", b"bc", true), None);
        assert_eq!(find(b"abcabc", b"ab", true), Some(0));
        assert_eq!(find(b"abc", b"", false), Some(0));
        assert_eq!(find(b"ab", b"abc", false), None);
        assert_eq!(find(b"abc", b"x", false), None);
    }
}
