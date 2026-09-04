// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Program snapshots of the fonts a job defines: a Type 1 dictionary's
//! `Private/lenIV`, `Private/Subrs`, and `CharStrings`, a Type 42
//! dictionary's `sfnts` and `CharStrings`, or a `CIDFontType 2`
//! dictionary's `sfnts` and `CIDMap`, read once into an immutable
//! `ps_fonts::Program`. The dictionary entries are read as the VM sees
//! them, without the access checks operators apply: a font's `Private`
//! is `noaccess` and its charstrings are `noaccess` strings by design.
//! A Type 1 snapshot also carries what regenerating the program needs
//! (`FontBBox`, `PaintType`, and the `FontInfo` and `Private` entries),
//! the entries printed here in their source form since the font crate
//! has no printer of its own.

use std::collections::BTreeMap;

use ps_fonts::truetype::CidMap;
use ps_fonts::type1::Type1Dict;
use ps_fonts::{Program, TrueTypeProgram, Type1Program};

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Access, Object, Type};
use crate::ops::font::entry;
use crate::ops::output::source;

/// The program behind `dict`; `invalidfont` when the dictionary has no
/// usable program.
pub(crate) fn snapshot(i: &mut Interp, dict: Object) -> Result<Program, VmError> {
    if entry(i, dict, "CIDFontType")?.and_then(Object::as_i32) == Some(2) {
        return cidfont_type2(i, dict);
    }
    match entry(i, dict, "FontType")?.and_then(Object::as_i32) {
        Some(1) => type1(i, dict),
        Some(42) => type42(i, dict),
        // A FontType 2 dictionary carries no program of its own: the one
        // `StartData` built has its program cached before this is asked.
        _ => Err(VmError::InvalidFont),
    }
}

fn get(i: &mut Interp, dict: Object, key: &str) -> Option<Object> {
    let key = i.intern(key);
    i.mem.dict(dict)?.get(key)
}

fn get_dict(i: &mut Interp, dict: Object, key: &str) -> Option<Object> {
    get(i, dict, key).filter(|o| o.ty() == Type::Dict)
}

fn string(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    if object.ty() != Type::String {
        return Err(VmError::InvalidFont);
    }
    i.mem
        .string(object)
        .map(<[u8]>::to_vec)
        .ok_or(VmError::InvalidFont)
}

/// The entries of `dict` whose keys are names, as `(name, value)`.
fn named_entries(i: &Interp, dict: Object) -> Result<Vec<(Vec<u8>, Object)>, VmError> {
    let dict = i.mem.dict(dict).ok_or(VmError::InvalidFont)?;
    Ok(dict
        .iter()
        .filter_map(|(key, value)| key.as_name().map(|atom| (atom, value)))
        .map(|(atom, value)| (i.mem.name_text(atom).to_vec(), value))
        .collect())
}

/// Whether `source` prints `object` in a form that reads back: numbers,
/// booleans, names, and readable strings and arrays.
fn printable(object: Object) -> bool {
    match object.ty() {
        Type::Integer | Type::Real | Type::Boolean | Type::Name => true,
        Type::String | Type::Array | Type::PackedArray => {
            object.access().unwrap_or_default() <= Access::ReadOnly
        }
        _ => false,
    }
}

/// The printable entries of `dict` as `(key, source text)`, `skip`ped
/// keys left out; nothing for a missing dictionary.
fn printed_entries(
    i: &mut Interp,
    dict: Option<Object>,
    skip: &[&[u8]],
) -> Vec<(Vec<u8>, Vec<u8>)> {
    let Some(dict) = dict else {
        return Vec::new();
    };
    named_entries(i, dict)
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, value)| !skip.contains(&key.as_slice()) && printable(*value))
        .map(|(key, value)| (key, source(i, value)))
        .collect()
}

fn numbers(i: &Interp, array: Option<Object>) -> Vec<f32> {
    array
        .and_then(|a| i.mem.array(a))
        .map(|items| items.iter().filter_map(|o| o.as_number()).collect())
        .unwrap_or_default()
}

fn type1_dict(i: &mut Interp, dict: Object, private: Option<Object>) -> Type1Dict {
    let bbox = get(i, dict, "FontBBox");
    let font_bbox = match numbers(i, bbox).as_slice() {
        &[llx, lly, urx, ury] => [llx, lly, urx, ury],
        _ => [0.0; 4],
    };
    let paint_type = get(i, dict, "PaintType")
        .and_then(Object::as_i32)
        .unwrap_or(0);
    let font_info = get_dict(i, dict, "FontInfo");
    let font_info = printed_entries(i, font_info, &[]);
    let private = printed_entries(i, private, &[b"Subrs", b"lenIV"]);
    Type1Dict {
        font_bbox,
        paint_type,
        font_info,
        private,
    }
}

fn type1(i: &mut Interp, dict: Object) -> Result<Program, VmError> {
    let charstrings = get_dict(i, dict, "CharStrings").ok_or(VmError::InvalidFont)?;
    let private = get_dict(i, dict, "Private");
    let (len_iv, subrs) = match private {
        Some(private) => {
            let len_iv = get(i, private, "lenIV")
                .and_then(Object::as_i32)
                .unwrap_or(4);
            let subrs = match get(i, private, "Subrs") {
                Some(array) if matches!(array.ty(), Type::Array | Type::PackedArray) => {
                    let items = i.mem.array(array).ok_or(VmError::InvalidFont)?.to_vec();
                    items
                        .into_iter()
                        .map(|item| string(i, item))
                        .collect::<Result<Vec<_>, _>>()?
                }
                _ => Vec::new(),
            };
            (len_iv, subrs)
        }
        None => (4, Vec::new()),
    };
    let glyphs = named_entries(i, charstrings)?
        .into_iter()
        .map(|(name, value)| string(i, value).map(|code| (name, code)))
        .collect::<Result<Vec<_>, _>>()?;
    let entries = type1_dict(i, dict, private);
    Ok(Program::Type1(
        Type1Program::new(len_iv, subrs, glyphs).with_dict(entries),
    ))
}

