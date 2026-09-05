// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The resource operators (PLRM3 §3.9) over the `Font`, `Encoding`,
//! `ProcSet`, `FontSet`, `CMap`, and `CIDFont` categories. Each category
//! has a local and a global instance dictionary selected by the
//! allocation mode, plus its built-in instances: the resident fonts, the
//! two encoding arrays in `systemdict`, the `FontSetInit` and `CIDInit`
//! procedure sets, and the predefined CMaps, loaded on first use. A
//! built-in instance reports status 2 until it has been loaded — a
//! resident face materialised, a predefined CMap's program run, a
//! procedure set found — and 1 from then on; `restore` does not clear
//! that, the loaded object living in global VM. Other categories are
//! `undefined`.

use ps_fonts::ResidentFace;

use crate::error::VmError;
use crate::interp::{Category, Frame, Interp, LoopFrame};
use crate::object::{Access, Object, Type};
use crate::ops::array::bytes;
use crate::ops::cidinit::{self, Resolved};
use crate::ops::font;

op_table! { OPS {
    "findresource" => findresource, [Any, Name];
    "resourcestatus" => resourcestatus, [Any, Name];
    "defineresource" => defineresource, [Any, Any, Name];
    "undefineresource" => undefineresource, [Any, Name];
    "resourceforall" => resourceforall, [String, Array, String, Name];
}}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Font,
    Encoding,
    ProcSet,
    FontSet,
    CMap,
    CidFont,
}

const BUILTIN_ENCODINGS: [&str; 2] = ["ISOLatin1Encoding", "StandardEncoding"];
const BUILTIN_PROCSETS: [&str; 2] = ["CIDInit", "FontSetInit"];

/// Status of an instance defined by the program, in VM.
const STATUS_DEFINED: i32 = 0;
/// Status of a built-in instance loaded into VM.
const STATUS_LOADED: i32 = 1;
/// Status of a built-in instance not yet loaded, held outside VM.
const STATUS_RESIDENT: i32 = 2;

fn kind(i: &Interp, category: Object) -> Result<Kind, VmError> {
    let atom = category.as_name().ok_or(VmError::TypeCheck)?;
    match i.mem.name_text(atom) {
        b"Font" => Ok(Kind::Font),
        b"Encoding" => Ok(Kind::Encoding),
        b"ProcSet" => Ok(Kind::ProcSet),
        b"FontSet" => Ok(Kind::FontSet),
        b"CMap" => Ok(Kind::CMap),
        b"CIDFont" => Ok(Kind::CidFont),
        _ => Err(VmError::Undefined),
    }
}

fn dicts(i: &Interp, kind: Kind) -> Category {
    match kind {
        Kind::Font => i.font_category,
        Kind::Encoding => i.encoding_category,
        Kind::ProcSet => i.procset_category,
        Kind::FontSet => i.fontset_category,
        Kind::CMap => i.cmap_category,
        Kind::CidFont => i.cidfont_category,
    }
}

/// An instance the program defined, local VM first.
fn defined(i: &mut Interp, kind: Kind, key: Object) -> Result<Option<Object>, VmError> {
    let category = dicts(i, kind);
    if let Some(instance) = i.mem.dict_get(category.local, key)? {
        return Ok(Some(instance));
    }
    i.mem.dict_get(category.global, key)
}

/// A built-in instance by name, materialised if need be.
fn builtin(i: &mut Interp, kind: Kind, name: &[u8]) -> Result<Option<Object>, VmError> {
    match kind {
        Kind::Font => match ResidentFace::from_postscript_name(name) {
            Some(std) => font::resident(i, std).map(Some),
            None => Ok(None),
        },
        Kind::Encoding => Ok(match name {
            b"StandardEncoding" => Some(i.standard_encoding),
            b"ISOLatin1Encoding" => Some(i.iso_latin1_encoding),
            _ => None,
        }),
        Kind::ProcSet => Ok(match procset_index(name) {
            Some(index) => {
                i.loaded_procsets[index] = true;
                Some(if name == b"FontSetInit" {
                    i.font_set_init
                } else {
                    i.cid_init
                })
            }
            None => None,
        }),
        // Predefined CMaps are resolved by `findresource` itself, since
        // loading one runs a program.
        Kind::FontSet | Kind::CMap | Kind::CidFont => Ok(None),
    }
}

