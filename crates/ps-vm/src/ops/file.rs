// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! File and output operators (PLRM3 §3.8, §8.2). The standard streams are
//! the ones the embedder injected; any other name goes to the file
//! capability, and without one `file` fails before touching the host.
//! Reads after `token` see exactly the bytes the scanner left, because the
//! file table owns the scanner's one byte of lookahead.

use crate::error::VmError;
use crate::interp::{Frame, Interp, Marker, SourceFrame, SourceSlot, lookup_in, scan_error};
use crate::memory::Memory;
use crate::names::Atom;
use crate::object::{Access, Handle, Object, Type};
use crate::ops::array::bytes;
use crate::ops::output::{brief, full};
use crate::scanner::{Scan, Scanner};
use crate::source::{FileSource, Source, StringSource};

op_table! { OPS {
    "file" => file, [String, String];
    "closefile" => closefile, [Any];
    "read" => read, [Any];
    "write" => write, [Any, Int];
    "readline" => readline, [Any, String];
    "readstring" => readstring, [Any, String];
    "readhexstring" => readhexstring, [Any, String];
    "writestring" => writestring, [Any, String];
    "token" => token, [Any];
    "currentfile" => currentfile;
    "flush" => flush;
    "flushfile" => flushfile, [Any];
    "==" => double_equals, [Any];
    "pstack" => pstack;
    "stack" => stack;
    "eexec" => eexec;
}}

pub(crate) fn file_operand(object: Object, needed: Access) -> Result<Handle, VmError> {
    if object.ty() != Type::File {
        return Err(VmError::TypeCheck);
    }
    if object.access().unwrap_or_default() > needed {
        return Err(VmError::InvalidAccess);
    }
    Ok(object.handle().expect("file is composite"))
}

fn file(i: &mut Interp) -> Result<(), VmError> {
    let mode = bytes(i, i.peek(0)?)?;
    let name = bytes(i, i.peek(1)?)?;
    let reading = mode.first() == Some(&b'r');
    let writing = matches!(mode.first(), Some(b'w' | b'a'));
    let object = match name.as_slice() {
        b"%stdin" | b"%stdout" | b"%stderr" => {
            let (standard, wanted) = match name.as_slice() {
                b"%stdin" => (i.stdin_file(), reading),
                b"%stdout" => (i.stdout_file(), writing),
                _ => (i.stderr_file(), writing),
            };
            let standard = standard.ok_or(VmError::UndefinedFileName)?;
            if !wanted {
                return Err(VmError::InvalidFileAccess);
            }
            standard
        }
        _ => {
            let opened = i.mem.open_file(&name, &mode)?;
            if reading {
                opened
                    .with_access(Access::ReadOnly)
                    .expect("file objects carry access")
            } else {
                opened
            }
        }
    };
    i.pop()?;
    i.pop()?;
    i.push(object)
}

/// `closefile` on a decode filter that has not reached its end-of-data
/// marker first reads through the marker, so the source continues after
/// it — the `currentfile … filter … closefile` idiom relies on that.
fn closefile(i: &mut Interp) -> Result<(), VmError> {
    let file = i.peek(0)?;
    if file.ty() == Type::File && file.eq(i.run_file()) {
        i.discard_run_input();
    } else {
        if file.ty() == Type::File {
            drain_to_marker(i, file)?;
        }
        i.mem.close_file(file)?;
    }
    i.pop()?;
    Ok(())
}

/// Reads a decode filter to its end-of-data marker if it has one and has
/// not reached it, and then each filter under it in a chain likewise, so
/// the source at the bottom continues after the last marker; anything
/// else is left alone.
pub(crate) fn drain_to_marker(i: &mut Interp, file: Object) -> Result<(), VmError> {
    let mut handle = file.handle().expect("file is composite");
    loop {
        if i.mem.files().ends_at_marker(handle) {
            let mut sink = [0u8; 256];
            while i.mem.files_mut().read(handle, &mut sink)? > 0 {}
        }
        match i.mem.files().layer_base(handle) {
            Some(base) if i.mem.files().is_filter(base) => handle = base,
            _ => return Ok(()),
        }
    }
}

fn read(i: &mut Interp) -> Result<(), VmError> {
    let file = i.peek(0)?;
    let mut byte = [0u8; 1];
    let n = i.mem.file_read(file, &mut byte)?;
    i.pop()?;
    if n == 1 {
        i.push(Object::integer(i32::from(byte[0])))?;
        i.push(Object::boolean(true))
    } else {
        i.push(Object::boolean(false))
    }
}