/// The `sfnts` strings joined. A string of odd length ending in a zero
/// byte carries the conventional padding byte, which is not font data;
/// tables and glyph records are even-sized, so no other string is odd.
fn sfnts_bytes(i: &mut Interp, sfnts: Object) -> Result<Vec<u8>, VmError> {
    let strings = i.mem.array(sfnts).ok_or(VmError::InvalidFont)?.to_vec();
    let mut bytes = Vec::new();
    for item in strings {
        let data = string(i, item)?;
        let keep = if !data.len().is_multiple_of(2) && data.last() == Some(&0) {
            data.len() - 1
        } else {
            data.len()
        };
        bytes.extend_from_slice(&data[..keep]);
    }
    Ok(bytes)
}

fn type42(i: &mut Interp, dict: Object) -> Result<Program, VmError> {
    let sfnts = get(i, dict, "sfnts")
        .filter(|o| matches!(o.ty(), Type::Array | Type::PackedArray))
        .ok_or(VmError::InvalidFont)?;
    let charstrings = get_dict(i, dict, "CharStrings").ok_or(VmError::InvalidFont)?;
    let bytes = sfnts_bytes(i, sfnts)?;
    let names: BTreeMap<Vec<u8>, u16> = named_entries(i, charstrings)?
        .into_iter()
        .filter_map(|(name, value)| {
            let gid = u16::try_from(value.as_i32()?).ok()?;
            Some((name, gid))
        })
        .collect();
    let program = TrueTypeProgram::parse(bytes)
        .map_err(|_| VmError::InvalidFont)?
        .with_names(names);
    Ok(Program::TrueType(program))
}

/// A `CIDFontType 2` dictionary: the TrueType program of `sfnts` with
/// the `CIDMap` as its CID map — a string (or array of strings) of
/// `GDBytes` per CID, a dictionary of CID to glyph index, or an integer
/// offset added to every CID — over `CIDCount` CIDs.
fn cidfont_type2(i: &mut Interp, dict: Object) -> Result<Program, VmError> {
    let sfnts = get(i, dict, "sfnts")
        .filter(|o| matches!(o.ty(), Type::Array | Type::PackedArray))
        .ok_or(VmError::InvalidFont)?;
    let bytes = sfnts_bytes(i, sfnts)?;
    let program = TrueTypeProgram::parse(bytes).map_err(|_| VmError::InvalidFont)?;
    let count = get(i, dict, "CIDCount")
        .and_then(Object::as_i32)
        .and_then(|n| u32::try_from(n).ok());
    let cid_map = get(i, dict, "CIDMap").ok_or(VmError::InvalidFont)?;
    let map = match cid_map.ty() {
        Type::String | Type::Array | Type::PackedArray => {
            let data = match cid_map.ty() {
                Type::String => string(i, cid_map)?,
                _ => {
                    let strings = i.mem.array(cid_map).ok_or(VmError::InvalidFont)?.to_vec();
                    let mut data = Vec::new();
                    for item in strings {
                        data.extend(string(i, item)?);
                    }
                    data
                }
            };
            let width = get(i, dict, "GDBytes")
                .and_then(Object::as_i32)
                .map_or(2, |n| n.clamp(1, 4) as usize);
            let available = data.len() / width;
            let count = count.map_or(available, |c| (c as usize).min(available));
            let table = data
                .chunks(width)
                .take(count)
                .map(|chunk| {
                    chunk
                        .iter()
                        .fold(0u32, |acc, &b| acc << 8 | u32::from(b))
                        .min(u32::from(u16::MAX)) as u16
                })
                .collect();
            CidMap::Table(table)
        }
        Type::Dict => {
            let entries = i.mem.dict(cid_map).ok_or(VmError::InvalidFont)?;
            let pairs: Vec<(u16, u16)> = entries
                .iter()
                .filter_map(|(key, value)| {
                    let cid = u16::try_from(key.as_i32()?).ok()?;
                    let gid = u16::try_from(value.as_i32()?).ok()?;
                    Some((cid, gid))
                })
                .collect();
            let highest = pairs.iter().map(|&(cid, _)| u32::from(cid) + 1).max();
            let count = count.or(highest).unwrap_or(0) as usize;
            let mut table = vec![0u16; count];
            for (cid, gid) in pairs {
                if let Some(slot) = table.get_mut(usize::from(cid)) {
                    *slot = gid;
                }
            }
            CidMap::Table(table)
        }
        Type::Integer => CidMap::Offset {
            offset: u16::try_from(cid_map.as_i32().expect("integer"))
                .map_err(|_| VmError::InvalidFont)?,
            count: count.unwrap_or_else(|| u32::from(program.num_glyphs())),
        },
        _ => return Err(VmError::InvalidFont),
    };
    Ok(Program::TrueType(program.with_cid_map(map)))
}
