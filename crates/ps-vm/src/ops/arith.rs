// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Arithmetic, relational, boolean, and bitwise operators (PLRM3 §8.2).
//! Integer results that do not fit become reals; real results that are not
//! finite are `undefinedresult`.

use std::cmp::Ordering;

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Type};
use crate::ops::Num;

op_table! { OPS {
    "add" => add, [Num, Num];
    "sub" => sub, [Num, Num];
    "mul" => mul, [Num, Num];
    "div" => div, [Num, Num];
    "idiv" => idiv, [Int, Int];
    "mod" => modulo, [Int, Int];
    "neg" => neg, [Num];
    "abs" => abs, [Num];
    "sqrt" => sqrt, [Num];
    "atan" => atan, [Num, Num];
    "cos" => cos, [Num];
    "sin" => sin, [Num];
    "exp" => exp, [Num, Num];
    "ln" => ln, [Num];
    "log" => log, [Num];
    "ceiling" => ceiling, [Num];
    "floor" => floor, [Num];
    "round" => round, [Num];
    "truncate" => truncate, [Num];
    "eq" => eq, [Any, Any];
    "ne" => ne, [Any, Any];
    "gt" => gt, [Any, Any];
    "ge" => ge, [Any, Any];
    "lt" => lt, [Any, Any];
    "le" => le, [Any, Any];
    "and" => and, [Any, Any];
    "or" => or, [Any, Any];
    "xor" => xor, [Any, Any];
    "not" => not, [Any];
    "bitshift" => bitshift, [Int, Int];
}}

fn real(value: f32) -> Result<Num, VmError> {
    if value.is_finite() {
        Ok(Num::Real(value))
    } else {
        Err(VmError::UndefinedResult)
    }
}

// Operands stay on the stack when the operation fails.
fn operand(i: &Interp, n: usize) -> Result<Num, VmError> {
    Num::of(i.peek(n)?).ok_or(VmError::TypeCheck)
}

fn binary(i: &mut Interp, f: fn(Num, Num) -> Result<Num, VmError>) -> Result<(), VmError> {
    let b = operand(i, 0)?;
    let a = operand(i, 1)?;
    let result = f(a, b)?;
    i.pop()?;
    i.pop()?;
    i.push(result.to_object())
}

fn unary(i: &mut Interp, f: fn(Num) -> Result<Num, VmError>) -> Result<(), VmError> {
    let a = operand(i, 0)?;
    let result = f(a)?;
    i.pop()?;
    i.push(result.to_object())
}

fn real_unary(i: &mut Interp, f: fn(f32) -> Result<f32, VmError>) -> Result<(), VmError> {
    let a = operand(i, 0)?;
    let result = real(f(a.as_f32())?)?;
    i.pop()?;
    i.push(result.to_object())
}

fn add(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| match (a, b) {
        (Num::Int(x), Num::Int(y)) => Ok(x
            .checked_add(y)
            .map_or(Num::Real((i64::from(x) + i64::from(y)) as f32), Num::Int)),
        _ => real(a.as_f32() + b.as_f32()),
    })
}

fn sub(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| match (a, b) {
        (Num::Int(x), Num::Int(y)) => Ok(x
            .checked_sub(y)
            .map_or(Num::Real((i64::from(x) - i64::from(y)) as f32), Num::Int)),
        _ => real(a.as_f32() - b.as_f32()),
    })
}

fn mul(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| match (a, b) {
        (Num::Int(x), Num::Int(y)) => Ok(x
            .checked_mul(y)
            .map_or(Num::Real((i64::from(x) * i64::from(y)) as f32), Num::Int)),
        _ => real(a.as_f32() * b.as_f32()),
    })
}

fn div(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| {
        if b.as_f32() == 0.0 {
            return Err(VmError::UndefinedResult);
        }
        real(a.as_f32() / b.as_f32())
    })
}

fn idiv(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| match (a, b) {
        (Num::Int(x), Num::Int(y)) => x
            .checked_div(y)
            .map(Num::Int)
            .ok_or(VmError::UndefinedResult),
        _ => Err(VmError::TypeCheck),
    })
}

fn modulo(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |a, b| match (a, b) {
        (Num::Int(_), Num::Int(0)) => Err(VmError::UndefinedResult),
        (Num::Int(x), Num::Int(y)) => Ok(Num::Int(x.wrapping_rem(y))),
        _ => Err(VmError::TypeCheck),
    })
}

fn neg(i: &mut Interp) -> Result<(), VmError> {
    unary(i, |a| match a {
        Num::Int(x) => Ok(x.checked_neg().map_or(Num::Real(-(x as f32)), Num::Int)),
        Num::Real(r) => Ok(Num::Real(-r)),
    })
}

