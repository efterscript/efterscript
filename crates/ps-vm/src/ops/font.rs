// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Font operators (PLRM3 §5, §8.2): font dictionaries and the font
//! directories, the resident standard fonts materialised on first
//! `findfont`, name substitution, the derived-font operators, the show
//! family, and the width operators a Type 3 glyph procedure calls.
//!
//! `OPS` is always defined: a scripting embedder can define, find, scale,
//! and measure fonts without a graphics backend, the VM keeping the
//! current font in its own slot. `PAINT_OPS` marks the page and enters
//! `systemdict` only with a backend, like the rest of the graphics group.

use ps_fonts::{Encoding, ResidentFace};

use crate::error::VmError;
use crate::graphics::{FontRef, Matrix};
use crate::interp::Interp;
use crate::memory::Memory;
use crate::object::{Access, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::graphics::read_matrix;
use crate::ops::pagedevice::in_global;
use crate::ops::show::{self, Variant};

op_table! { OPS {
    "definefont" => definefont, [Any, Dict];
    "undefinefont" => undefinefont, [Any];
    "findfont" => findfont, [Any];
    "scalefont" => scalefont, [Dict, Num];
    "makefont" => makefont, [Dict, Array];
    "setfont" => setfont, [Dict];
    "currentfont" => currentfont;
    "selectfont" => selectfont, [Any, Any];
    "stringwidth" => stringwidth, [String];
    "setcachedevice" => setcachedevice, [Num, Num, Num, Num, Num, Num];
    "setcachedevice2" => setcachedevice2, [Num, Num, Num, Num, Num, Num, Num, Num, Num, Num];
    "setcharwidth" => setcharwidth, [Num, Num];
}}

op_table! { graphics PAINT_OPS {
    "show" => show_, [String];
    "ashow" => ashow, [Num, Num, String];
    "widthshow" => widthshow, [Num, Num, Int, String];
    "awidthshow" => awidthshow, [Num, Num, Int, Num, Num, String];
    "kshow" => kshow, [Array, String];
    "xshow" => xshow, [String, Array];
    "yshow" => yshow, [String, Array];
    "xyshow" => xyshow, [String, Array];
    "glyphshow" => glyphshow, [Name];
    "charpath" => charpath, [String, Bool];
}}

/// The marker entry of a resident font's dictionary: the index of the
/// resident face whose metrics it measures with.
pub const RESIDENT_KEY: &str = "ResidentFont";

fn is_array(object: Object) -> bool {
    matches!(object.ty(), Type::Array | Type::PackedArray)
}

fn num_at(i: &Interp, n: usize) -> Result<f32, VmError> {
    i.peek(n)?.as_number().ok_or(VmError::TypeCheck)
}

fn drop(i: &mut Interp, count: usize) -> Result<(), VmError> {
    for _ in 0..count {
        i.pop()?;
    }
    Ok(())
}

/// The value under `key` in a font dictionary, which needs read access.
pub(crate) fn entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<Object>, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)
}

/// A read-only array of 256 names for an encoding table, unassigned codes
/// as `.notdef`, in the current allocation space.
pub(crate) fn encoding_array(mem: &mut Memory, table: &Encoding) -> Result<Object, VmError> {
    let mut names = Vec::with_capacity(256);
    for name in table {
        names.push(mem.intern(name.unwrap_or(".notdef").as_bytes())?);
    }
    let array = mem.alloc_array(names)?;
    Ok(array
        .with_access(Access::ReadOnly)
        .expect("arrays carry access"))
}

// --- definition and lookup ------------------------------------------------------

/// The structure `definefont` requires: a `FontType` this VM can handle,
/// a matrix, a full encoding, and for Type 3 a glyph procedure.
fn validate(i: &mut Interp, font: Object) -> Result<(), VmError> {
    let font_type = entry(i, font, "FontType")?.and_then(Object::as_i32);
    if !matches!(font_type, Some(1 | 2 | 3 | 42)) {
        return Err(VmError::InvalidFont);
    }
    let matrix = entry(i, font, "FontMatrix")?.ok_or(VmError::InvalidFont)?;
    read_matrix(i, matrix).map_err(|_| VmError::InvalidFont)?;
    let encoding = entry(i, font, "Encoding")?.ok_or(VmError::InvalidFont)?;
    if !is_array(encoding) || encoding.length() != Some(256) {
        return Err(VmError::InvalidFont);
    }
    if font_type == Some(3) {
        let procedure = entry(i, font, "BuildGlyph")?.or(entry(i, font, "BuildChar")?);
        if !procedure.is_some_and(|p| is_array(p) && p.is_executable()) {
            return Err(VmError::InvalidFont);
        }
    }
    Ok(())
}

