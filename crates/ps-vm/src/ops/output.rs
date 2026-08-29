// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Output operators writing to the injected standard output. This module
//! holds the minimum the error machinery and its scenarios need; the file
//! and output group completes it.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Type};

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