fn abs(i: &mut Interp) -> Result<(), VmError> {
    unary(i, |a| match a {
        Num::Int(x) => Ok(x.checked_abs().map_or(Num::Real(-(x as f32)), Num::Int)),
        Num::Real(r) => Ok(Num::Real(r.abs())),
    })
}

fn sqrt(i: &mut Interp) -> Result<(), VmError> {
    real_unary(i, |v| {
        if v < 0.0 {
            Err(VmError::RangeCheck)
        } else {
            Ok(v.sqrt())
        }
    })
}

fn atan(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |num, den| {
        let (num, den) = (num.as_f32(), den.as_f32());
        if num == 0.0 && den == 0.0 {
            return Err(VmError::UndefinedResult);
        }
        let mut degrees = num.atan2(den).to_degrees();
        if degrees < 0.0 {
            degrees += 360.0;
        }
        real(degrees)
    })
}

/// Sine and cosine of an angle in degrees, reduced modulo 360 in double
/// precision before the conversion to radians so a large angle keeps
/// single precision's digits, and exact at the quarter turns.
fn sin_cos_degrees(degrees: f32) -> (f64, f64) {
    let turn = f64::from(degrees).rem_euclid(360.0);
    if turn == 0.0 {
        (0.0, 1.0)
    } else if turn == 90.0 {
        (1.0, 0.0)
    } else if turn == 180.0 {
        (0.0, -1.0)
    } else if turn == 270.0 {
        (-1.0, 0.0)
    } else {
        turn.to_radians().sin_cos()
    }
}

fn cos(i: &mut Interp) -> Result<(), VmError> {
    real_unary(i, |v| Ok(sin_cos_degrees(v).1 as f32))
}

fn sin(i: &mut Interp) -> Result<(), VmError> {
    real_unary(i, |v| Ok(sin_cos_degrees(v).0 as f32))
}

fn exp(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |base, exponent| {
        real(base.as_f32().powf(exponent.as_f32()))
    })
}

fn ln(i: &mut Interp) -> Result<(), VmError> {
    real_unary(i, |v| {
        if v <= 0.0 {
            Err(VmError::RangeCheck)
        } else {
            Ok(v.ln())
        }
    })
}

fn log(i: &mut Interp) -> Result<(), VmError> {
    real_unary(i, |v| {
        if v <= 0.0 {
            Err(VmError::RangeCheck)
        } else {
            Ok(v.log10())
        }
    })
}

fn rounding(i: &mut Interp, f: fn(f32) -> f32) -> Result<(), VmError> {
    let a = operand(i, 0)?;
    let result = match a {
        Num::Int(_) => a,
        Num::Real(r) => Num::Real(f(r)),
    };
    i.pop()?;
    i.push(result.to_object())
}

fn ceiling(i: &mut Interp) -> Result<(), VmError> {
    rounding(i, f32::ceil)
}

fn floor(i: &mut Interp) -> Result<(), VmError> {
    rounding(i, f32::floor)
}

// Halfway cases go to the greater integer.
fn round(i: &mut Interp) -> Result<(), VmError> {
    rounding(i, |r| (r + 0.5).floor())
}

fn truncate(i: &mut Interp) -> Result<(), VmError> {
    rounding(i, f32::trunc)
}

// --- relational ----------------------------------------------------------------

/// The `eq` operator's relation: `Object::eq` plus string contents, and a
/// string against a name by text.
pub(crate) fn objects_equal(i: &Interp, a: Object, b: Object) -> bool {
    let text = |o: Object| match o.ty() {
        Type::String => i.mem.string(o),
        Type::Name => Some(i.mem.name_text(o.as_name().expect("name"))),
        _ => None,
    };
    match (a.ty(), b.ty()) {
        (Type::String, Type::String) | (Type::String, Type::Name) | (Type::Name, Type::String) => {
            match (text(a), text(b)) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            }
        }
        _ => a.eq(b),
    }
}

fn eq(i: &mut Interp) -> Result<(), VmError> {
    let b = i.pop()?;
    let a = i.pop()?;
    let equal = objects_equal(i, a, b);
    i.push(Object::boolean(equal))
}

fn ne(i: &mut Interp) -> Result<(), VmError> {
    let b = i.pop()?;
    let a = i.pop()?;
    let equal = objects_equal(i, a, b);
    i.push(Object::boolean(!equal))
}

