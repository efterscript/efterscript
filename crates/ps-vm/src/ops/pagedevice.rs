// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `setpagedevice` and `currentpagedevice` (PLRM3 §6.1) under the
//! tolerant-acceptance policy: `PageSize` becomes the backend's media box,
//! every other entry is recorded and readable back, and no key is
//! rejected — though a recognised key given a value of the wrong type is
//! `typecheck` (PLRM3 §6.2 gives each its type). The page-device
//! dictionary lives in global VM, so the values a request carries are
//! copied there. These operators work without a graphics backend, so
//! page setup can be tested in a scripting-only VM.

use crate::error::VmError;
use crate::graphics::Bounds;
use crate::interp::Interp;
use crate::object::{Access, Object, Space, Type};
use crate::ops::array::{bytes, items};

op_table! { OPS {
    "setpagedevice" => setpagedevice, [Dict];
    "currentpagedevice" => currentpagedevice;
}}

/// Deeper nesting than this in a request value is `limitcheck`; it also
/// bounds the recursion of the copy.
const MAX_VALUE_DEPTH: usize = 32;

const DEFAULT_PAGE_SIZE: (i32, i32) = (612, 792);

/// What a recognised key's value may be.
#[derive(Clone, Copy)]
enum Accepted {
    Dict,
    Bool,
    Int,
    ArrayOrNull,
}

/// The recognised keys other than `PageSize`, which has its own check.
const TYPED_KEYS: [(&str, Accepted); 11] = [
    ("InputAttributes", Accepted::Dict),
    ("OutputAttributes", Accepted::Dict),
    ("Policies", Accepted::Dict),
    ("Duplex", Accepted::Bool),
    ("Collate", Accepted::Bool),
    ("Tumble", Accepted::Bool),
    ("NumCopies", Accepted::Int),
    ("Orientation", Accepted::Int),
    ("ImagingBBox", Accepted::ArrayOrNull),
    ("HWResolution", Accepted::ArrayOrNull),
    ("PageOffset", Accepted::ArrayOrNull),
];

/// `typecheck` when `key` is recognised and `value` is not of a type it
/// accepts; an unrecognised key accepts anything.
fn check_type(i: &Interp, key: Object, value: Object) -> Result<(), VmError> {
    let Some(atom) = key.as_name() else {
        return Ok(());
    };
    let name = i.mem.name_text(atom);
    let Some((_, accepted)) = TYPED_KEYS.iter().find(|(k, _)| k.as_bytes() == name) else {
        return Ok(());
    };
    let ok = match accepted {
        Accepted::Dict => value.ty() == Type::Dict,
        Accepted::Bool => value.ty() == Type::Boolean,
        Accepted::Int => value.ty() == Type::Integer,
        Accepted::ArrayOrNull => {
            matches!(value.ty(), Type::Array | Type::PackedArray | Type::Null)
        }
    };
    if ok { Ok(()) } else { Err(VmError::TypeCheck) }
}

/// Gives the fresh page-device dictionary its default page size and makes
/// it read-only; later requests merge through the raw storage.
pub(crate) fn seed(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.page_device();
    let (width, height) = DEFAULT_PAGE_SIZE;
    let size = in_global(i, |i| {
        i.mem
            .alloc_array(vec![Object::integer(width), Object::integer(height)])
    })?;
    let key = i.intern("PageSize");
    i.mem.dict_put(dict, key, size)?;
    i.mem.dict_set_access(dict, Access::ReadOnly)
}

/// The media box the current `PageSize` entry describes.
pub(crate) fn media_box(i: &mut Interp) -> Option<Bounds> {
    let key = i.intern("PageSize");
    let size = i.mem.dict(i.page_device())?.get(key)?;
    page_size(i, size).ok()
}

fn page_size(i: &Interp, size: Object) -> Result<Bounds, VmError> {
    if !matches!(size.ty(), Type::Array | Type::PackedArray) {
        return Err(VmError::TypeCheck);
    }
    let items = items(i, size)?;
    let [width, height] = items.as_slice() else {
        return Err(VmError::RangeCheck);
    };
    let width = width.as_number().ok_or(VmError::TypeCheck)?;
    let height = height.as_number().ok_or(VmError::TypeCheck)?;
    Ok(Bounds::new(0.0, 0.0, width, height))
}

/// Runs `f` in global allocation mode, restoring the mode afterwards.
pub(crate) fn in_global<T>(i: &mut Interp, f: impl FnOnce(&mut Interp) -> T) -> T {
    let mode = i.mem.current_global();
    i.mem.set_global(true);
    let result = f(i);
    i.mem.set_global(mode);
    result
}

/// A copy of `object` whose every part lives in global VM; global and
/// simple objects are returned as they are.
pub(crate) fn globalize(i: &mut Interp, object: Object) -> Result<Object, VmError> {
    in_global(i, |i| copy_global(i, object, 0))
}

fn copy_global(i: &mut Interp, object: Object, depth: usize) -> Result<Object, VmError> {
    if object.space() != Some(Space::Local) {
        return Ok(object);
    }
    if depth > MAX_VALUE_DEPTH {
        return Err(VmError::LimitCheck);
    }
    let copy = match object.ty() {
        Type::Array | Type::PackedArray => {
            let mut copied = Vec::new();
            for item in items(i, object)? {
                copied.push(copy_global(i, item, depth + 1)?);
            }
            if object.is_packed() {
                i.mem.alloc_packed_array(copied)?
            } else {
                i.mem.alloc_array(copied)?
            }
        }
        Type::String => {
            let content = bytes(i, object)?;
            i.mem.alloc_string(content)
        }
        Type::Dict => {
            let entries = i.mem.dict_entries(object)?;
            let dict = i
                .mem
                .new_dict(u32::try_from(entries.len()).map_err(|_| VmError::LimitCheck)?);
            for (key, value) in entries {
                let key = copy_global(i, key, depth + 1)?;
                let value = copy_global(i, value, depth + 1)?;
                i.mem.dict_put(dict, key, value)?;
            }
            dict
        }
        _ => return Err(VmError::TypeCheck),
    };
    Ok(copy.with_exec(object.is_executable()))
}

fn setpagedevice(i: &mut Interp) -> Result<(), VmError> {
    let request = i.peek(0)?;
    let entries = i.mem.dict_entries(request)?;
    let dict = i.page_device();
    let page_size_key = i.intern("PageSize");
    let mut media_box = None;
    for (key, value) in &entries {
        check_type(i, *key, *value)?;
    }
    for (key, value) in entries {
        let key = globalize(i, key)?;
        let value = globalize(i, value)?;
        if key.eq(page_size_key) {
            media_box = Some(page_size(i, value)?);
        }
        i.mem
            .dict_mut(dict)
            .expect("page device exists")
            .insert(key, value);
    }
    if let (Some(media_box), Some(backend)) = (media_box, i.graphics_backend()) {
        backend.set_media_box(media_box)?;
    }
    i.pop()?;
    Ok(())
}

fn currentpagedevice(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.page_device();
    i.push(dict)
}
