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
//!
//! `ReusableStreamDecode` (PLRM3 §3.13.3) is the one filter that is not
//! a layer: the whole source — through the decode filters the parameter
//! dictionary's `Filter` and `DecodeParms` name, in order — is read when
//! the operator runs, and the result becomes a positionable in-memory
//! file. The read runs as a loop frame (`ReusableRead`) rather than
//! inside the operator, so a source that starves — the job's own stream
//! arriving in pieces, or a procedure that must run for its next string
//! — suspends the read where it is and resumes it, the way image data
//! from a procedure is collected. `AsyncRead` is accepted and ignored
//! (the read is always eager) and `Intent` is type-checked only.

use codec::predictor::Predictor;

use crate::decoders::Decoder;
use crate::encoders::Encoder;
use crate::error::VmError;
use crate::interp::{Frame, Interp, LoopFrame};
use crate::memory::Memory;
use crate::object::{Access, Handle, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::file::file_operand;
use crate::ops::graphics::is_array;

/// The most bytes a reusable stream holds; beyond it the read is
/// `limitcheck`, so a source without an end cannot exhaust memory.
const MAX_REUSABLE_BYTES: usize = 1 << 28;

/// Bytes read from the chain per step of a reusable read.
const READ_CHUNK: usize = 4096;

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
    Reusable,
}

