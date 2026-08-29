// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Type, attribute, and conversion operators (PLRM3 §8.2). Access only
//! ever tightens; for arrays, strings, and files it is a property of the
//! object, for dictionaries of the shared storage.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Access, Object, Type};
use crate::ops::output::{brief, format_real};
use crate::ops::{Num, array};
use crate::scanner::parse_number;

op_table! { OPS {
    "type" => type_, [Any];
    "cvlit" => cvlit, [Any];
    "cvx" => cvx, [Any];
    "xcheck" => xcheck, [Any];
    "executeonly" => executeonly, [Any];
    "readonly" => readonly, [Any];
    "noaccess" => noaccess, [Any];
    "rcheck" => rcheck, [Any];
    "wcheck" => wcheck, [Any];
    "cvi" => cvi, [Any];
    "cvr" => cvr, [Any];
    "cvn" => cvn, [String];
    "cvs" => cvs, [Any, String];
    "cvrs" => cvrs, [Num, Int, String];
}}

fn type_(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    let name = i.intern(object.ty().name());
    i.push(name)
}

fn cvlit(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    i.push(object.as_literal())
}

fn cvx(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    i.push(object.as_executable())
}

fn xcheck(i: &mut Interp) -> Result<(), VmError> {
    let object = i.pop()?;
    i.push(Object::boolean(object.is_executable()))
}

fn has_access(object: Object, dict_allowed: bool) -> Result<(), VmError> {
    match object.ty() {
        Type::Array | Type::PackedArray | Type::String | Type::File => Ok(()),
        Type::Dict if dict_allowed => Ok(()),
        _ => Err(VmError::TypeCheck),
    }
}

// Tightens the access of the object on top of the stack, replacing it.
fn restrict(i: &mut Interp, access: Access, dict_allowed: bool) -> Result<(), VmError> {
    let object = i.peek(0)?;
    has_access(object, dict_allowed)?;
    let result = if object.ty() == Type::Dict {
        i.mem.dict_set_access(object, access)?;
        object
    } else {
        if object.access().unwrap_or_default() > access {
            return Err(VmError::InvalidAccess);
        }
        object.with_access(access).expect("carries access")
    };
    i.pop()?;
    i.push(result)
}

fn executeonly(i: &mut Interp) -> Result<(), VmError> {
    restrict(i, Access::ExecuteOnly, false)
}

fn readonly(i: &mut Interp) -> Result<(), VmError> {
    restrict(i, Access::ReadOnly, true)
}

fn noaccess(i: &mut Interp) -> Result<(), VmError> {
    restrict(i, Access::None, true)
}

fn current_access(i: &Interp, object: Object) -> Result<Access, VmError> {
    has_access(object, true)?;
    match object.ty() {
        Type::Dict => i.mem.dict_access(object),
        _ => Ok(object.access().unwrap_or_default()),
    }
}

fn rcheck(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(0)?;
    let access = current_access(i, object)?;
    i.pop()?;
    i.push(Object::boolean(access <= Access::ReadOnly))
}

fn wcheck(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(0)?;
    let access = current_access(i, object)?;
    i.pop()?;
    i.push(Object::boolean(access == Access::Unlimited))
}

// The number a string denotes, read as the scanner would read one token.
fn number_of(i: &Interp, object: Object) -> Result<Num, VmError> {
    match Num::of(object) {
        Some(n) => Ok(n),
        None if object.ty() == Type::String => {
            let text = array::bytes(i, object)?;
            let word = text.trim_ascii();
            match parse_number(word) {
                Ok(Some(number)) => Ok(Num::of(number).expect("a number")),
                Ok(None) => Err(VmError::TypeCheck),
                Err(kind) => Err(crate::interp::scan_error(kind)),
            }
        }
        None => Err(VmError::TypeCheck),
    }
}

fn to_int(n: Num) -> Result<i32, VmError> {
    match n {
        Num::Int(v) => Ok(v),
        Num::Real(r) => {
            let t = r.trunc();
            if t.is_finite() && t >= i32::MIN as f32 && t < 2_147_483_648.0 {
                Ok(t as i32)
            } else {
                Err(VmError::RangeCheck)
            }
        }
    }
}

fn cvi(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(0)?;
    let value = to_int(number_of(i, object)?)?;
    i.pop()?;
    i.push(Object::integer(value))
}

fn cvr(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(0)?;
    let value = number_of(i, object)?.as_f32();
    i.pop()?;
    i.push(Object::real(value))
}

fn cvn(i: &mut Interp) -> Result<(), VmError> {
    let string = i.peek(0)?;
    let text = array::bytes(i, string)?;
    let name = i.mem.intern(&text)?.with_exec(string.is_executable());
    i.pop()?;
    i.push(name)
}

// Writes `text` into the front of the string on top of the stack and
// replaces the top `operands` objects with the filled interval.
fn fill(i: &mut Interp, text: &[u8], operands: usize) -> Result<(), VmError> {
    let string = i.peek(0)?;
    i.mem.string_put_bytes(string, 0, text)?;
    let filled = string
        .with_interval(0, u32::try_from(text.len()).expect("fits the string"))
        .expect("checked by put");
    for _ in 0..operands {
        i.pop()?;
    }
    i.push(filled)
}

fn cvs(i: &mut Interp) -> Result<(), VmError> {
    let object = i.peek(1)?;
    let text = brief(i, object);
    fill(i, &text, 2)
}

fn cvrs(i: &mut Interp) -> Result<(), VmError> {
    let radix = i.peek(1)?.as_i32().expect("integer");
    let number = Num::of(i.peek(2)?).expect("number");
    let text = match radix {
        10 => match number {
            Num::Int(v) => v.to_string(),
            Num::Real(r) => format_real(r),
        },
        2..=36 => {
            let mut value = to_int(number)? as u32;
            let radix = radix as u32;
            let mut digits = Vec::new();
            loop {
                let digit = u8::try_from(value % radix).expect("below the radix");
                digits.push(if digit < 10 {
                    b'0' + digit
                } else {
                    b'A' + digit - 10
                });
                value /= radix;
                if value == 0 {
                    break;
                }
            }
            digits.reverse();
            String::from_utf8(digits).expect("ascii digits")
        }
        _ => return Err(VmError::RangeCheck),
    };
    fill(i, text.as_bytes(), 3)
}