/// `definefont` proper, shared with `defineresource`: validates, gives
/// the dictionary its `FID`, makes it read-only, and registers it in
/// `FontDirectory` (and `GlobalFontDirectory` in global allocation
/// mode) under `key`.
pub(crate) fn define(i: &mut Interp, key: Object, font: Object) -> Result<Object, VmError> {
    validate(i, font)?;
    let key = i.mem.dict_key(key)?;
    let fid = i.intern("FID");
    if !i.mem.dict_known(font, fid)? {
        let id = i.allocate_fid();
        i.mem.dict_put(font, fid, id)?;
    }
    if let Some(id) = i.mem.dict_get(font, fid)?.and_then(Object::as_font_id) {
        let matrix = entry(i, font, "FontMatrix")?.ok_or(VmError::InvalidFont)?;
        let matrix = read_matrix(i, matrix).map_err(|_| VmError::InvalidFont)?;
        i.record_defined_matrix(id, matrix);
    }
    i.mem.dict_set_access(font, Access::ReadOnly)?;
    let category = i.font_category;
    if i.mem.current_global() {
        i.mem.dict_put(category.global, key, font)?;
    }
    i.mem.dict_put(category.local, key, font)?;
    Ok(font)
}

fn definefont(i: &mut Interp) -> Result<(), VmError> {
    let font = i.peek(0)?;
    let key = i.peek(1)?;
    define(i, key, font)?;
    drop(i, 2)?;
    i.push(font)
}

/// Removes `key` from both font directories.
pub(crate) fn undefine(i: &mut Interp, key: Object) -> Result<(), VmError> {
    let key = i.mem.dict_key(key)?;
    let category = i.font_category;
    i.mem.dict_undef(category.local, key)?;
    i.mem.dict_undef(category.global, key)
}

fn undefinefont(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    undefine(i, key)?;
    drop(i, 1)
}

/// The font defined under `key` in either directory.
pub(crate) fn defined(i: &mut Interp, key: Object) -> Result<Option<Object>, VmError> {
    let category = i.font_category;
    if let Some(font) = i.mem.dict_get(category.local, key)? {
        return Ok(Some(font));
    }
    i.mem.dict_get(category.global, key)
}

/// The text of a name or string key; anything else is `typecheck`.
pub(crate) fn key_text(i: &mut Interp, key: Object) -> Result<Vec<u8>, VmError> {
    let key = i.mem.dict_key(key)?;
    match key.as_name() {
        Some(atom) => Ok(i.mem.name_text(atom).to_vec()),
        None => Err(VmError::TypeCheck),
    }
}

/// `findfont`: the directories, then the resident set by exact name,
/// then substitution when the configuration allows it.
pub(crate) fn find(i: &mut Interp, key: Object) -> Result<Object, VmError> {
    let text = key_text(i, key)?;
    let key = i.mem.dict_key(key)?;
    if let Some(font) = defined(i, key)? {
        return Ok(font);
    }
    if let Some(face) = ResidentFace::from_postscript_name(&text) {
        return resident(i, face);
    }
    if !i.fonts_config.substitute {
        return Err(VmError::InvalidFont);
    }
    let font = ps_fonts::substitute(&text);
    i.record_substitution(text, font.postscript_name());
    resident(i, font)
}

fn findfont(i: &mut Interp) -> Result<(), VmError> {
    let key = i.peek(0)?;
    let font = find(i, key)?;
    drop(i, 1)?;
    i.push(font)
}

/// The dictionary of a resident font, built in global VM on first use.
pub(crate) fn resident(i: &mut Interp, font: ResidentFace) -> Result<Object, VmError> {
    if let Some(dict) = i.resident_fonts[font.index()] {
        return Ok(dict);
    }
    let dict = in_global(i, |i| materialise(i, font))?;
    i.resident_fonts[font.index()] = Some(dict);
    Ok(dict)
}

/// A number object: an integer when the value is integral.
pub(crate) fn number(value: f32) -> Object {
    if value == value.trunc() && value.abs() < 1e9 {
        Object::integer(value as i32)
    } else {
        Object::real(value)
    }
}

