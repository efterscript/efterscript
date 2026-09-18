// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `CIDInit` procedure set (PLRM3 §5.11.4): the operators a CMap
//! program is written in, which build a `efterscript_fonts::cmap::CMap` on the
//! interpreter between `begincmap` and `endcmap`, and the `StartData`
//! form that reads the glyph data of the CIDFont dictionary being
//! defined (`ops::fontset` dispatches here for the string-operand
//! form). `endcmap` turns the current dictionary into the CMap
//! dictionary by giving it a `CodeMap` entry whose id resolves to the
//! built CMap; `/CMap defineresource` then stores that dictionary.
//!
//! The predefined CMaps are the shipped resource files, run through
//! these same operators on first use. An operator that needs one not
//! yet loaded — `findresource`, `usecmap`, `composefont`, `definefont`
//! — starts the load and returns with its operands in place: the file's
//! text runs as a string source above a `CMapLoad` marker, and when the
//! marker is reached the resource moves into the predefined table and
//! the operator runs again, finding it there.

use std::rc::Rc;

use efterscript_fonts::cidfont::{CidLayout, FdLayout, SubrSource, Type1CidProgram};
use efterscript_fonts::cmap::{CMap, CMapBuilder, CMapError, CidSystemInfo};
use efterscript_fonts::{Program, type1};