// `None` when the operands are numbers that do not order (NaN).
fn compare(i: &Interp, a: Object, b: Object) -> Result<Option<Ordering>, VmError> {
    match (Num::of(a), Num::of(b)) {
        (Some(Num::Int(x)), Some(Num::Int(y))) => Ok(Some(x.cmp(&y))),
        (Some(x), Some(y)) => Ok(x.as_f32().partial_cmp(&y.as_f32())),
        _ if a.ty() == Type::String && b.ty() == Type::String => {
            let x = i.mem.string(a).ok_or(VmError::InvalidAccess)?;
            let y = i.mem.string(b).ok_or(VmError::InvalidAccess)?;
            Ok(Some(x.cmp(y)))
        }
        _ => Err(VmError::TypeCheck),
    }
}

fn relation(i: &mut Interp, holds: fn(Ordering) -> bool) -> Result<(), VmError> {
    let b = i.peek(0)?;
    let a = i.peek(1)?;
    let result = compare(i, a, b)?.is_some_and(holds);
    i.pop()?;
    i.pop()?;
    i.push(Object::boolean(result))
}

fn gt(i: &mut Interp) -> Result<(), VmError> {
    relation(i, Ordering::is_gt)
}

fn ge(i: &mut Interp) -> Result<(), VmError> {
    relation(i, Ordering::is_ge)
}

fn lt(i: &mut Interp) -> Result<(), VmError> {
    relation(i, Ordering::is_lt)
}

fn le(i: &mut Interp) -> Result<(), VmError> {
    relation(i, Ordering::is_le)
}

// --- boolean and bitwise --------------------------------------------------------

fn bitwise(
    i: &mut Interp,
    on_bool: fn(bool, bool) -> bool,
    on_int: fn(i32, i32) -> i32,
) -> Result<(), VmError> {
    let b = i.peek(0)?;
    let a = i.peek(1)?;
    let result = match (a.ty(), b.ty()) {
        (Type::Boolean, Type::Boolean) => Object::boolean(on_bool(
            a.as_bool().expect("boolean"),
            b.as_bool().expect("boolean"),
        )),
        (Type::Integer, Type::Integer) => Object::integer(on_int(
            a.as_i32().expect("integer"),
            b.as_i32().expect("integer"),
        )),
        _ => return Err(VmError::TypeCheck),
    };
    i.pop()?;
    i.pop()?;
    i.push(result)
}

fn and(i: &mut Interp) -> Result<(), VmError> {
    bitwise(i, |a, b| a & b, |a, b| a & b)
}

fn or(i: &mut Interp) -> Result<(), VmError> {
    bitwise(i, |a, b| a | b, |a, b| a | b)
}

fn xor(i: &mut Interp) -> Result<(), VmError> {
    bitwise(i, |a, b| a ^ b, |a, b| a ^ b)
}

fn not(i: &mut Interp) -> Result<(), VmError> {
    let a = i.peek(0)?;
    let result = match a.ty() {
        Type::Boolean => Object::boolean(!a.as_bool().expect("boolean")),
        Type::Integer => Object::integer(!a.as_i32().expect("integer")),
        _ => return Err(VmError::TypeCheck),
    };
    i.pop()?;
    i.push(result)
}

// Shifts are logical on the 32-bit pattern; a shift of 32 or more clears it.
fn bitshift(i: &mut Interp) -> Result<(), VmError> {
    binary(i, |value, shift| match (value, shift) {
        (Num::Int(value), Num::Int(shift)) => {
            let value = value as u32;
            let result = if shift >= 0 {
                value.checked_shl(shift as u32).unwrap_or(0)
            } else {
                value.checked_shr(shift.unsigned_abs()).unwrap_or(0)
            };
            Ok(Num::Int(result as i32))
        }
        _ => Err(VmError::TypeCheck),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_are_reduced_before_conversion() {
        // A million degrees is 280 degrees; the true cosine is
        // 0.173648178, and the narrowed result must be its nearest
        // single-precision value.
        let (sin, cos) = sin_cos_degrees(1_000_000.0);
        assert_eq!(cos as f32, 0.173_648_18);
        assert_eq!(sin as f32, -0.984_807_753_f64 as f32);
        assert_eq!(sin_cos_degrees(-168_437.0), sin_cos_degrees(43.0));
        assert_eq!(sin_cos_degrees(90.0), (1.0, 0.0));
        assert_eq!(sin_cos_degrees(-90.0), (-1.0, 0.0));
        assert_eq!(sin_cos_degrees(180.0), (0.0, -1.0));
        assert_eq!(sin_cos_degrees(720.0), (0.0, 1.0));
        assert_eq!(sin_cos_degrees(45.0).0 as f32, 0.707_106_77);
    }
}