fn materialise(i: &mut Interp, font: ResidentFace) -> Result<Object, VmError> {
    let dict = i.mem.new_dict(10);
    let name = i.intern(font.postscript_name());
    let matrix = i.mem.alloc_array(vec![
        Object::real(0.001),
        Object::integer(0),
        Object::integer(0),
        Object::real(0.001),
        Object::integer(0),
        Object::integer(0),
    ])?;
    let bbox = i.mem.alloc_array(font.bbox().map(number).to_vec())?;
    let encoding = if font.is_symbolic() {
        encoding_array(&mut i.mem, font.builtin_encoding())?
    } else {
        i.standard_encoding
    };
    let fid = i.allocate_fid();
    let index = i32::try_from(font.index()).expect("thirty-five faces");
    let entries = [
        ("FontType", Object::integer(1)),
        ("FontName", name),
        ("FontMatrix", matrix),
        ("FontBBox", bbox),
        ("PaintType", Object::integer(0)),
        ("Encoding", encoding),
        ("FID", fid),
        (RESIDENT_KEY, Object::integer(index)),
    ];
    for (key, value) in entries {
        let key = i.intern(key);
        i.mem.dict_put(dict, key, value)?;
    }
    i.mem.dict_set_access(dict, Access::ReadOnly)?;
    Ok(dict)
}

// --- derived fonts and the current font ------------------------------------------

/// A copy of `font` sharing every entry but `FontMatrix`, which becomes
/// the original followed by `transform` (PLRM3 §5.3: showing with the
/// derived font is showing with the original under `transform concat`).
pub(crate) fn derive(i: &mut Interp, font: Object, transform: Matrix) -> Result<Object, VmError> {
    let entries = i.mem.dict_entries(font)?;
    let matrix_key = i.intern("FontMatrix");
    let matrix = entries
        .iter()
        .find(|(key, _)| key.eq(matrix_key))
        .map(|&(_, value)| value)
        .ok_or(VmError::InvalidFont)?;
    let matrix = read_matrix(i, matrix).map_err(|_| VmError::InvalidFont)?;
    let composed = matrix.then(transform);
    let array = i
        .mem
        .alloc_array(composed.0.iter().map(|&v| Object::real(v)).collect())?;
    let dict = i
        .mem
        .new_dict(u32::try_from(entries.len()).map_err(|_| VmError::LimitCheck)?);
    for (key, value) in entries {
        let value = if key.eq(matrix_key) { array } else { value };
        i.mem.dict_put(dict, key, value)?;
    }
    i.mem.dict_set_access(dict, Access::ReadOnly)?;
    Ok(dict)
}

fn scalefont(i: &mut Interp) -> Result<(), VmError> {
    let scale = num_at(i, 0)?;
    let font = i.peek(1)?;
    let derived = derive(i, font, Matrix::scaling(scale, scale))?;
    drop(i, 2)?;
    i.push(derived)
}

fn makefont(i: &mut Interp) -> Result<(), VmError> {
    let transform = read_matrix(i, i.peek(0)?)?;
    let font = i.peek(1)?;
    let derived = derive(i, font, transform)?;
    drop(i, 2)?;
    i.push(derived)
}

/// `setfont`: a font dictionary that has been through `definefont` (it
/// carries an `FID`) becomes the current font.
pub(crate) fn select(i: &mut Interp, font: Object) -> Result<(), VmError> {
    let matrix = entry(i, font, "FontMatrix")?.ok_or(VmError::InvalidFont)?;
    let matrix = read_matrix(i, matrix).map_err(|_| VmError::InvalidFont)?;
    if entry(i, font, "FID")?.is_none() {
        return Err(VmError::InvalidFont);
    }
    let instance = i.font_instance(font)?;
    i.set_current_font(Some(FontRef { instance, matrix }))
}

fn setfont(i: &mut Interp) -> Result<(), VmError> {
    let font = i.peek(0)?;
    select(i, font)?;
    drop(i, 1)
}

fn currentfont(i: &mut Interp) -> Result<(), VmError> {
    let font = i.current_font().ok_or(VmError::InvalidFont)?;
    let dict = i.font_dict(font.instance).ok_or(VmError::InvalidFont)?;
    i.push(dict)
}

fn selectfont(i: &mut Interp) -> Result<(), VmError> {
    let scale = i.peek(0)?;
    let key = i.peek(1)?;
    let transform = if is_array(scale) {
        read_matrix(i, scale)?
    } else {
        let s = scale.as_number().ok_or(VmError::TypeCheck)?;
        Matrix::scaling(s, s)
    };
    let font = find(i, key)?;
    let derived = derive(i, font, transform)?;
    select(i, derived)?;
    drop(i, 2)
}

// --- showing and measuring ---------------------------------------------------------

fn codes(i: &Interp, string: Object) -> Result<Vec<u8>, VmError> {
    bytes(i, string)
}

fn numbers(i: &Interp, array: Object) -> Result<Vec<f32>, VmError> {
    items(i, array)?
        .into_iter()
        .map(|o| o.as_number().ok_or(VmError::TypeCheck))
        .collect()
}

fn stringwidth(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    show::begin(i, "stringwidth", Variant::Show, string, true, 1)
}

fn show_(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    show::begin(i, "show", Variant::Show, string, false, 1)
}