fn lookup(name: &[u8]) -> Option<Name> {
    Some(match name {
        b"ReusableStreamDecode" => Name::Reusable,
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
            Name::Reusable => unreachable!("reusable streams read their own parameters"),
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

/// The parameters of a reusable stream (PLRM3 §3.13.3, Table 3.24): the
/// decode filters to apply to the source in order, each with its own
/// parameter dictionary, and whether closing the stream closes the
/// source. A `CloseSource` inside a pre-filter's dictionary is ignored,
/// as the table says.
struct ReusableParams {
    filters: Vec<(Decode, Option<Object>)>,
    close_source: bool,
}

impl ReusableParams {
    /// `Filter` is a name or an array of names of decode filters (an
    /// unknown name is `undefined`, an encode filter or a reusable
    /// stream `rangecheck`); `DecodeParms` is that filter's dictionary,
    /// or an array as long as `Filter` with `null` where a filter takes
    /// none. `AsyncRead` must be a boolean and `Intent` an integer.
    fn read(i: &mut Interp, dict: Option<Object>) -> Result<Self, VmError> {
        let mut params = ReusableParams {
            filters: Vec::new(),
            close_source: false,
        };
        let Some(dict) = dict else {
            return Ok(params);
        };
        params.close_source = bool_entry(i, dict, "CloseSource")?.unwrap_or(false);
        bool_entry(i, dict, "AsyncRead")?;
        int_entry(i, dict, "Intent")?;
        let names = match entry(i, dict, "Filter")? {
            None => Vec::new(),
            Some(name) if name.ty() == Type::Name => vec![name],
            Some(array) if is_array(array) => items(i, array)?,
            Some(_) => return Err(VmError::TypeCheck),
        };
        let parms = match entry(i, dict, "DecodeParms")? {
            None => vec![None; names.len()],
            Some(parms) if parms.ty() == Type::Dict => match names.len() {
                0 => Vec::new(),
                1 => vec![Some(parms)],
                _ => return Err(VmError::RangeCheck),
            },
            Some(array) if is_array(array) => {
                let elements = items(i, array)?;
                if elements.len() != names.len() {
                    return Err(VmError::RangeCheck);
                }
                elements
                    .into_iter()
                    .map(|element| match element.ty() {
                        Type::Null => Ok(None),
                        Type::Dict => Ok(Some(element)),
                        _ => Err(VmError::TypeCheck),
                    })
                    .collect::<Result<_, _>>()?
            }
            Some(_) => return Err(VmError::TypeCheck),
        };
        for (name, parms) in names.into_iter().zip(parms) {
            let atom = name.as_name().ok_or(VmError::TypeCheck)?;
            let kind = match lookup(i.mem.name_text(atom)) {
                Some(Name::Decode(kind)) => kind,
                Some(_) => return Err(VmError::RangeCheck),
                None => return Err(VmError::Undefined),
            };
            params.filters.push((kind, parms));
        }
        Ok(params)
    }
}

/// A reusable stream being read: the top of the pre-filter chain (or
/// the source itself), the entries the operator opened and will close
/// when the read ends, the source to close then if `CloseSource` asked
/// for it, and the bytes so far.
#[derive(Clone, Debug)]
pub struct ReusableRead {
    chain: Handle,
    owned: Vec<Handle>,
    close: Option<Handle>,
    data: Vec<u8>,
}

impl ReusableRead {
    /// Reads the next chunk; `true` when the chain has ended. A read
    /// that must wait fails as any other and is taken again after the
    /// rollback, which undoes the partial chunk. When the top of the
    /// chain ends, every layer under it that has not met its own
    /// end-of-data marker is read through it, as `closefile` on a chain
    /// does, so the source continues after the encoded data.
    pub(crate) fn step(&mut self, mem: &mut Memory) -> Result<bool, VmError> {
        let mut buf = [0u8; READ_CHUNK];
        let got = mem.files_mut().read(self.chain, &mut buf)?;
        if got == 0 {
            for &handle in self.owned.iter().rev() {
                if mem.files().ends_at_marker(handle) {
                    while mem.files_mut().read(handle, &mut buf)? > 0 {}
                }
            }
            return Ok(true);
        }
        if self.data.len() + got > MAX_REUSABLE_BYTES {
            return Err(VmError::LimitCheck);
        }
        self.data.extend_from_slice(&buf[..got]);
        Ok(false)
    }

    /// Closes the entries the read opened; called when its frame is
    /// discarded, whether the read finished or not.
    pub(crate) fn abandon(&self, mem: &mut Memory) {
        for &handle in self.owned.iter().rev() {
            let _ = mem.files_mut().close(handle);
        }
    }
}

/// The read has ended and its frame is gone: the source is closed if
/// asked, and the reusable stream over the bytes is the operator's
/// result.
pub(crate) fn finish_reusable(i: &mut Interp, read: ReusableRead) -> Result<(), VmError> {
    if let Some(source) = read.close {
        i.mem.files_mut().close(source)?;
    }
    let handle = i.mem.files_mut().open_reusable(read.data);
    let file = Object::file(i.mem.current_space(), handle)
        .with_access(Access::ReadOnly)
        .expect("file objects carry access");
    i.push(file)
}

/// Opens the source and the pre-filter chain over it and starts the
/// read; the operands are consumed and the result comes when the read
/// ends.
fn start_reusable(
    i: &mut Interp,
    source: Object,
    params: ReusableParams,
    operands: usize,
) -> Result<(), VmError> {
    let decoders = params
        .filters
        .iter()
        .map(|&(kind, dict)| Params::read(i, Name::Decode(kind), dict)?.decoder(kind))
        .collect::<Result<Vec<_>, _>>()?;
    let is_file = source.ty() == Type::File;
    let base = open_source(i, source)?;
    let mut owned = if is_file { Vec::new() } else { vec![base] };
    let mut chain = base;
    for decoder in decoders {
        chain = i.mem.files_mut().open_decoder(chain, decoder, false)?;
        owned.push(chain);
    }
    let read = ReusableRead {
        chain,
        owned,
        close: (is_file && params.close_source).then_some(base),
        data: Vec::new(),
    };
    for _ in 0..operands {
        i.pop()?;
    }
    i.push_frame(Frame::Loop(LoopFrame::ReusableRead(Box::new(read))))
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
    if name == Name::Reusable {
        let params = ReusableParams::read(i, operands.dict)?;
        return start_reusable(i, source, params, operands.source_at + 1);
    }
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
        Name::Reusable => unreachable!("started above"),
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
        assert_eq!(lookup(b"ReusableStreamDecode"), Some(Name::Reusable));
        let decode = FILTERS
            .iter()
            .filter(|n| matches!(lookup(n.as_bytes()), Some(Name::Decode(_))))
            .count();
        assert_eq!((decode, FILTERS.len() - decode), (7, 7));
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