fn procset_index(name: &[u8]) -> Option<usize> {
    BUILTIN_PROCSETS.iter().position(|p| p.as_bytes() == name)
}

/// Whether the built-in instance `name` has been loaded into VM.
fn loaded(i: &Interp, kind: Kind, name: &[u8]) -> bool {
    match kind {
        Kind::Font => ResidentFace::from_postscript_name(name)
            .is_some_and(|face| i.resident_fonts[face.index()].is_some()),
        Kind::ProcSet => procset_index(name).is_some_and(|index| i.loaded_procsets[index]),
        Kind::CMap => i.predefined_cmap(name).is_some(),
        Kind::Encoding | Kind::FontSet | Kind::CidFont => false,
    }
}

fn has_builtin(kind: Kind, name: &[u8]) -> bool {
    match kind {
        Kind::Font => ResidentFace::from_postscript_name(name).is_some(),
        Kind::Encoding => BUILTIN_ENCODINGS.iter().any(|e| e.as_bytes() == name),
        Kind::ProcSet => BUILTIN_PROCSETS.iter().any(|p| p.as_bytes() == name),
        Kind::CMap => cidinit::is_predefined(name),
        Kind::FontSet | Kind::CidFont => false,
    }
}

fn findresource(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let key = i.peek(1)?;
    let key = i.mem.dict_key(key)?;
    let found = match defined(i, kind, key)? {
        Some(instance) => instance,
        None if kind == Kind::CMap => match cidinit::resolve(i, key, "findresource")? {
            Resolved::Found(dict) => dict,
            Resolved::Pending => return Ok(()),
        },
        None => {
            let name = key.as_name().map(|a| i.mem.name_text(a).to_vec());
            match name {
                Some(name) => builtin(i, kind, &name)?.ok_or(VmError::Undefined)?,
                None => return Err(VmError::Undefined),
            }
        }
    };
    i.pop()?;
    i.pop()?;
    i.push(found)
}

fn resourcestatus(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let key = i.peek(1)?;
    let key = i.mem.dict_key(key)?;
    let status = if defined(i, kind, key)?.is_some() {
        Some(STATUS_DEFINED)
    } else {
        match key
            .as_name()
            .filter(|&a| has_builtin(kind, i.mem.name_text(a)))
        {
            Some(atom) if matches!(kind, Kind::Font | Kind::ProcSet | Kind::CMap) => {
                let name = i.mem.name_text(atom).to_vec();
                Some(if loaded(i, kind, &name) {
                    STATUS_LOADED
                } else {
                    STATUS_RESIDENT
                })
            }
            Some(_) => Some(STATUS_DEFINED),
            None => None,
        }
    };
    i.pop()?;
    i.pop()?;
    match status {
        Some(status) => {
            i.push(Object::integer(status))?;
            i.push(Object::integer(0))?;
            i.push(Object::boolean(true))
        }
        None => i.push(Object::boolean(false)),
    }
}

fn defineresource(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let instance = i.peek(1)?;
    let key = i.peek(2)?;
    match kind {
        Kind::Font | Kind::CidFont => {
            if instance.ty() != Type::Dict {
                return Err(VmError::TypeCheck);
            }
            if font::define(i, key, instance)?.is_none() {
                return Ok(());
            }
        }
        Kind::CMap => {
            if !cidinit::is_cmap_dict(i, instance) {
                return Err(VmError::TypeCheck);
            }
            let key = i.mem.dict_key(key)?;
            let category = dicts(i, kind);
            let dict = if i.mem.current_global() {
                category.global
            } else {
                category.local
            };
            i.mem.dict_put(dict, key, instance)?;
            i.mem.dict_set_access(instance, Access::ReadOnly)?;
        }
        Kind::Encoding | Kind::ProcSet | Kind::FontSet => {
            let wanted = match kind {
                Kind::ProcSet => Type::Dict,
                _ => Type::Array,
            };
            if instance.ty() != wanted
                && !(wanted == Type::Array && instance.ty() == Type::PackedArray)
            {
                return Err(VmError::TypeCheck);
            }
            if kind == Kind::Encoding && instance.length() != Some(256) {
                return Err(VmError::RangeCheck);
            }
            let key = i.mem.dict_key(key)?;
            let category = dicts(i, kind);
            let dict = if i.mem.current_global() {
                category.global
            } else {
                category.local
            };
            i.mem.dict_put(dict, key, instance)?;
        }
    }
    i.pop()?;
    i.pop()?;
    i.pop()?;
    i.push(instance)
}