use crate::error::VmError;
use crate::interp::{Frame, Interp, Marker, SourceFrame, SourceSlot};
use crate::memory::Memory;
use crate::names::Atom;
use crate::object::{Access, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::font::{self, entry, number};
use crate::ops::graphics::read_matrix;
use crate::scanner::Scanner;
use crate::source::StringSource;

op_table! { procset OPS {
    "begincmap" => begincmap;
    "endcmap" => endcmap;
    "begincodespacerange" => begin_block, [Int];
    "endcodespacerange" => endcodespacerange;
    "begincidrange" => begin_block, [Int];
    "endcidrange" => endcidrange;
    "begincidchar" => begin_block, [Int];
    "endcidchar" => endcidchar;
    "beginnotdefrange" => begin_block, [Int];
    "endnotdefrange" => endnotdefrange;
    "beginbfchar" => begin_block, [Int];
    "endbfchar" => endbfchar;
    "beginbfrange" => begin_block, [Int];
    "endbfrange" => endbfrange;
    "beginusematrix" => begin_block, [Int];
    "endusematrix" => end_ignored;
    "usecmap" => usecmap, [Name];
    "usefont" => usefont, [Int];
}}

/// The key of the entry `endcmap` adds to the CMap dictionary: a font-id
/// object holding the id of the built CMap, which a program can copy
/// but not forge.
pub(crate) const CODE_MAP_KEY: &str = "CodeMap";

const OPERATORS: [&str; 19] = [
    "begincmap",
    "endcmap",
    "begincodespacerange",
    "endcodespacerange",
    "begincidrange",
    "endcidrange",
    "begincidchar",
    "endcidchar",
    "beginnotdefrange",
    "endnotdefrange",
    "beginbfchar",
    "endbfchar",
    "beginbfrange",
    "endbfrange",
    "beginusematrix",
    "endusematrix",
    "usecmap",
    "usefont",
    "StartData",
];

/// The `CIDInit` dictionary: every operator above, read-only, in the
/// allocation space `mem` is set to.
pub(crate) fn init_dict(mem: &mut Memory) -> Result<Object, VmError> {
    let dict = mem.new_dict(OPERATORS.len() as u32);
    for name in OPERATORS {
        let index = crate::ops::find(name, crate::ops::Visibility::ProcSet)
            .expect("CIDInit operators are in the operator table");
        let key = mem.intern(name.as_bytes()).expect("short name");
        mem.dict_put(dict, key, Object::operator(index))?;
    }
    mem.dict_set_access(dict, Access::ReadOnly)?;
    Ok(dict)
}

pub(crate) fn is_predefined(name: &[u8]) -> bool {
    efterscript_fonts::cmap::predefined(name).is_some()
}

/// The names of the predefined CMaps, sorted.
pub(crate) fn predefined_names() -> Vec<&'static str> {
    efterscript_fonts::cmap::PREDEFINED
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

/// What resolving a CMap operand came to.
pub(crate) enum Resolved {
    Found(Object),
    /// A predefined CMap's load has started; the operator returns with
    /// its operands untouched and runs again when the load is done.
    Pending,
}

/// The CMap dictionary `operand` names: a dictionary is taken as it is;
/// a name finds a defined instance, then a predefined CMap, loading it
/// through `retry` when it is not yet loaded. An unknown name is
/// `undefined`.
pub(crate) fn resolve(i: &mut Interp, operand: Object, retry: &str) -> Result<Resolved, VmError> {
    if operand.ty() == Type::Dict {
        return Ok(Resolved::Found(operand));
    }
    let key = i.mem.dict_key(operand)?;
    let atom = key.as_name().ok_or(VmError::TypeCheck)?;
    let category = i.cmap_category;
    if let Some(found) = i.mem.dict_get(category.local, key)? {
        return Ok(Resolved::Found(found));
    }
    if let Some(found) = i.mem.dict_get(category.global, key)? {
        return Ok(Resolved::Found(found));
    }
    let name = i.mem.name_text(atom).to_vec();
    if let Some(found) = i.predefined_cmap(&name) {
        return Ok(Resolved::Found(found));
    }
    let Some(text) = efterscript_fonts::cmap::predefined(&name) else {
        return Err(VmError::Undefined);
    };
    let retry = crate::ops::find(retry, crate::ops::Visibility::Public)
        .or_else(|| crate::ops::find(retry, crate::ops::Visibility::ProcSet))
        .expect("the retrying operator is in the table");
    start_load(i, atom, text, retry)?;
    Ok(Resolved::Pending)
}

/// Runs a predefined CMap's program above a `CMapLoad` marker, in global
/// allocation mode so the dictionaries it builds outlive every save.
fn start_load(i: &mut Interp, name: Atom, text: &str, retry: u32) -> Result<(), VmError> {
    let global = i.mem.current_global();
    i.mem.set_global(true);
    let string = i.mem.alloc_string(text.as_bytes().to_vec());
    i.push_frame_unchecked(Frame::Marker(Marker::CMapLoad {
        name,
        global,
        retry,
    }));
    let frame = Frame::Source(Box::new(SourceFrame {
        slot: SourceSlot::String(StringSource::new(string).expect("string")),
        scanner: Scanner::new(),
    }));
    if let Err(e) = i.push_frame(frame) {
        i.pop_frame();
        return Err(e);
    }
    Ok(())
}

/// A `CMapLoad` marker was reached: the resource the program defined
/// moves from the category dictionary into the predefined table, and
/// the operator that needed it runs again.
pub(crate) fn loaded(i: &mut Interp, name: Atom, retry: u32) -> Result<(), VmError> {
    let key = Object::name(name);
    let category = i.cmap_category;
    let dict = i
        .mem
        .dict_get(category.global, key)?
        .ok_or(VmError::Undefined)?;
    i.mem.dict_undef(category.global, key)?;
    let text = i.mem.name_text(name).to_vec();
    i.cache_predefined_cmap(text, dict);
    i.push_frame(Frame::Object(Object::operator(retry)))
}

/// Whether `dict` is a CMap dictionary: one `endcmap` gave a `CodeMap`.
pub(crate) fn is_cmap_dict(i: &mut Interp, dict: Object) -> bool {
    dict.ty() == Type::Dict && cmap_id(i, dict).is_some()
}

fn cmap_id(i: &mut Interp, dict: Object) -> Option<u32> {
    let key = i.intern(CODE_MAP_KEY);
    i.mem.dict(dict)?.get(key)?.as_font_id()
}

/// The CMap a dictionary carries; `invalidfont` for a dictionary that
/// is not one.
pub(crate) fn cmap_of(i: &mut Interp, dict: Object) -> Result<Rc<CMap>, VmError> {
    if dict.ty() != Type::Dict {
        return Err(VmError::InvalidFont);
    }
    cmap_id(i, dict)
        .and_then(|id| i.cmap(id))
        .ok_or(VmError::InvalidFont)
}

// --- the CMap program operators -----------------------------------------------

fn begincmap(i: &mut Interp) -> Result<(), VmError> {
    i.cmap_builders.push(CMapBuilder::new());
    Ok(())
}

fn builder(i: &mut Interp) -> Result<&mut CMapBuilder, VmError> {
    i.cmap_builders.last_mut().ok_or(VmError::Undefined)
}

fn system_info(i: &mut Interp, value: Object) -> Result<Option<CidSystemInfo>, VmError> {
    let dict = match value.ty() {
        Type::Dict => value,
        Type::Array | Type::PackedArray => match items(i, value)?.first() {
            Some(first) if first.ty() == Type::Dict => *first,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    let text = |i: &mut Interp, key: &str| -> Result<Vec<u8>, VmError> {
        match entry(i, dict, key)? {
            Some(v) if v.ty() == Type::String => bytes(i, v),
            Some(v) if v.ty() == Type::Name => {
                Ok(i.mem.name_text(v.as_name().expect("name")).to_vec())
            }
            _ => Ok(Vec::new()),
        }
    };
    let registry = text(i, "Registry")?;
    let ordering = text(i, "Ordering")?;
    let supplement = entry(i, dict, "Supplement")?
        .and_then(Object::as_i32)
        .unwrap_or(0);
    Ok(Some(CidSystemInfo {
        registry,
        ordering,
        supplement,
    }))
}

/// Finishes the CMap: its name, writing mode, and system information are
/// read from the current dictionary, which then gets the `CodeMap` id.
fn endcmap(i: &mut Interp) -> Result<(), VmError> {
    let mut builder = i.cmap_builders.pop().ok_or(VmError::Undefined)?;
    let dict = i.current_dict();
    if let Some(name) = entry(i, dict, "CMapName")? {
        let text = match name.ty() {
            Type::Name => i.mem.name_text(name.as_name().expect("name")).to_vec(),
            Type::String => bytes(i, name)?,
            _ => Vec::new(),
        };
        builder.name(&text);
    }
    if let Some(wmode) = entry(i, dict, "WMode")?.and_then(Object::as_i32) {
        builder.wmode(u8::try_from(wmode).map_err(|_| VmError::RangeCheck)?);
    }
    if let Some(info) = entry(i, dict, "CIDSystemInfo")?
        && let Some(info) = system_info(i, info)?
    {
        builder.system_info(info);
    }
    let id = i.register_cmap(Rc::new(builder.build()));
    let key = i.intern(CODE_MAP_KEY);
    i.mem.dict_put(dict, key, Object::font_id(id))
}

/// `n begin…`: the count is dropped and a mark opens the block.
fn begin_block(i: &mut Interp) -> Result<(), VmError> {
    i.pop()?;
    i.push(Object::mark())
}

/// The objects above the innermost mark, in order; the mark is popped
/// with them. `unmatchedmark` without one.
fn block(i: &mut Interp) -> Result<Vec<Object>, VmError> {
    let stack = i.ostack();
    let depth = stack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let objects = stack[depth + 1..].to_vec();
    for _ in 0..=objects.len() {
        i.pop()?;
    }
    Ok(objects)
}

fn end_ignored(i: &mut Interp) -> Result<(), VmError> {
    block(i).map(|_| ())
}

fn string_of(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    if object.ty() != Type::String {
        return Err(VmError::TypeCheck);
    }
    bytes(i, object)
}

fn int_of(object: Object) -> Result<i64, VmError> {
    object.as_i32().map(i64::from).ok_or(VmError::TypeCheck)
}

fn malformed(_: CMapError) -> VmError {
    VmError::RangeCheck
}

fn endcodespacerange(i: &mut Interp) -> Result<(), VmError> {
    let objects = block(i)?;
    if !objects.len().is_multiple_of(2) {
        return Err(VmError::RangeCheck);
    }
    for pair in objects.chunks(2) {
        let low = string_of(i, pair[0])?;
        let high = string_of(i, pair[1])?;
        builder(i)?.codespace(&low, &high).map_err(malformed)?;
    }
    Ok(())
}

type RangeAdder =
    for<'a> fn(&'a mut CMapBuilder, &[u8], &[u8], i64) -> Result<&'a mut CMapBuilder, CMapError>;

fn ranges(i: &mut Interp, add: RangeAdder) -> Result<(), VmError> {
    let objects = block(i)?;
    if !objects.len().is_multiple_of(3) {
        return Err(VmError::RangeCheck);
    }
    for triple in objects.chunks(3) {
        let low = string_of(i, triple[0])?;
        let high = string_of(i, triple[1])?;
        let cid = int_of(triple[2])?;
        add(builder(i)?, &low, &high, cid).map_err(malformed)?;
    }
    Ok(())
}

fn endcidrange(i: &mut Interp) -> Result<(), VmError> {
    ranges(i, CMapBuilder::cid_range)
}

fn endnotdefrange(i: &mut Interp) -> Result<(), VmError> {
    ranges(i, CMapBuilder::notdef_range)
}

fn endcidchar(i: &mut Interp) -> Result<(), VmError> {
    let objects = block(i)?;
    if !objects.len().is_multiple_of(2) {
        return Err(VmError::RangeCheck);
    }
    for pair in objects.chunks(2) {
        let code = string_of(i, pair[0])?;
        let cid = int_of(pair[1])?;
        builder(i)?.cid_char(&code, cid).map_err(malformed)?;
    }
    Ok(())
}

/// A `bf` destination: a string's bytes, or a name's text.
fn destination(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    match object.ty() {
        Type::String => bytes(i, object),
        Type::Name => Ok(i.mem.name_text(object.as_name().expect("name")).to_vec()),
        _ => Err(VmError::TypeCheck),
    }
}

fn endbfchar(i: &mut Interp) -> Result<(), VmError> {
    let objects = block(i)?;
    if !objects.len().is_multiple_of(2) {
        return Err(VmError::RangeCheck);
    }
    for pair in objects.chunks(2) {
        let code = string_of(i, pair[0])?;
        let dst = destination(i, pair[1])?;
        builder(i)?.bf_char(&code, &dst).map_err(malformed)?;
    }
    Ok(())
}

fn endbfrange(i: &mut Interp) -> Result<(), VmError> {
    let objects = block(i)?;
    if !objects.len().is_multiple_of(3) {
        return Err(VmError::RangeCheck);
    }
    for triple in objects.chunks(3) {
        let low = string_of(i, triple[0])?;
        let high = string_of(i, triple[1])?;
        let dst = match triple[2].ty() {
            Type::Array | Type::PackedArray => items(i, triple[2])?
                .into_iter()
                .map(|item| destination(i, item))
                .collect::<Result<Vec<_>, _>>()?,
            _ => vec![destination(i, triple[2])?],
        };
        builder(i)?.bf_range(&low, &high, dst).map_err(malformed)?;
    }
    Ok(())
}

fn usecmap(i: &mut Interp) -> Result<(), VmError> {
    let name = i.peek(0)?;
    builder(i)?;
    let dict = match resolve(i, name, "usecmap")? {
        Resolved::Found(dict) => dict,
        Resolved::Pending => return Ok(()),
    };
    let parent = cmap_of(i, dict)?;
    builder(i)?.use_cmap(parent);
    i.pop()?;
    Ok(())
}

fn usefont(i: &mut Interp) -> Result<(), VmError> {
    let font = i.peek(0)?.as_i32().expect("integer");
    let font = u8::try_from(font).map_err(|_| VmError::RangeCheck)?;
    builder(i)?.use_font(font);
    i.pop()?;
    Ok(())
}

// --- StartData: a CIDFont's glyph data --------------------------------------------

/// Exactly `count` bytes from `file`; fewer is `invalidfont`.
fn read_binary(i: &mut Interp, file: Object, count: usize) -> Result<Vec<u8>, VmError> {
    let mut data = vec![0u8; count];
    let mut filled = 0;
    while filled < count {
        let got = i.mem.file_read(file, &mut data[filled..])?;
        if got == 0 {
            return Err(VmError::InvalidFont);
        }
        filled += got;
    }
    Ok(data)
}

/// `count` bytes from hexadecimal digits in `file`, whitespace skipped.
fn read_hex(i: &mut Interp, file: Object, count: usize) -> Result<Vec<u8>, VmError> {
    let mut data = Vec::with_capacity(count);
    let mut high = None;
    let mut byte = [0u8; 1];
    while data.len() < count {
        if i.mem.file_read(file, &mut byte)? == 0 {
            return Err(VmError::InvalidFont);
        }
        match type1::hex_value(byte[0]) {
            Some(v) => match high.take() {
                None => high = Some(v),
                Some(h) => data.push(h << 4 | v),
            },
            None if byte[0].is_ascii_whitespace() => {}
            None => return Err(VmError::InvalidFont),
        }
    }
    Ok(data)
}

fn int_entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<i32>, VmError> {
    Ok(entry(i, dict, key)?.and_then(Object::as_i32))
}

fn size_entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<usize>, VmError> {
    match int_entry(i, dict, key)? {
        None => Ok(None),
        Some(v) => usize::try_from(v)
            .map(Some)
            .map_err(|_| VmError::InvalidFont),
    }
}

fn matrix_entry(i: &mut Interp, dict: Object) -> Result<Option<[f32; 6]>, VmError> {
    match entry(i, dict, "FontMatrix")? {
        None => Ok(None),
        Some(m) => read_matrix(i, m)
            .map(|m| Some(m.0))
            .map_err(|_| VmError::InvalidFont),
    }
}

/// The layout the dictionary declares for its glyph data.
fn layout(i: &mut Interp, dict: Object) -> Result<CidLayout, VmError> {
    let cid_map_offset = size_entry(i, dict, "CIDMapOffset")?.ok_or(VmError::InvalidFont)?;
    let fd_bytes = size_entry(i, dict, "FDBytes")?.unwrap_or(0);
    let gd_bytes = size_entry(i, dict, "GDBytes")?.ok_or(VmError::InvalidFont)?;
    let cid_count = size_entry(i, dict, "CIDCount")?.ok_or(VmError::InvalidFont)?;
    let font_matrix = matrix_entry(i, dict)?.unwrap_or([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    let fd_array = entry(i, dict, "FDArray")?
        .filter(|a| matches!(a.ty(), Type::Array | Type::PackedArray))
        .ok_or(VmError::InvalidFont)?;
    let mut fds = Vec::new();
    for fd in items(i, fd_array)? {
        if fd.ty() != Type::Dict {
            return Err(VmError::InvalidFont);
        }
        let private = entry(i, fd, "Private")?.filter(|p| p.ty() == Type::Dict);
        let mut len_iv = 4;
        let mut subrs = SubrSource::None;
        if let Some(private) = private {
            let get = |i: &mut Interp, key: &str| -> Option<Object> {
                let key = i.intern(key);
                i.mem.dict(private)?.get(key)
            };
            if let Some(v) = get(i, "lenIV").and_then(Object::as_i32) {
                len_iv = v;
            }
            let map_offset = get(i, "SubrMapOffset").and_then(Object::as_i32);
            let sd_bytes = get(i, "SDBytes").and_then(Object::as_i32);
            let count = get(i, "SubrCount").and_then(Object::as_i32);
            if let (Some(offset), Some(bytes), Some(count)) = (map_offset, sd_bytes, count) {
                subrs = SubrSource::Map {
                    offset: usize::try_from(offset).map_err(|_| VmError::InvalidFont)?,
                    bytes: u8::try_from(bytes).map_err(|_| VmError::InvalidFont)?,
                    count: usize::try_from(count).map_err(|_| VmError::InvalidFont)?,
                };
            } else if let Some(array) =
                get(i, "Subrs").filter(|a| matches!(a.ty(), Type::Array | Type::PackedArray))
            {
                let strings = items(i, array)?
                    .into_iter()
                    .map(|s| string_of(i, s).map_err(|_| VmError::InvalidFont))
                    .collect::<Result<Vec<_>, _>>()?;
                subrs = SubrSource::Strings(strings);
            }
        }
        let font_matrix = matrix_entry(i, fd)?;
        fds.push(FdLayout {
            len_iv,
            subrs,
            font_matrix,
        });
    }
    Ok(CidLayout {
        cid_map_offset,
        fd_bytes: u8::try_from(fd_bytes).map_err(|_| VmError::InvalidFont)?,
        gd_bytes: u8::try_from(gd_bytes).map_err(|_| VmError::InvalidFont)?,
        cid_count: u32::try_from(cid_count).map_err(|_| VmError::InvalidFont)?,
        font_matrix,
        fds,
    })
}

/// `(Binary|Hex) count StartData`, with the CIDFont dictionary as the
/// current dictionary: reads the glyph data from the current file,
/// parses it as the dictionary's `CIDMap` and `FDArray` entries
/// describe, defines the dictionary as a `CIDFont` resource under its
/// `CIDFontName`, and then ends both that dictionary and the procedure
/// set's, which the canonical file form leaves open. The operands stay
/// in place until everything has been read and parsed.
pub(crate) fn start_data(i: &mut Interp) -> Result<(), VmError> {
    let count = i.peek(0)?.as_i32().expect("integer");
    let form = string_of(i, i.peek(1)?)?;
    let dict = i.current_dict();
    let count = usize::try_from(count).map_err(|_| VmError::RangeCheck)?;
    let hex = match form.as_slice() {
        b"Binary" => false,
        b"Hex" => true,
        _ => return Err(VmError::RangeCheck),
    };
    let name = entry(i, dict, "CIDFontName")?.ok_or(VmError::InvalidFont)?;
    let layout = layout(i, dict)?;
    let file = i.current_file();
    let data = if hex {
        read_hex(i, file, count)?
    } else {
        read_binary(i, file, count)?
    };
    let program = Type1CidProgram::parse(&data, &layout).map_err(|_| VmError::InvalidFont)?;
    let matrix_key = i.intern("FontMatrix");
    if !i.mem.dict_known(dict, matrix_key)? {
        let matrix = i.mem.alloc_array(layout.font_matrix.map(number).to_vec())?;
        i.mem.dict_put(dict, matrix_key, matrix)?;
    }
    let fid_key = i.intern("FID");
    let fid = match i.mem.dict_get(dict, fid_key)? {
        Some(fid) => fid,
        None => {
            let fid = i.allocate_fid();
            i.mem.dict_put(dict, fid_key, fid)?;
            fid
        }
    };
    i.cache_font_program(
        fid.as_font_id().expect("a font id"),
        Rc::new(Program::Type1Cid(program)),
    );
    font::define(i, name, dict)?.ok_or(VmError::InvalidFont)?;
    i.pop()?;
    i.pop()?;
    i.end_dict()?;
    i.end_dict()?;
    Ok(())
}
