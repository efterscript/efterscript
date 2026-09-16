// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Function dictionaries (PLRM3 §3.10.1): the sampled, exponential, and
//! stitching types read and checked into a [`FunctionSpec`] the boundary
//! carries, with the manual's defaults filled in. Nothing evaluates a
//! function here — shadings are the only use, and the viewer computes
//! their colours — so the checks are the dictionary's own: a missing
//! required entry is `undefined`, a wrongly typed one `typecheck`, a
//! value outside what the tables allow or a dimension that disagrees
//! with another `rangecheck`, and a sample source shorter than the table
//! needs `rangecheck` too. A sampled function's data is copied out of its
//! source once: a string as it is, a file from its start when it can be
//! positioned (a reusable stream, the manual's model) and otherwise from
//! where it stands.

use crate::error::VmError;
use crate::graphics::FunctionSpec;
use crate::interp::Interp;
use crate::object::{Access, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::file::file_operand;
use crate::ops::graphics::is_array;

/// How deeply stitching functions may nest before `limitcheck`.
const MAX_NESTING: usize = 8;

/// The most sample bytes one function carries; beyond it `limitcheck`.
const MAX_SAMPLE_BYTES: usize = 1 << 26;

const SAMPLE_DEPTHS: [i32; 8] = [1, 2, 4, 8, 12, 16, 24, 32];

pub(crate) fn entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<Object>, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)
}

/// The entry `key`, `undefined` when absent.
pub(crate) fn required(i: &mut Interp, dict: Object, key: &str) -> Result<Object, VmError> {
    entry(i, dict, key)?.ok_or(VmError::Undefined)
}

/// The elements of an array of numbers; `typecheck` for anything else.
pub(crate) fn numbers(i: &Interp, object: Object) -> Result<Vec<f32>, VmError> {
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    items(i, object)?
        .into_iter()
        .map(|o| o.as_number().ok_or(VmError::TypeCheck))
        .collect()
}

/// An optional array of exactly `len` numbers; `rangecheck` for another
/// length.
pub(crate) fn numbers_of(
    i: &mut Interp,
    dict: Object,
    key: &str,
    len: usize,
) -> Result<Option<Vec<f32>>, VmError> {
    match entry(i, dict, key)? {
        None => Ok(None),
        Some(object) => {
            let values = numbers(i, object)?;
            if values.len() != len {
                return Err(VmError::RangeCheck);
            }
            Ok(Some(values))
        }
    }
}

/// An integer entry; `typecheck` for another type.
pub(crate) fn integer(i: &mut Interp, dict: Object, key: &str) -> Result<Option<i32>, VmError> {
    match entry(i, dict, key)? {
        None => Ok(None),
        Some(object) => Ok(Some(object.as_i32().ok_or(VmError::TypeCheck)?)),
    }
}

/// Checks a `Domain` or `Range`: pairs of numbers, at least one, each
/// pair's first no greater than its second.
fn pairs(values: &[f32]) -> Result<(), VmError> {
    if values.is_empty() || !values.len().is_multiple_of(2) {
        return Err(VmError::RangeCheck);
    }
    if values
        .chunks(2)
        .any(|pair| pair[0] > pair[1] || pair[0].is_nan() || pair[1].is_nan())
    {
        return Err(VmError::RangeCheck);
    }
    Ok(())
}

/// Reads and checks a function dictionary taking `inputs` values and,
/// when `outputs` is given, producing that many.
pub fn read_function(
    i: &mut Interp,
    object: Object,
    inputs: usize,
    outputs: Option<usize>,
) -> Result<FunctionSpec, VmError> {
    read_nested(i, object, inputs, outputs, 0)
}

/// A shading's `Function` entry: one function of `outputs` outputs, or
/// an array of `outputs` one-output functions, each read with `inputs`.
pub fn read_function_or_array(
    i: &mut Interp,
    object: Object,
    inputs: usize,
    outputs: usize,
) -> Result<Vec<FunctionSpec>, VmError> {
    if object.ty() == Type::Dict {
        return Ok(vec![read_function(i, object, inputs, Some(outputs))?]);
    }
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let elements = items(i, object)?;
    if elements.len() != outputs {
        return Err(VmError::RangeCheck);
    }
    elements
        .into_iter()
        .map(|element| read_function(i, element, inputs, Some(1)))
        .collect()
}