fn write(i: &mut Interp) -> Result<(), VmError> {
    let value = i.peek(0)?.as_i32().expect("integer");
    let file = i.peek(1)?;
    let byte = u8::try_from(value).map_err(|_| VmError::RangeCheck)?;
    i.write_file(file, &[byte])?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

// The common shape of the `read…` operators: a file and a string, the
// string filled by `fill`, which returns how much was filled and whether
// the read succeeded.
fn read_into(
    i: &mut Interp,
    fill: impl FnOnce(&mut Memory, Handle, &mut [u8]) -> Result<(usize, bool), VmError>,
) -> Result<(), VmError> {
    let string = i.peek(0)?;
    let file = i.peek(1)?;
    let handle = file_operand(file, Access::ReadOnly)?;
    if string.access().unwrap_or_default() != Access::Unlimited {
        return Err(VmError::InvalidAccess);
    }
    let len = string.length().expect("string") as usize;
    let mut buffer = vec![0u8; len];
    let (filled, ok) = fill(&mut i.mem, handle, &mut buffer)?;
    i.mem.string_put_bytes(string, 0, &buffer[..filled])?;
    let result = string
        .with_interval(0, u32::try_from(filled).expect("within the string"))
        .expect("within the string");
    i.pop()?;
    i.pop()?;
    i.push(result)?;
    i.push(Object::boolean(ok))
}

fn readline(i: &mut Interp) -> Result<(), VmError> {
    read_into(i, |mem, handle, buf| {
        let files = mem.files_mut();
        let mut n = 0;
        loop {
            let Some(byte) = files.consume(handle)? else {
                return Ok((n, false));
            };
            match byte {
                b'\n' => return Ok((n, true)),
                b'\r' => {
                    if files.peek(handle)? == Some(b'\n') {
                        files.consume(handle)?;
                    }
                    return Ok((n, true));
                }
                _ => {
                    if n == buf.len() {
                        return Err(VmError::RangeCheck);
                    }
                    buf[n] = byte;
                    n += 1;
                }
            }
        }
    })
}

fn readstring(i: &mut Interp) -> Result<(), VmError> {
    read_into(i, |mem, handle, buf| {
        if buf.is_empty() {
            return Err(VmError::RangeCheck);
        }
        let mut n = 0;
        while n < buf.len() {
            let got = mem.files_mut().read(handle, &mut buf[n..])?;
            if got == 0 {
                break;
            }
            n += got;
        }
        Ok((n, n == buf.len()))
    })
}

fn readhexstring(i: &mut Interp) -> Result<(), VmError> {
    read_into(i, |mem, handle, buf| {
        if buf.is_empty() {
            return Err(VmError::RangeCheck);
        }
        let files = mem.files_mut();
        let mut n = 0;
        let mut high: Option<u8> = None;
        while n < buf.len() {
            let Some(byte) = files.consume(handle)? else {
                break;
            };
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => continue,
            };
            match high.take() {
                None => high = Some(digit),
                Some(h) => {
                    buf[n] = h << 4 | digit;
                    n += 1;
                }
            }
        }
        if let Some(h) = high
            && n < buf.len()
        {
            buf[n] = h << 4;
            n += 1;
        }
        Ok((n, n == buf.len()))
    })
}

fn writestring(i: &mut Interp) -> Result<(), VmError> {
    let text = bytes(i, i.peek(0)?)?;
    let file = i.peek(1)?;
    i.write_file(file, &text)?;
    i.pop()?;
    i.pop()?;
    Ok(())
}

fn scan_one(i: &mut Interp, source: &mut dyn Source) -> Result<Option<Object>, VmError> {
    let dstack = &i.dstack;
    let mut resolver = |atom: Atom, mem: &mut Memory| lookup_in(dstack, mem, Object::name(atom));
    let mut scanner = Scanner::new();
    match scanner.next(source, &mut i.mem, &mut resolver) {
        Ok(Scan::Token { object, .. }) => Ok(Some(object)),
        Ok(Scan::End) => Ok(None),
        // The file may grow: the operator waits and scans again.
        Ok(Scan::NeedMore) => Err(VmError::NeedMore),
        Err(e) => Err(scan_error(e.kind)),
    }
}

