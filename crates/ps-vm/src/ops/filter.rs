// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `filter` operator (PLRM3 §3.13): `source /Name filter` and
//! `source dict /Name filter` return a file that reads the named decode
//! filter's output over the source — a file, a string, or a procedure —
//! and `target /Name filter` (a writable file) one that writes the named
//! encode filter's output through to the target. The optional parameter
//! dictionary sits between the source or target and the name;
//! `SubFileDecode` also takes its two parameters positionally, as
//! `source count string /SubFileDecode filter`, and `RunLengthEncode`
//! requires its record length there, as `target [dict] length
//! /RunLengthEncode filter`. The decoders and
//! encoders themselves live in `decoders.rs` and `encoders.rs` and run
//! inside file-table entries, so a filtered file reads, peeks, executes,
//! writes, and suspends like any other.

use codec::predictor::Predictor;

use crate::decoders::Decoder;
use crate::encoders::Encoder;
use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Access, Handle, Object, Type};
use crate::ops::array::bytes;
use crate::ops::file::file_operand;

op_table! { OPS {
    "filter" => filter, [Name];
}}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decode {
    AsciiHex,
    Ascii85,
    RunLength,
    Flate,
    Lzw,
    SubFile,
    Dct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encode {
    AsciiHex,
    Ascii85,
    RunLength,
    Flate,
    Lzw,
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Name {
    Decode(Decode),
    Encode(Encode),
}

fn lookup(name: &[u8]) -> Option<Name> {
    Some(match name {
        b"ASCIIHexDecode" => Name::Decode(Decode::AsciiHex),
        b"ASCII85Decode" => Name::Decode(Decode::Ascii85),
        b"RunLengthDecode" => Name::Decode(Decode::RunLength),
        b"FlateDecode" => Name::Decode(Decode::Flate),
        b"LZWDecode" => Name::Decode(Decode::Lzw),
        b"SubFileDecode" => Name::Decode(Decode::SubFile),
        b"DCTDecode" => Name::Decode(Decode::Dct),
        b"ASCIIHexEncode" => Name::Encode(Encode::AsciiHex),
        b"ASCII85Encode" => Name::Encode(Encode::Ascii85),
        b"RunLengthEncode" => Name::Encode(Encode::RunLength),
        b"FlateEncode" => Name::Encode(Encode::Flate),
        b"LZWEncode" => Name::Encode(Encode::Lzw),
        b"NullEncode" => Name::Encode(Encode::Null),
        _ => return None,
    })
}

/// The parameters a filter honours, read from the dictionary operand
/// with the standard defaults.
#[derive(Clone, Debug)]
struct Params {
    predictor: i64,
    colors: i64,
    bits_per_component: i64,
    columns: i64,
    early_change: bool,
    eod_count: usize,
    eod_string: Vec<u8>,
    close_source: bool,
    close_target: bool,
    record_length: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            predictor: 1,
            colors: 1,
            bits_per_component: 8,
            columns: 1,
            early_change: true,
            eod_count: 0,
            eod_string: Vec::new(),
            close_source: false,
            close_target: false,
            record_length: 0,
        }
    }
}

fn entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<Object>, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)
}

fn int_entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<i64>, VmError> {
    match entry(i, dict, key)? {
        Some(value) => Ok(Some(i64::from(value.as_i32().ok_or(VmError::TypeCheck)?))),
        None => Ok(None),
    }
}

fn bool_entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<bool>, VmError> {
    match entry(i, dict, key)? {
        Some(value) => Ok(Some(value.as_bool().ok_or(VmError::TypeCheck)?)),
        None => Ok(None),
    }
}

