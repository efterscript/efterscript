// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `setdistillerparams` and `currentdistillerparams` (the parameters
//! reference) under the page device's tolerant policy: a recognised key
//! given a value of the wrong type is `typecheck`, checked over the whole
//! request before anything changes; every key, recognised or not, is
//! recorded and readable back; and each request reaches the graphics
//! backend as values, from where the writer applies what it honours. The
//! dictionary lives in global VM and is not subject to `restore`, like the
//! page device. Both operators work without a backend, so a job's guard
//! finds them defined either way.

use crate::error::VmError;
use crate::graphics::MarkValue;
use crate::interp::Interp;
use crate::object::{Object, Type};
use crate::ops::pagedevice::{globalize, in_global};
use crate::ops::pdfmark::value;

op_table! { OPS {
    "setdistillerparams" => setdistillerparams, [Dict];
    "currentdistillerparams" => currentdistillerparams;
}}

/// Deeper nesting than this in a seeded value is `limitcheck`; it also
/// bounds the recursion of the conversion.
const MAX_VALUE_DEPTH: usize = 32;

/// What a recognised key's value may be.
#[derive(Clone, Copy)]
enum Accepted {
    Bool,
    Number,
    Name,
}

/// The keys whose type is checked; every other key accepts anything.
const TYPED_KEYS: [(&str, Accepted); 15] = [
    ("CompressPages", Accepted::Bool),
    ("EmbedAllFonts", Accepted::Bool),
    ("SubsetFonts", Accepted::Bool),
    ("DownsampleColorImages", Accepted::Bool),
    ("DownsampleGrayImages", Accepted::Bool),
    ("DownsampleMonoImages", Accepted::Bool),
    ("CompatibilityLevel", Accepted::Number),
    ("ColorImageResolution", Accepted::Number),
    ("GrayImageResolution", Accepted::Number),
    ("MonoImageResolution", Accepted::Number),
    ("ColorImageDownsampleType", Accepted::Name),
    ("GrayImageDownsampleType", Accepted::Name),
    ("MonoImageDownsampleType", Accepted::Name),
    ("ColorConversionStrategy", Accepted::Name),
    ("AutoRotatePages", Accepted::Name),
];

/// The values the dictionary starts with: the writer's own defaults for
/// the keys it honours, so a job reading a parameter it never set finds
/// the value in force. An embedder with other defaults reseeds through
/// [`Interp::set_distiller_params`].
pub fn default_distiller_params() -> Vec<(Vec<u8>, MarkValue)> {
    let name = |text: &str| MarkValue::Name(text.as_bytes().to_vec());
    [
        ("CompressPages", MarkValue::Bool(true)),
        ("EmbedAllFonts", MarkValue::Bool(false)),
        ("SubsetFonts", MarkValue::Bool(true)),
        ("CompatibilityLevel", MarkValue::Real(1.7)),
        ("DownsampleColorImages", MarkValue::Bool(false)),
        ("DownsampleGrayImages", MarkValue::Bool(false)),
        ("DownsampleMonoImages", MarkValue::Bool(false)),
        ("ColorImageResolution", MarkValue::Int(150)),
        ("GrayImageResolution", MarkValue::Int(150)),
        ("MonoImageResolution", MarkValue::Int(300)),
        ("ColorImageDownsampleType", name("Average")),
        ("GrayImageDownsampleType", name("Average")),
        ("MonoImageDownsampleType", name("Average")),
        ("ColorConversionStrategy", name("LeaveColorUnchanged")),
        ("AutoRotatePages", name("None")),
    ]
    .into_iter()
    .map(|(key, value)| (key.as_bytes().to_vec(), value))
    .collect()
}

/// `typecheck` when `key` is recognised and `value` is not of a type it
/// accepts.
fn check_type(i: &Interp, key: Object, value: Object) -> Result<(), VmError> {
    let Some(atom) = key.as_name() else {
        return Ok(());
    };
    let name = i.mem.name_text(atom);
    let Some((_, accepted)) = TYPED_KEYS.iter().find(|(k, _)| k.as_bytes() == name) else {
        return Ok(());
    };
    let ok = match accepted {
        Accepted::Bool => value.ty() == Type::Boolean,
        Accepted::Number => value.is_number(),
        Accepted::Name => value.ty() == Type::Name,
    };
    if ok { Ok(()) } else { Err(VmError::TypeCheck) }
}

