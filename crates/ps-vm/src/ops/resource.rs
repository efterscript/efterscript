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
//! that, the loaded object living in global VM.
//!
//! The implicit categories (PLRM3 §3.9.4) — `FontType`, `FMapType`,
//! `Filter`, `ColorSpaceFamily`, `Category`, `Generic` — describe this
//! interpreter's own capabilities: their members are the tables below,
//! derived from what the code accepts, and are not the program's to
//! change. Other categories are `undefined`.

use ps_fonts::ResidentFace;

use crate::error::VmError;
use crate::interp::{Category, Frame, Interp, LoopFrame, ResourceKey};
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
    Implicit(Implicit),
}

/// The implicit categories, whose instances are their keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Implicit {
    FontType,
    FMapType,
    Filter,
    ColorSpaceFamily,
    Category,
    Generic,
}

/// The font types `definefont` accepts: 0 (with map type 9 and a CMap),
/// 1, 2 (loaded through a FontSet, definable by hand), 3, and 42.
pub(crate) const FONT_TYPES: [i32; 5] = [0, 1, 2, 3, 42];
pub(crate) const FMAP_TYPES: [i32; 1] = [9];
/// No filters exist yet, so the category is empty until they do.
pub(crate) const FILTERS: [&str; 0] = [];
/// The families the colour boundary carries, sorted.
pub(crate) const COLOR_SPACE_FAMILIES: [&str; 6] = [
    "DeviceCMYK",
    "DeviceGray",
    "DeviceN",
    "DeviceRGB",
    "Indexed",
    "Separation",
];
/// Every category name, the implicit ones included, sorted.
pub(crate) const CATEGORIES: [&str; 12] = [
    "CIDFont",
    "CMap",
    "Category",
    "ColorSpaceFamily",
    "Encoding",
    "FMapType",
    "Filter",
    "Font",
    "FontSet",
    "FontType",
    "Generic",
    "ProcSet",
];

impl Implicit {
    fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"FontType" => Implicit::FontType,
            b"FMapType" => Implicit::FMapType,
            b"Filter" => Implicit::Filter,
            b"ColorSpaceFamily" => Implicit::ColorSpaceFamily,
            b"Category" => Implicit::Category,
            b"Generic" => Implicit::Generic,
            _ => return None,
        })
    }

    /// The members, integers or names, in enumeration order.
    pub(crate) fn members(self) -> Vec<ResourceKey> {
        let names = |list: &[&str]| {
            list.iter()
                .map(|n| ResourceKey::Name(n.as_bytes().to_vec()))
                .collect()
        };
        match self {
            Implicit::FontType => FONT_TYPES.iter().map(|&t| ResourceKey::Int(t)).collect(),
            Implicit::FMapType => FMAP_TYPES.iter().map(|&t| ResourceKey::Int(t)).collect(),
            Implicit::Filter => names(&FILTERS),
            Implicit::ColorSpaceFamily => names(&COLOR_SPACE_FAMILIES),
            Implicit::Category => names(&CATEGORIES),
            Implicit::Generic => Vec::new(),
        }
    }

    /// Whether `key` (an integer or a name) is a member; keys of other
    /// types never are.
    fn has(self, i: &Interp, key: Object) -> bool {
        let key = match key.ty() {
            Type::Integer => ResourceKey::Int(key.as_i32().expect("integer")),
            Type::Name => ResourceKey::Name(i.mem.name_text(key.as_name().expect("name")).to_vec()),
            _ => return false,
        };
        self.members().contains(&key)
    }
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
        name => Implicit::from_name(name)
            .map(Kind::Implicit)
            .ok_or(VmError::Undefined),
    }
}

/// The instance dictionaries of a category the program may define into.
fn dicts(i: &Interp, kind: Kind) -> Option<Category> {
    Some(match kind {
        Kind::Font => i.font_category,
        Kind::Encoding => i.encoding_category,
        Kind::ProcSet => i.procset_category,
        Kind::FontSet => i.fontset_category,
        Kind::CMap => i.cmap_category,
        Kind::CidFont => i.cidfont_category,
        Kind::Implicit(_) => return None,
    })
}

/// An instance the program defined, local VM first.
fn defined(i: &mut Interp, kind: Kind, key: Object) -> Result<Option<Object>, VmError> {
    let Some(category) = dicts(i, kind) else {
        return Ok(None);
    };
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
        Kind::FontSet | Kind::CMap | Kind::CidFont | Kind::Implicit(_) => Ok(None),
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
        Kind::Encoding | Kind::FontSet | Kind::CidFont | Kind::Implicit(_) => false,
    }
}

fn has_builtin(kind: Kind, name: &[u8]) -> bool {
    match kind {
        Kind::Font => ResidentFace::from_postscript_name(name).is_some(),
        Kind::Encoding => BUILTIN_ENCODINGS.iter().any(|e| e.as_bytes() == name),
        Kind::ProcSet => BUILTIN_PROCSETS.iter().any(|p| p.as_bytes() == name),
        Kind::CMap => cidinit::is_predefined(name),
        Kind::FontSet | Kind::CidFont | Kind::Implicit(_) => false,
    }
}

fn findresource(i: &mut Interp) -> Result<(), VmError> {
    let kind = kind(i, i.peek(0)?)?;
    let key = i.peek(1)?;
    let key = i.mem.dict_key(key)?;
    let found = match defined(i, kind, key)? {
        Some(instance) => instance,
        // An implicit resource has no instance object: the key is it.
        None if matches!(kind, Kind::Implicit(c) if c.has(i, key)) => key,
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
    } else if let Kind::Implicit(category) = kind {
        category.has(i, key).then_some(STATUS_DEFINED)
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
        Kind::Implicit(_) => return Err(VmError::InvalidAccess),
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
            let category = dicts(i, kind).expect("a defined category");
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
            let category = dicts(i, kind).expect("a defined category");
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
        Kind::Implicit(_) => return Err(VmError::InvalidAccess),
        Kind::Font => font::undefine(i, key)?,
        Kind::Encoding | Kind::ProcSet | Kind::FontSet | Kind::CMap | Kind::CidFont => {
            let key = i.mem.dict_key(key)?;
            let category = dicts(i, kind).expect("a defined category");
            i.mem.dict_undef(category.local, key)?;
            i.mem.dict_undef(category.global, key)?;
        }
    }
    i.pop()?;
    i.pop()?;
    Ok(())
}

/// The instance keys matching `template`: the program's definitions in
/// insertion order (local, then global, without repeats), then the
/// built-in instances in sorted order. An integer key has no text for
/// the template to match and is always listed.
fn keys(i: &mut Interp, kind: Kind, template: &[u8]) -> Result<Vec<ResourceKey>, VmError> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    if let Some(category) = dicts(i, kind) {
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
        Kind::Implicit(category) => {
            return Ok(category
                .members()
                .into_iter()
                .filter(|key| match key {
                    ResourceKey::Name(name) => matches(template, name),
                    ResourceKey::Int(_) => true,
                })
                .collect());
        }
    };
    for name in builtins {
        if !names.iter().any(|n| n == name.as_bytes()) {
            names.push(name.as_bytes().to_vec());
        }
    }
    names.retain(|name| matches(template, name));
    Ok(names.into_iter().map(ResourceKey::Name).collect())
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
    let keys = keys(i, kind, &template)?;
    for _ in 0..4 {
        i.pop()?;
    }
    i.push_frame(Frame::Loop(LoopFrame::ResourceForAll {
        body,
        keys,
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