impl Params {
    /// Reads the keys `name` uses; a wrong type is `typecheck`, a value
    /// out of range `rangecheck`, and other keys are ignored.
    fn read(i: &mut Interp, name: Name, dict: Option<Object>) -> Result<Self, VmError> {
        let mut params = Params::default();
        let Some(dict) = dict else {
            return Ok(params);
        };
        let kind = match name {
            Name::Decode(kind) => kind,
            Name::Encode(kind) => {
                if let Some(close) = bool_entry(i, dict, "CloseTarget")? {
                    params.close_target = close;
                }
                if kind == Encode::Lzw {
                    params.read_early_change(i, dict)?;
                }
                return Ok(params);
            }
        };
        if let Some(close) = bool_entry(i, dict, "CloseSource")? {
            params.close_source = close;
        }
        match kind {
            Decode::Flate | Decode::Lzw => {
                for (key, slot) in [
                    ("Predictor", &mut params.predictor),
                    ("Colors", &mut params.colors),
                    ("BitsPerComponent", &mut params.bits_per_component),
                    ("Columns", &mut params.columns),
                ] {
                    if let Some(value) = int_entry(i, dict, key)? {
                        *slot = value;
                    }
                }
                if kind == Decode::Lzw {
                    params.read_early_change(i, dict)?;
                }
            }
            Decode::SubFile => {
                if let Some(count) = int_entry(i, dict, "EODCount")? {
                    params.eod_count = usize::try_from(count).map_err(|_| VmError::RangeCheck)?;
                }
                if let Some(string) = entry(i, dict, "EODString")? {
                    params.eod_string = bytes(i, string)?;
                }
            }
            Decode::Dct => {
                // Parsed for type, unused: the samples are never decoded.
                if let Some(transform) = int_entry(i, dict, "ColorTransform")?
                    && !(0..=1).contains(&transform)
                {
                    return Err(VmError::RangeCheck);
                }
            }
            Decode::AsciiHex | Decode::Ascii85 | Decode::RunLength => {}
        }
        Ok(params)
    }

    fn read_early_change(&mut self, i: &mut Interp, dict: Object) -> Result<(), VmError> {
        if let Some(early) = int_entry(i, dict, "EarlyChange")? {
            self.early_change = match early {
                0 => false,
                1 => true,
                _ => return Err(VmError::RangeCheck),
            };
        }
        Ok(())
    }

    fn predictor(&self) -> Result<Option<Predictor>, VmError> {
        Predictor::new(
            self.predictor,
            self.colors,
            self.bits_per_component,
            self.columns,
        )
        .map_err(|_| VmError::RangeCheck)
    }

    fn decoder(&self, kind: Decode) -> Result<Decoder, VmError> {
        Ok(match kind {
            Decode::AsciiHex => Decoder::ascii_hex(),
            Decode::Ascii85 => Decoder::ascii85(),
            Decode::RunLength => Decoder::run_length(),
            Decode::Flate => Decoder::flate(self.predictor()?),
            Decode::Lzw => Decoder::lzw(self.early_change, self.predictor()?),
            Decode::SubFile => Decoder::sub_file(self.eod_count, self.eod_string.clone()),
            Decode::Dct => Decoder::Dct,
        })
    }

    fn encoder(&self, kind: Encode) -> Encoder {
        match kind {
            Encode::AsciiHex => Encoder::ascii_hex(),
            Encode::Ascii85 => Encoder::ascii85(),
            Encode::RunLength => Encoder::run_length(self.record_length),
            Encode::Flate => Encoder::flate(),
            Encode::Lzw => Encoder::lzw(self.early_change),
            Encode::Null => Encoder::Null,
        }
    }
}

/// The operands under the name: the run-length encoder's record length,
/// the parameter dictionary if any, the positional sub-file pair if
/// any, and the source's position.
struct Operands {
    record_length: Option<usize>,
    dict: Option<Object>,
    positional: Option<(usize, Vec<u8>)>,
    source_at: usize,
}

fn operands(i: &mut Interp, name: Name) -> Result<Operands, VmError> {
    let mut at = 1;
    let mut record_length = None;
    if name == Name::Encode(Encode::RunLength) {
        let length = i.peek(at)?.as_i32().ok_or(VmError::TypeCheck)?;
        record_length = Some(usize::try_from(length).map_err(|_| VmError::RangeCheck)?);
        at += 1;
    }
    let dict = match i.peek(at) {
        Ok(object) if object.ty() == Type::Dict => {
            at += 1;
            Some(object)
        }
        _ => None,
    };
    let mut positional = None;
    if name == Name::Decode(Decode::SubFile)
        && dict.is_none()
        && let Ok(string) = i.peek(at)
        && string.ty() == Type::String
        && let Ok(count) = i.peek(at + 1)
        && count.ty() == Type::Integer
        && i.peek(at + 2).is_ok()
    {
        let count =
            usize::try_from(count.as_i32().expect("integer")).map_err(|_| VmError::RangeCheck)?;
        positional = Some((count, bytes(i, string)?));
        at += 2;
    }
    Ok(Operands {
        record_length,
        dict,
        positional,
        source_at: at,
    })
}