/// Fills the fresh dictionary with the defaults.
pub(crate) fn seed(i: &mut Interp) -> Result<(), VmError> {
    put_values(i, &default_distiller_params())
}

/// Stores `entries` in the dictionary, in global VM, without telling
/// the backend; what an embedder seeding its own defaults needs.
pub(crate) fn put_values(i: &mut Interp, entries: &[(Vec<u8>, MarkValue)]) -> Result<(), VmError> {
    let dict = i.distiller_params();
    for (key, value) in entries {
        let key = i.mem.intern(key).map_err(|_| VmError::LimitCheck)?;
        let value = in_global(i, |i| object(i, value, 0))?;
        i.mem
            .dict_mut(dict)
            .expect("the parameter dictionary exists")
            .insert(key, value);
    }
    Ok(())
}

/// The object a value becomes, allocated in the current VM.
fn object(i: &mut Interp, value: &MarkValue, depth: usize) -> Result<Object, VmError> {
    if depth > MAX_VALUE_DEPTH {
        return Err(VmError::LimitCheck);
    }
    Ok(match value {
        MarkValue::Name(name) => i.mem.intern(name).map_err(|_| VmError::LimitCheck)?,
        MarkValue::String(bytes) => i.mem.alloc_string(bytes.clone()),
        MarkValue::Int(v) => Object::integer(*v),
        MarkValue::Real(v) => Object::real(*v),
        MarkValue::Bool(v) => Object::boolean(*v),
        MarkValue::Null => Object::null(),
        MarkValue::Array(items) => {
            let items = items
                .iter()
                .map(|item| object(i, item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            i.mem.alloc_array(items)?
        }
        MarkValue::Dict(entries) => {
            let dict = i
                .mem
                .new_dict(u32::try_from(entries.len()).map_err(|_| VmError::LimitCheck)?);
            for (key, entry) in entries {
                let key = i.mem.intern(key).map_err(|_| VmError::LimitCheck)?;
                let entry = object(i, entry, depth + 1)?;
                i.mem.dict_put(dict, key, entry)?;
            }
            dict
        }
    })
}

fn setdistillerparams(i: &mut Interp) -> Result<(), VmError> {
    let request = i.peek(0)?;
    let entries = i.mem.dict_entries(request)?;
    for (key, value) in &entries {
        check_type(i, *key, *value)?;
    }
    // Only name-keyed entries with a value the boundary can carry reach
    // the backend; the rest are still recorded for reading back.
    let values: Vec<(Vec<u8>, MarkValue)> = entries
        .iter()
        .filter_map(|(key, entry)| {
            let name = i.mem.name_text(key.as_name()?).to_vec();
            Some((name, value(i, *entry, 0).ok()?))
        })
        .collect();
    let dict = i.distiller_params();
    for (key, entry) in entries {
        let key = globalize(i, key)?;
        let entry = globalize(i, entry)?;
        i.mem
            .dict_mut(dict)
            .expect("the parameter dictionary exists")
            .insert(key, entry);
    }
    if let Some(backend) = i.graphics_backend() {
        backend.set_distiller_params(&values)?;
    }
    i.pop()?;
    Ok(())
}

/// A fresh copy of the dictionary each time, as the reference describes,
/// so a job may edit the copy and hand it back.
fn currentdistillerparams(i: &mut Interp) -> Result<(), VmError> {
    let entries = i.mem.dict_entries(i.distiller_params())?;
    let copy = i
        .mem
        .new_dict(u32::try_from(entries.len().max(1)).map_err(|_| VmError::LimitCheck)?);
    for (key, value) in entries {
        i.mem.dict_put(copy, key, value)?;
    }
    i.push(copy)
}