fn undefineresource(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let key = i.peek(1)?;
    match kind {
        Kind::Font => font::undefine(i, key)?,
        Kind::Encoding | Kind::ProcSet | Kind::FontSet | Kind::CMap | Kind::CidFont => {
            let key = i.mem.dict_key(key)?;
            let category = dicts(i, kind);
            i.mem.dict_undef(category.local, key)?;
            i.mem.dict_undef(category.global, key)?;
        }
    }
    i.pop()?;
    i.pop()?;
    Ok(())
}

/// The instance names matching `template`: the program's definitions in
/// insertion order (local, then global, without repeats), then the
/// built-in instances in sorted order.
fn names(i: &mut Interp, kind: Kind, template: &[u8]) -> Result<Vec<Vec<u8>>, VmError> {
    let category = dicts(i, kind);
    let mut names: Vec<Vec<u8>> = Vec::new();
    for dict in [category.local, category.global] {
        for (key, _) in i.mem.dict_entries(dict)? {
            if let Some(atom) = key.as_name() {
                let text = i.mem.name_text(atom).to_vec();
                if !names.contains(&text) {
                    names.push(text);
                }
            }
        }
    }
    let builtins: Vec<&str> = match kind {
        Kind::Font => ResidentFace::ALL
            .iter()
            .map(|f| f.postscript_name())
            .collect(),
        Kind::Encoding => BUILTIN_ENCODINGS.to_vec(),
        Kind::ProcSet => BUILTIN_PROCSETS.to_vec(),
        Kind::CMap => cidinit::predefined_names(),
        Kind::FontSet | Kind::CidFont => Vec::new(),
    };
    for name in builtins {
        if !names.iter().any(|n| n == name.as_bytes()) {
            names.push(name.as_bytes().to_vec());
        }
    }
    names.retain(|name| matches(template, name));
    Ok(names)
}

/// Template matching per PLRM3 §3.9.2 `resourceforall`: `*` any run of
/// characters, `?` any one, `\` quoting the next.
pub(crate) fn matches(template: &[u8], name: &[u8]) -> bool {
    match template.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|k| matches(rest, &name[k..])),
        Some((b'?', rest)) => name
            .split_first()
            .is_some_and(|(_, tail)| matches(rest, tail)),
        Some((b'\\', rest)) => match (rest.split_first(), name.split_first()) {
            (Some((&quoted, rest)), Some((&first, tail))) => quoted == first && matches(rest, tail),
            (None, _) => name.is_empty(),
            _ => false,
        },
        Some((&literal, rest)) => name
            .split_first()
            .is_some_and(|(&first, tail)| first == literal && matches(rest, tail)),
    }
}

fn resourceforall(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let scratch = i.peek(1)?;
    let body = i.peek(2)?;
    let template = bytes(i, i.peek(3)?)?;
    if scratch.access().unwrap_or_default() != Access::Unlimited {
        return Err(VmError::InvalidAccess);
    }
    let names = names(i, kind, &template)?;
    for _ in 0..4 {
        i.pop()?;
    }
    i.push_frame(Frame::Loop(LoopFrame::ResourceForAll {
        body,
        names,
        scratch,
        next: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_glob() {
        assert!(matches(b"*", b""));
        assert!(matches(b"*", b"Helvetica"));
        assert!(matches(b"Times-*", b"Times-Roman"));
        assert!(!matches(b"Times-*", b"Helvetica"));
        assert!(matches(b"Times-?oman", b"Times-Roman"));
        assert!(!matches(b"Times-?oman", b"Times-Rroman"));
        assert!(matches(b"*Bold*", b"Helvetica-BoldOblique"));
        assert!(matches(b"\\*", b"*"));
        assert!(!matches(b"\\*", b"a"));
        assert!(matches(b"a\\", b"a"));
        assert!(!matches(b"", b"a"));
        assert!(matches(b"", b""));
    }
}
