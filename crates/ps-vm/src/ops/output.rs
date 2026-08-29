// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Output operators writing to the injected standard output. This module
//! holds the minimum the error machinery and its scenarios need; the file
//! and output group completes it.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Access, Object, Type};

op_table! { OPS {
    "print" => print, [String];
    "=" => equals, [Any];
}}

fn print(i: &mut Interp) -> Result<(), VmError> {
    let string = i.peek(0)?;
    let bytes = i.mem.string(string).ok_or(VmError::InvalidAccess)?.to_vec();
    i.write_stdout(&bytes)?;
    i.pop()?;
    Ok(())
}

fn equals(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    let mut text = brief(i, object);
    text.push(b'\n');
    i.write_stdout(&text)
}

/// The text `=` writes for an object: numbers, booleans, names, and
/// operators by value, strings by content, everything else as
/// `--nostringval--`.
pub fn brief(i: &Interp, object: Object) -> Vec<u8> {
    match object.ty() {
        Type::Integer => object.as_i32().expect("integer").to_string().into_bytes(),
        Type::Real => format_real(object.as_f32().expect("real")).into_bytes(),
        Type::Boolean => if object.as_bool().expect("boolean") {
            "true"
        } else {
            "false"
        }
        .as_bytes()
        .to_vec(),
        Type::Name => i.mem.name_text(object.as_name().expect("name")).to_vec(),
        Type::Operator => i
            .ops
            .get(object.as_operator().expect("operator") as usize)
            .map_or_else(
                || b"--nostringval--".to_vec(),
                |e| e.name.as_bytes().to_vec(),
            ),
        Type::String => i.mem.string(object).map(<[u8]>::to_vec).unwrap_or_default(),
        _ => b"--nostringval--".to_vec(),
    }
}

/// Nesting beyond this prints `...` instead of descending, so a self-
/// containing array terminates and the host stack stays bounded.
const MAX_PRINT_DEPTH: usize = 64;

/// The text `==` writes for an object: its syntactic form where one
/// exists, a bracketed type name otherwise.
pub fn full(i: &Interp, object: Object) -> Vec<u8> {
    let mut out = Vec::new();
    write_full(i, object, &mut out, 0);
    out
}

fn write_full(i: &Interp, object: Object, out: &mut Vec<u8>, depth: usize) {
    match object.ty() {
        Type::Name => {
            if object.is_literal() {
                out.push(b'/');
            }
            out.extend(i.mem.name_text(object.as_name().expect("name")));
        }
        Type::String => match i.mem.string(object) {
            Some(bytes) if object.access().unwrap_or_default() <= Access::ReadOnly => {
                out.push(b'(');
                for &b in bytes {
                    match b {
                        b'(' | b')' | b'\\' => out.extend([b'\\', b]),
                        b'\n' => out.extend(b"\\n"),
                        b'\r' => out.extend(b"\\r"),
                        b'\t' => out.extend(b"\\t"),
                        0x08 => out.extend(b"\\b"),
                        0x0C => out.extend(b"\\f"),
                        0x20..=0x7E => out.push(b),
                        _ => out.extend(format!("\\{b:03o}").into_bytes()),
                    }
                }
                out.push(b')');
            }
            _ => out.extend(b"--nostringval--"),
        },
        Type::Array | Type::PackedArray => {
            if depth >= MAX_PRINT_DEPTH {
                out.extend(b"...");
                return;
            }
            match i.mem.array(object) {
                Some(items) if object.access().unwrap_or_default() <= Access::ReadOnly => {
                    let (open, close) = if object.is_executable() {
                        (b'{', b'}')
                    } else {
                        (b'[', b']')
                    };
                    out.push(open);
                    for (k, &item) in items.iter().enumerate() {
                        if k > 0 {
                            out.push(b' ');
                        }
                        write_full(i, item, out, depth + 1);
                    }
                    out.push(close);
                }
                _ => out.extend(b"--nostringval--"),
            }
        }
        Type::Operator => {
            out.extend(b"--");
            out.extend(brief(i, object));
            out.extend(b"--");
        }
        Type::Dict => out.extend(b"-dict-"),
        Type::File => out.extend(b"-file-"),
        Type::Mark => out.extend(b"-mark-"),
        Type::Null => out.extend(b"-null-"),
        Type::Save => out.extend(b"-save-"),
        Type::FontId => out.extend(b"-fontID-"),
        Type::GState => out.extend(b"-gstate-"),
        Type::Integer | Type::Real | Type::Boolean => out.extend(brief(i, object)),
    }
}

/// A real in the form `=` prints: integral values keep one decimal.
pub fn format_real(value: f32) -> String {
    if value.is_finite() && value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_print_with_a_decimal_point() {
        assert_eq!(format_real(5.0), "5.0");
        assert_eq!(format_real(-0.5), "-0.5");
        assert_eq!(format_real(2147483648.0), "2147483648.0");
        assert_eq!(format_real(0.001), "0.001");
        assert_eq!(format_real(1e10), "10000000000.0");
        assert_eq!(format_real(1e20), "100000000000000000000");
    }
}