/// Opens the base entry a decode layer reads: the file itself, a
/// one-shot stream over a string's bytes, or a procedure source.
fn open_source(i: &mut Interp, source: Object) -> Result<Handle, VmError> {
    match source.ty() {
        Type::File => {
            let handle = file_operand(source, Access::ReadOnly)?;
            if !i.mem.files().is_open(handle) {
                return Err(VmError::IoError);
            }
            Ok(handle)
        }
        Type::String => {
            let data = bytes(i, source)?;
            Ok(i.mem.files_mut().open_bytes(data))
        }
        Type::Array | Type::PackedArray if source.is_executable() => {
            if source.access().unwrap_or_default() > Access::ExecuteOnly {
                return Err(VmError::InvalidAccess);
            }
            Ok(i.mem.files_mut().open_procedure(source))
        }
        _ => Err(VmError::TypeCheck),
    }
}

/// The target an encode entry writes to: an open file with write
/// access.
fn open_target(i: &mut Interp, target: Object) -> Result<Handle, VmError> {
    let handle = file_operand(target, Access::Unlimited)?;
    if !i.mem.files().is_open(handle) {
        return Err(VmError::IoError);
    }
    Ok(handle)
}

fn filter(i: &mut Interp) -> Result<(), VmError> {
    let atom = i.peek(0)?.as_name().expect("name");
    let name = i.mem.name_text(atom).to_vec();
    let name = lookup(&name).ok_or(VmError::Undefined)?;
    let operands = operands(i, name)?;
    let source = i.peek(operands.source_at)?;
    let mut params = Params::read(i, name, operands.dict)?;
    if let Some((count, string)) = operands.positional {
        params.eod_count = count;
        params.eod_string = string;
    }
    if let Some(length) = operands.record_length {
        params.record_length = length;
    }
    // Everything is checked before an entry is opened.
    let (handle, access) = match name {
        Name::Decode(kind) => {
            let decoder = params.decoder(kind)?;
            let base = open_source(i, source)?;
            let handle = i
                .mem
                .files_mut()
                .open_decoder(base, decoder, params.close_source)?;
            (handle, Access::ReadOnly)
        }
        Name::Encode(kind) => {
            let encoder = params.encoder(kind);
            let target = open_target(i, source)?;
            let handle = i
                .mem
                .files_mut()
                .open_encoder(target, encoder, params.close_target)?;
            (handle, Access::Unlimited)
        }
    };
    let file = Object::file(i.mem.current_space(), handle)
        .with_access(access)
        .expect("file objects carry access");
    for _ in 0..=operands.source_at {
        i.pop()?;
    }
    i.push(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::resource::FILTERS;

    #[test]
    fn every_category_member_is_a_known_name_and_nothing_else_is() {
        for name in FILTERS {
            assert!(lookup(name.as_bytes()).is_some(), "{name}");
        }
        assert_eq!(lookup(b"NoSuchDecode"), None);
        assert_eq!(
            lookup(b"ASCIIHexDecode"),
            Some(Name::Decode(Decode::AsciiHex))
        );
        assert_eq!(lookup(b"NullEncode"), Some(Name::Encode(Encode::Null)));
        let decode = FILTERS
            .iter()
            .filter(|n| matches!(lookup(n.as_bytes()), Some(Name::Decode(_))))
            .count();
        assert_eq!((decode, FILTERS.len() - decode), (7, 6));
    }

    #[test]
    fn defaults_match_the_standard() {
        let p = Params::default();
        assert_eq!(p.predictor, 1);
        assert_eq!(p.colors, 1);
        assert_eq!(p.bits_per_component, 8);
        assert_eq!(p.columns, 1);
        assert!(p.early_change);
        assert!(p.predictor().unwrap().is_none());
        assert!(!p.close_source);
        assert!(!p.close_target);
        assert!(matches!(p.decoder(Decode::Dct), Ok(Decoder::Dct)));
        assert!(matches!(p.encoder(Encode::Null), Encoder::Null));
    }
}