fn read_nested(
    i: &mut Interp,
    object: Object,
    inputs: usize,
    outputs: Option<usize>,
    depth: usize,
) -> Result<FunctionSpec, VmError> {
    if depth > MAX_NESTING {
        return Err(VmError::LimitCheck);
    }
    if object.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    let kind = required(i, object, "FunctionType")?
        .as_i32()
        .ok_or(VmError::TypeCheck)?;
    let domain = required(i, object, "Domain")?;
    let domain = numbers(i, domain)?;
    pairs(&domain)?;
    if domain.len() != 2 * inputs {
        return Err(VmError::RangeCheck);
    }
    let range = match entry(i, object, "Range")? {
        Some(range) => {
            let range = numbers(i, range)?;
            pairs(&range)?;
            range
        }
        None => Vec::new(),
    };
    if let Some(n) = outputs
        && !range.is_empty()
        && range.len() != 2 * n
    {
        return Err(VmError::RangeCheck);
    }
    match kind {
        0 => read_sampled(i, object, domain, range, outputs),
        2 => read_exponential(i, object, domain, range, outputs),
        3 => read_stitching(i, object, domain, range, outputs, depth),
        _ => Err(VmError::RangeCheck),
    }
}

// Table 3.13.
fn read_sampled(
    i: &mut Interp,
    dict: Object,
    domain: Vec<f32>,
    range: Vec<f32>,
    outputs: Option<usize>,
) -> Result<FunctionSpec, VmError> {
    if range.is_empty() {
        return Err(VmError::Undefined);
    }
    let inputs = domain.len() / 2;
    let n = range.len() / 2;
    if let Some(expected) = outputs
        && n != expected
    {
        return Err(VmError::RangeCheck);
    }
    let size = required(i, dict, "Size")?;
    if !is_array(size) {
        return Err(VmError::TypeCheck);
    }
    let size: Vec<u32> = items(i, size)?
        .into_iter()
        .map(|o| {
            let n = o.as_i32().ok_or(VmError::TypeCheck)?;
            u32::try_from(n)
                .ok()
                .filter(|&n| n > 0)
                .ok_or(VmError::RangeCheck)
        })
        .collect::<Result<_, _>>()?;
    if size.len() != inputs {
        return Err(VmError::RangeCheck);
    }
    let bits = integer(i, dict, "BitsPerSample")?.ok_or(VmError::Undefined)?;
    if !SAMPLE_DEPTHS.contains(&bits) {
        return Err(VmError::RangeCheck);
    }
    let bits = bits as u8;
    let order = match integer(i, dict, "Order")? {
        None => 1,
        Some(order @ (1 | 3)) => order as u8,
        Some(_) => return Err(VmError::RangeCheck),
    };
    let encode = match numbers_of(i, dict, "Encode", 2 * inputs)? {
        Some(encode) => encode,
        None => size.iter().flat_map(|&s| [0.0, (s - 1) as f32]).collect(),
    };
    let decode = numbers_of(i, dict, "Decode", 2 * n)?.unwrap_or_else(|| range.clone());
    let source = required(i, dict, "DataSource")?;
    let samples = size
        .iter()
        .try_fold(1usize, |acc, &s| acc.checked_mul(s as usize))
        .and_then(|count| count.checked_mul(n))
        .and_then(|count| count.checked_mul(usize::from(bits)))
        .map(|bits| bits.div_ceil(8))
        .filter(|&count| count <= MAX_SAMPLE_BYTES)
        .ok_or(VmError::LimitCheck)?;
    let samples = read_samples(i, source, samples)?;
    Ok(FunctionSpec::Sampled {
        domain,
        range,
        size,
        bits,
        order,
        encode,
        decode,
        samples,
    })
}