fn ashow(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    let ay = num_at(i, 1)?;
    let ax = num_at(i, 2)?;
    show::begin(i, "ashow", Variant::AShow { ax, ay }, string, false, 3)
}

fn widthshow(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    let code = i.peek(1)?.as_i32().expect("integer");
    let cy = num_at(i, 2)?;
    let cx = num_at(i, 3)?;
    show::begin(
        i,
        "widthshow",
        Variant::WidthShow { cx, cy, code },
        string,
        false,
        4,
    )
}

fn awidthshow(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    let ay = num_at(i, 1)?;
    let ax = num_at(i, 2)?;
    let code = i.peek(3)?.as_i32().expect("integer");
    let cy = num_at(i, 4)?;
    let cx = num_at(i, 5)?;
    show::begin(
        i,
        "awidthshow",
        Variant::AWidthShow {
            cx,
            cy,
            code,
            ax,
            ay,
        },
        string,
        false,
        6,
    )
}

fn kshow(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(0)?)?;
    let procedure = i.peek(1)?;
    show::begin(i, "kshow", Variant::KShow { procedure }, string, false, 2)
}

fn positioned(
    i: &mut Interp,
    operator: &'static str,
    make: fn(Vec<f32>) -> Variant,
) -> Result<(), VmError> {
    let values = numbers(i, i.peek(0)?)?;
    let string = codes(i, i.peek(1)?)?;
    show::begin(i, operator, make(values), string, false, 2)
}

fn xshow(i: &mut Interp) -> Result<(), VmError> {
    positioned(i, "xshow", Variant::XShow)
}

fn yshow(i: &mut Interp) -> Result<(), VmError> {
    positioned(i, "yshow", Variant::YShow)
}

fn xyshow(i: &mut Interp) -> Result<(), VmError> {
    positioned(i, "xyshow", Variant::XYShow)
}

fn glyphshow(i: &mut Interp) -> Result<(), VmError> {
    let name = i.peek(0)?;
    show::begin(
        i,
        "glyphshow",
        Variant::GlyphShow(name),
        Vec::new(),
        false,
        1,
    )
}

/// `charpath`: the string's outlines join the current path; the boolean
/// is accepted and, with no stroked fonts drawn as such, ignored.
fn charpath(i: &mut Interp) -> Result<(), VmError> {
    let string = codes(i, i.peek(1)?)?;
    show::begin_charpath(i, string)
}

// --- glyph width declarations ---------------------------------------------------------

/// Records the width (and, for a cached glyph, the box) of the glyph
/// whose procedure is running; outside one the operator is `undefined`.
fn declare_width(
    i: &mut Interp,
    width: (f32, f32),
    bbox: Option<crate::graphics::Bounds>,
    operands: usize,
) -> Result<(), VmError> {
    let run = show::running_glyph(i).ok_or(VmError::Undefined)?;
    run.width = Some(width);
    run.bbox = bbox;
    drop(i, operands)
}

fn setcachedevice(i: &mut Interp) -> Result<(), VmError> {
    let bbox =
        crate::graphics::Bounds::new(num_at(i, 3)?, num_at(i, 2)?, num_at(i, 1)?, num_at(i, 0)?);
    let width = (num_at(i, 5)?, num_at(i, 4)?);
    declare_width(i, width, Some(bbox), 6)
}

// The vertical-mode operands are read and dropped: this VM shows in
// writing mode 0 only.
fn setcachedevice2(i: &mut Interp) -> Result<(), VmError> {
    let bbox =
        crate::graphics::Bounds::new(num_at(i, 7)?, num_at(i, 6)?, num_at(i, 5)?, num_at(i, 4)?);
    let width = (num_at(i, 9)?, num_at(i, 8)?);
    declare_width(i, width, Some(bbox), 10)
}

fn setcharwidth(i: &mut Interp) -> Result<(), VmError> {
    let width = (num_at(i, 1)?, num_at(i, 0)?);
    declare_width(i, width, None, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_values_become_integers() {
        assert_eq!(number(-166.0).as_i32(), Some(-166));
        assert_eq!(number(0.5).as_f32(), Some(0.5));
        assert_eq!(number(1e10).as_f32(), Some(1e10));
    }

    #[test]
    fn encoding_arrays_fill_gaps_with_notdef() {
        let mut mem = Memory::new();
        let array = encoding_array(&mut mem, &ps_fonts::STANDARD_ENCODING).unwrap();
        assert_eq!(array.length(), Some(256));
        assert_eq!(array.access(), Some(Access::ReadOnly));
        let items = mem.array(array).unwrap();
        assert_eq!(mem.name_text(items[65].as_name().unwrap()), b"A");
        assert_eq!(mem.name_text(items[0].as_name().unwrap()), b".notdef");
    }
}