fn token(i: &mut Interp) -> Result<(), VmError> {
    let source = i.peek(0)?;
    match source.ty() {
        Type::String => {
            bytes(i, source)?;
            let mut string_source = StringSource::new(source).expect("string");
            let scanned = scan_one(i, &mut string_source)?;
            i.pop()?;
            match scanned {
                Some(object) => {
                    i.push(string_source.remainder())?;
                    i.push(object)?;
                    i.push(Object::boolean(true))
                }
                None => i.push(Object::boolean(false)),
            }
        }
        Type::File => {
            file_operand(source, Access::ReadOnly)?;
            let mut file_source = FileSource::new(source).expect("file");
            let scanned = scan_one(i, &mut file_source)?;
            i.pop()?;
            match scanned {
                Some(object) => {
                    i.push(object)?;
                    i.push(Object::boolean(true))
                }
                None => i.push(Object::boolean(false)),
            }
        }
        _ => Err(VmError::TypeCheck),
    }
}

fn currentfile(i: &mut Interp) -> Result<(), VmError> {
    let file = i.current_file();
    i.push(file)
}

fn flush(i: &mut Interp) -> Result<(), VmError> {
    match i.stdout_file() {
        Some(file) => i.mem.flush_file(file),
        None => Ok(()),
    }
}

// On an input file the unread remainder is discarded, as the spec
// describes; on an output file buffered bytes are pushed out.
fn flushfile(i: &mut Interp) -> Result<(), VmError> {
    let file = i.peek(0)?;
    if file.ty() != Type::File {
        return Err(VmError::TypeCheck);
    }
    if file.access().unwrap_or_default() == Access::Unlimited {
        i.mem.flush_file(file)?;
    } else {
        let mut sink = [0u8; 256];
        while i.mem.file_read(file, &mut sink)? > 0 {}
    }
    i.pop()?;
    Ok(())
}

fn double_equals(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    let mut text = full(i, object);
    text.push(b'\n');
    i.write_stdout(&text)
}

fn dump(i: &mut Interp, format: fn(&Interp, Object) -> Vec<u8>) -> Result<(), VmError> {
    let mut text = Vec::new();
    for &object in i.ostack.iter().rev() {
        text.extend(format(i, object));
        text.push(b'\n');
    }
    i.write_stdout(&text)
}

fn pstack(i: &mut Interp) -> Result<(), VmError> {
    dump(i, full)
}

fn stack(i: &mut Interp) -> Result<(), VmError> {
    dump(i, brief)
}

/// `eexec` (PLRM3 §8.2): a file operand becomes a decrypting layer over
/// it, read from right after the operator's delimiter; a string operand
/// is decrypted whole. Either runs as a source with `systemdict` on the
/// dictionary stack under an `Eexec` marker that undoes the push and
/// closes the layer when the source ends by `closefile`, end of data, or
/// an error unwinding past it.
fn eexec(i: &mut Interp) -> Result<(), VmError> {
    let source = i.peek(0)?;
    match source.ty() {
        Type::File => {
            let base = file_operand(source, Access::ReadOnly)?;
            if !i.mem.files().is_open(base) {
                return Err(VmError::IoError);
            }
            let systemdict = i.dicts.systemdict;
            let dicts = i.dstack.len();
            i.push_dict(systemdict)?;
            let layer = i.mem.files_mut().open_layer(base)?;
            let object = Object::file(i.mem.current_space(), layer)
                .with_access(Access::ReadOnly)
                .expect("file objects carry access");
            i.pop()?;
            let slot = SourceSlot::File {
                object,
                source: FileSource::from_handle(layer),
            };
            begin_section(i, Some(layer), dicts, slot)
        }
        Type::String => {
            let cipher = bytes(i, source)?;
            let plain = ps_fonts::type1::decrypt_section(&cipher);
            let systemdict = i.dicts.systemdict;
            let dicts = i.dstack.len();
            i.push_dict(systemdict)?;
            let string = i.mem.alloc_string(plain);
            i.pop()?;
            let slot = SourceSlot::String(StringSource::new(string).expect("string"));
            begin_section(i, None, dicts, slot)
        }
        _ => Err(VmError::TypeCheck),
    }
}

fn begin_section(
    i: &mut Interp,
    layer: Option<Handle>,
    dicts: usize,
    slot: SourceSlot,
) -> Result<(), VmError> {
    i.push_frame_unchecked(Frame::Marker(Marker::Eexec { layer, dicts }));
    let frame = Frame::Source(Box::new(SourceFrame {
        slot,
        scanner: Scanner::new(),
    }));
    if let Err(e) = i.push_frame(frame) {
        i.pop_frame();
        return Err(e);
    }
    Ok(())
}