/// `count` bytes of sample data from a string or a file; `rangecheck`
/// when the source holds fewer. A positionable file is read from its
/// start, any other file from where it is.
fn read_samples(i: &mut Interp, source: Object, count: usize) -> Result<Vec<u8>, VmError> {
    match source.ty() {
        Type::String => {
            let data = bytes(i, source)?;
            if data.len() < count {
                return Err(VmError::RangeCheck);
            }
            Ok(data[..count].to_vec())
        }
        Type::File => {
            let handle = file_operand(source, Access::ReadOnly)?;
            let files = i.mem.files_mut();
            if !files.is_open(handle) {
                return Err(VmError::IoError);
            }
            if files.is_positionable(handle) {
                files.set_file_position(handle, 0)?;
            }
            let mut data = vec![0u8; count];
            let mut filled = 0;
            while filled < count {
                let got = files.read(handle, &mut data[filled..])?;
                if got == 0 {
                    return Err(VmError::RangeCheck);
                }
                filled += got;
            }
            Ok(data)
        }
        _ => Err(VmError::TypeCheck),
    }
}

// Table 3.14.
fn read_exponential(
    i: &mut Interp,
    dict: Object,
    domain: Vec<f32>,
    range: Vec<f32>,
    outputs: Option<usize>,
) -> Result<FunctionSpec, VmError> {
    if domain.len() != 2 {
        return Err(VmError::RangeCheck);
    }
    let n = required(i, dict, "N")?
        .as_number()
        .ok_or(VmError::TypeCheck)?;
    let c0 = match entry(i, dict, "C0")? {
        Some(c0) => numbers(i, c0)?,
        None => vec![0.0],
    };
    let c1 = match entry(i, dict, "C1")? {
        Some(c1) => numbers(i, c1)?,
        None => vec![1.0],
    };
    if c0.is_empty() || c0.len() != c1.len() {
        return Err(VmError::RangeCheck);
    }
    if !range.is_empty() && range.len() != 2 * c0.len() {
        return Err(VmError::RangeCheck);
    }
    if let Some(expected) = outputs
        && c0.len() != expected
    {
        return Err(VmError::RangeCheck);
    }
    Ok(FunctionSpec::Exponential {
        domain,
        range,
        c0,
        c1,
        n,
    })
}

// Table 3.15.
fn read_stitching(
    i: &mut Interp,
    dict: Object,
    domain: Vec<f32>,
    range: Vec<f32>,
    outputs: Option<usize>,
    depth: usize,
) -> Result<FunctionSpec, VmError> {
    if domain.len() != 2 {
        return Err(VmError::RangeCheck);
    }
    let functions = required(i, dict, "Functions")?;
    if !is_array(functions) {
        return Err(VmError::TypeCheck);
    }
    let functions = items(i, functions)?;
    let k = functions.len();
    if k == 0 {
        return Err(VmError::RangeCheck);
    }
    let bounds = required(i, dict, "Bounds")?;
    let bounds = numbers(i, bounds)?;
    if bounds.len() != k - 1 {
        return Err(VmError::RangeCheck);
    }
    let encode = required(i, dict, "Encode")?;
    let encode = numbers(i, encode)?;
    if encode.len() != 2 * k {
        return Err(VmError::RangeCheck);
    }
    // The bounds partition the domain in increasing order, strictly
    // inside it.
    let mut last = domain[0];
    for &bound in &bounds {
        if bound <= last || bound.is_nan() {
            return Err(VmError::RangeCheck);
        }
        last = bound;
    }
    if k > 1 && domain[1] <= last {
        return Err(VmError::RangeCheck);
    }
    let mut expected = outputs.or((!range.is_empty()).then_some(range.len() / 2));
    let mut parts = Vec::with_capacity(k);
    for function in functions {
        let part = read_nested(i, function, 1, expected, depth + 1)?;
        expected = Some(part.outputs());
        parts.push(part);
    }
    Ok(FunctionSpec::Stitching {
        domain,
        range,
        functions: parts,
        bounds,
        encode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_must_be_ordered_and_present() {
        assert_eq!(pairs(&[0.0, 1.0]), Ok(()));
        assert_eq!(pairs(&[0.0, 0.0, -1.0, 1.0]), Ok(()));
        assert_eq!(pairs(&[]), Err(VmError::RangeCheck));
        assert_eq!(pairs(&[0.0]), Err(VmError::RangeCheck));
        assert_eq!(pairs(&[1.0, 0.0]), Err(VmError::RangeCheck));
        assert_eq!(pairs(&[0.0, f32::NAN]), Err(VmError::RangeCheck));
    }
}
