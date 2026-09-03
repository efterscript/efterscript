// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `FontSetInit` procedure set: `StartData` reads a CFF program from
//! the current file and defines its name-keyed fonts as FontType 2
//! dictionaries whose glyph programs are cached on the interpreter, and
//! the FontSet resource as the array of their names. A CID-keyed font in
//! the data is parsed and held by name for the composite-font machinery;
//! it is not a font a program can `findfont`.

use std::rc::Rc;

use ps_fonts::{CffProgram, Program};

use crate::error::VmError;
use crate::interp::Interp;
use crate::memory::Memory;
use crate::object::{Access, Object, Type};
use crate::ops::font::{self, number};

op_table! { procset OPS {
    "StartData" => start_data, [Name, Int];
}}

/// The `FontSetInit` dictionary: the one operator, read-only, in the
/// allocation space `mem` is set to.
pub(crate) fn init_dict(mem: &mut Memory) -> Result<Object, VmError> {
    let dict = mem.new_dict(1);
    let index = crate::ops::find("StartData", crate::ops::Visibility::ProcSet)
        .expect("StartData is in the operator table");
    let key = mem.intern(b"StartData").expect("short name");
    mem.dict_put(dict, key, Object::operator(index))?;
    mem.dict_set_access(dict, Access::ReadOnly)?;
    Ok(dict)
}

/// Exactly `count` bytes from `file`; fewer is `invalidfont`.
fn read_exact(i: &mut Interp, file: Object, count: usize) -> Result<Vec<u8>, VmError> {
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

fn start_data(i: &mut Interp) -> Result<(), VmError> {
    let count = i.peek(0)?.as_i32().expect("integer");
    let key = i.peek(1)?;
    let count = usize::try_from(count).map_err(|_| VmError::RangeCheck)?;
    let file = i.current_file();
    let data = read_exact(i, file, count)?;
    let fonts = ps_fonts::cff::parse_fonts(&data).map_err(|_| VmError::InvalidFont)?;
    let mut names = Vec::with_capacity(fonts.len());
    for cff in fonts {
        let name_text = cff.name().to_vec();
        if cff.is_cid_keyed() {
            i.cache_cid_program(name_text, Rc::new(Program::Cff(cff)));
            continue;
        }
        let name = i.mem.intern(&name_text).map_err(|_| VmError::LimitCheck)?;
        let (dict, fid) = font_dict(i, &cff, name)?;
        i.cache_font_program(fid, Rc::new(Program::Cff(cff)));
        font::define(i, name, dict)?;
        names.push(name);
    }
    let set = i
        .mem
        .alloc_array(names)?
        .with_access(Access::ReadOnly)
        .expect("arrays carry access");
    let category = i.fontset_category;
    let dict = if i.mem.current_global() {
        category.global
    } else {
        category.local
    };
    let key = i.mem.dict_key(key)?;
    i.mem.dict_put(dict, key, set)?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

/// A FontType 2 dictionary for a name-keyed program, with its `FID`
/// allocated so the program can be cached before `definefont` runs.
fn font_dict(i: &mut Interp, cff: &CffProgram, name: Object) -> Result<(Object, u32), VmError> {
    let matrix = i.mem.alloc_array(cff.font_matrix().map(number).to_vec())?;
    let bbox = i.mem.alloc_array(cff.font_bbox().map(number).to_vec())?;
    let encoding = if cff.has_standard_encoding() {
        i.standard_encoding
    } else {
        let table = cff.encoding();
        let mut names = Vec::with_capacity(256);
        for gid in table {
            let text = gid
                .and_then(|gid| cff.glyph_name(gid))
                .unwrap_or(b".notdef");
            names.push(i.mem.intern(text).map_err(|_| VmError::LimitCheck)?);
        }
        i.mem
            .alloc_array(names)?
            .with_access(Access::ReadOnly)
            .expect("arrays carry access")
    };
    let charstrings = i.mem.new_dict(u32::from(cff.glyph_count()));
    for (gid, glyph_name) in cff.charset_names().into_iter().enumerate() {
        let key = i.mem.intern(glyph_name).map_err(|_| VmError::LimitCheck)?;
        i.mem
            .dict_put(charstrings, key, Object::integer(gid as i32))?;
    }
    i.mem.dict_set_access(charstrings, Access::ReadOnly)?;
    let fid = i.allocate_fid();
    let dict = i.mem.new_dict(8);
    let entries = [
        ("FontType", Object::integer(2)),
        ("FontName", name),
        ("FontMatrix", matrix),
        ("FontBBox", bbox),
        ("PaintType", Object::integer(cff.paint_type())),
        ("Encoding", encoding),
        ("CharStrings", charstrings),
        ("FID", fid),
    ];
    for (key, value) in entries {
        let key = i.intern(key);
        i.mem.dict_put(dict, key, value)?;
    }
    debug_assert!(dict.ty() == Type::Dict);
    Ok((dict, fid.as_font_id().expect("a font id")))
}
