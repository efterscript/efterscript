// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! CIE-based colour spaces (PLRM3 §4.8.3) and the colour-rendering
//! operators (§7.1). The four family dictionaries are read and checked
//! into a `CieSpace` the VM keeps beside the boundary colour: the
//! backend only ever sees one of the PDF calibrated spaces (ISO 32000-1
//! §8.6.5). A space whose two stages amount to one gamma-and-matrix
//! stage is recognised by the *shape* of its procedures and carried as
//! `CalGray` or `CalRGB` with the components unchanged; every other space
//! is carried as `Lab`, each colour converted through the space's own
//! procedures, matrices, and table to XYZ and then to L*a*b* by a
//! [`CieJob`] the interpreter loop runs as a `CieDecode` frame (the
//! procedures are PostScript; everything between them is native).
//!
//! `setcolorrendering`, `currentcolorrendering`, and `findcolorrendering`
//! record a rendering dictionary and never apply it — distillation keeps
//! colour device-independent — and are defined with or without a
//! graphics backend, like the screen operators.

use std::rc::Rc;

use crate::error::VmError;
use crate::graphics::{CieColor, ImageSpec, SpaceSpec};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Access, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::graphics::{is_array, procedure_object};
use crate::ops::pagedevice::in_global;

op_table! { OPS {
    "setcolorrendering" => setcolorrendering, [Any];
    "currentcolorrendering" => currentcolorrendering;
    "findcolorrendering" => findcolorrendering, [Any];
}}

/// The tolerance within which `MatrixA` counts as the white point and a
/// matrix as the identity.
const NEAR: f32 = 1e-4;

const IDENTITY_3X3: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const UNIT_RANGE_3: [f32; 6] = [0.0, 1.0, 0.0, 1.0, 0.0, 1.0];

/// The name `findcolorrendering` answers when nothing better is defined.
const DEFAULT_RENDERING: &str = "DefaultColorRendering";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    A,
    Abc,
    Def,
    Defg,
}

impl Family {
    pub(crate) fn from_name(name: &[u8]) -> Option<Family> {
        Some(match name {
            b"CIEBasedA" => Family::A,
            b"CIEBasedABC" => Family::Abc,
            b"CIEBasedDEF" => Family::Def,
            b"CIEBasedDEFG" => Family::Defg,
            _ => return None,
        })
    }

    /// The number of components a colour in the family has.
    pub(crate) fn components(self) -> usize {
        match self {
            Family::A => 1,
            Family::Abc | Family::Def => 3,
            Family::Defg => 4,
        }
    }

    /// The number of components the first stage takes: one for `A`,
    /// three for the others, whose table (if any) delivers `ABC`.
    fn first_stage(self) -> usize {
        match self {
            Family::A => 1,
            _ => 3,
        }
    }
}

/// The lookup-table pre-stage of the `DEF` and `DEFG` families (PLRM3
/// Tables 4.7 and 4.8).
#[derive(Clone, Debug)]
pub(crate) struct TableStage {
    /// `RangeDEF` (six numbers) or `RangeDEFG` (eight).
    pub range_def: Vec<f32>,
    /// `DecodeDEF`/`DecodeDEFG`; `None` when absent (identity).
    pub decode_def: Option<Vec<Object>>,
    /// `RangeHIJ` (six) or `RangeHIJK` (eight).
    pub range_hij: Vec<f32>,
    /// The table's dimensions, three or four, each at least 2.
    pub dims: Vec<u32>,
    /// The table's strings concatenated in index order, three bytes per
    /// entry, so entry `(h, i, j[, k])` starts at `3 × (((h × NI + i) ×
    /// NJ + j) × NK + k)`.
    pub data: Vec<u8>,
}

/// A CIE-based colour space as its dictionary gave it, with the
/// manual's defaults filled in. Procedures are kept as objects; a
/// [`CieJob`] hands them to the interpreter loop one call at a time.
#[derive(Clone, Debug)]
pub(crate) struct CieSpace {
    pub family: Family,
    pub white: [f32; 3],
    pub black: [f32; 3],
    /// `RangeA` (two numbers) or `RangeABC` (six).
    pub range_abc: Vec<f32>,
    /// `DecodeA` (one procedure) or `DecodeABC` (three); `None` when
    /// absent (identity).
    pub decode_abc: Option<Vec<Object>>,
    /// `MatrixA` (three numbers) or `MatrixABC` (nine).
    pub matrix_abc: Vec<f32>,
    pub range_lmn: [f32; 6],
    /// `DecodeLMN`; `None` when absent (identity).
    pub decode_lmn: Option<Vec<Object>>,
    pub matrix_lmn: [f32; 9],
    /// The table stage of the `DEF`/`DEFG` families; `None` for the
    /// others.
    pub table: Option<TableStage>,
}

/// What the VM keeps for a colour-space array that involves a CIE-based
/// space: the array as the program gave it, and, when the array itself
/// names a CIE family, the parsed space and whether it collapsed to a
/// calibrated space (components pass through) or is carried as `Lab`.
#[derive(Clone, Debug)]
pub(crate) struct CieEntry {
    pub array: Object,
    pub space: Option<Rc<CieSpace>>,
    pub collapsed: bool,
}

// --- reading the dictionary ----------------------------------------------------

fn entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<Object>, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)
}

/// `object` as exactly `count` numbers: `typecheck` unless an array of
/// numbers, `rangecheck` for another length.
fn numbers(i: &Interp, object: Object, count: usize) -> Result<Vec<f32>, VmError> {
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let values: Vec<f32> = items(i, object)?
        .into_iter()
        .map(|o| o.as_number().ok_or(VmError::TypeCheck))
        .collect::<Result<_, _>>()?;
    if values.len() != count {
        return Err(VmError::RangeCheck);
    }
    Ok(values)
}

/// An optional array of `count` numbers, `default` when absent.
fn numbers_or(
    i: &mut Interp,
    dict: Object,
    key: &str,
    count: usize,
    default: &[f32],
) -> Result<Vec<f32>, VmError> {
    match entry(i, dict, key)? {
        Some(object) => numbers(i, object, count),
        None => Ok(default.to_vec()),
    }
}

/// A `Range*` entry of `pairs` pairs, `[0 1]` each when absent; a pair
/// whose minimum exceeds its maximum is `rangecheck`.
fn range(i: &mut Interp, dict: Object, key: &str, pairs: usize) -> Result<Vec<f32>, VmError> {
    let values = numbers_or(i, dict, key, 2 * pairs, &[0.0, 1.0].repeat(pairs))?;
    if values.chunks(2).any(|pair| pair[0] > pair[1]) {
        return Err(VmError::RangeCheck);
    }
    Ok(values)
}

fn is_procedure(object: Object) -> bool {
    is_array(object) && object.is_executable()
}

/// A `Decode*` entry: an array of exactly `count` procedures (`DecodeA`
/// is one procedure on its own); `None` when absent.
fn procedures(
    i: &mut Interp,
    dict: Object,
    key: &str,
    count: usize,
    single: bool,
) -> Result<Option<Vec<Object>>, VmError> {
    let Some(object) = entry(i, dict, key)? else {
        return Ok(None);
    };
    if single {
        if !is_procedure(object) {
            return Err(VmError::TypeCheck);
        }
        return Ok(Some(vec![object]));
    }
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let procs = items(i, object)?;
    if procs.len() != count {
        return Err(VmError::RangeCheck);
    }
    if procs.iter().any(|&p| !is_procedure(p)) {
        return Err(VmError::TypeCheck);
    }
    Ok(Some(procs))
}

/// A tristimulus entry: three numbers. The white point must have a unit
/// Y and positive X and Z, the black point non-negative components.
fn tristimulus(i: &Interp, object: Object, white: bool) -> Result<[f32; 3], VmError> {
    let values = numbers(i, object, 3)?;
    let point = [values[0], values[1], values[2]];
    let valid = if white {
        point[0] > 0.0 && point[1] == 1.0 && point[2] > 0.0
    } else {
        point.iter().all(|&v| v >= 0.0)
    };
    if !valid || point.iter().any(|v| !v.is_finite()) {
        return Err(VmError::RangeCheck);
    }
    Ok(point)
}

/// The `WhitePoint` entry, which is required, and the `BlackPoint`
/// entry, zero when absent.
fn points(i: &mut Interp, dict: Object) -> Result<([f32; 3], [f32; 3]), VmError> {
    let white = entry(i, dict, "WhitePoint")?.ok_or(VmError::Undefined)?;
    let white = tristimulus(i, white, true)?;
    let black = match entry(i, dict, "BlackPoint")? {
        Some(object) => tristimulus(i, object, false)?,
        None => [0.0; 3],
    };
    Ok((white, black))
}

/// A table dimension: an integer of at least 2.
fn dimension(object: Object) -> Result<u32, VmError> {
    if object.ty() != Type::Integer {
        return Err(VmError::TypeCheck);
    }
    let n = object.as_i32().expect("integer");
    if n < 2 {
        return Err(VmError::RangeCheck);
    }
    Ok(n as u32)
}

/// The strings of one row of a table, each `len` bytes, `count` of them,
/// appended to `data`.
fn table_strings(
    i: &Interp,
    row: Object,
    count: u32,
    len: usize,
    data: &mut Vec<u8>,
) -> Result<(), VmError> {
    if !is_array(row) {
        return Err(VmError::TypeCheck);
    }
    let strings = items(i, row)?;
    if strings.len() != count as usize {
        return Err(VmError::RangeCheck);
    }
    for string in strings {
        if string.ty() != Type::String {
            return Err(VmError::TypeCheck);
        }
        let content = bytes(i, string)?;
        if content.len() != len {
            return Err(VmError::RangeCheck);
        }
        data.extend_from_slice(&content);
    }
    Ok(())
}

/// The `Table` entry of a `DEF` (`inputs` 3) or `DEFG` (`inputs` 4)
/// space: the dimensions, then the strings, or for `DEFG` the arrays of
/// strings, copied in index order.
fn table(i: &mut Interp, dict: Object, inputs: usize) -> Result<(Vec<u32>, Vec<u8>), VmError> {
    let object = entry(i, dict, "Table")?.ok_or(VmError::Undefined)?;
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let elements = items(i, object)?;
    if elements.len() != inputs + 1 {
        return Err(VmError::RangeCheck);
    }
    let dims: Vec<u32> = elements[..inputs]
        .iter()
        .map(|&d| dimension(d))
        .collect::<Result<_, _>>()?;
    let strings = elements[inputs];
    let mut data = Vec::new();
    match dims.as_slice() {
        [nh, ni, nj] => {
            table_strings(
                i,
                strings,
                *nh,
                3 * (*ni as usize) * (*nj as usize),
                &mut data,
            )?;
        }
        [nh, ni, nj, nk] => {
            if !is_array(strings) {
                return Err(VmError::TypeCheck);
            }
            let rows = items(i, strings)?;
            if rows.len() != *nh as usize {
                return Err(VmError::RangeCheck);
            }
            for row in rows {
                table_strings(i, row, *ni, 3 * (*nj as usize) * (*nk as usize), &mut data)?;
            }
        }
        _ => unreachable!("three or four dimensions"),
    }
    Ok((dims, data))
}

/// Reads and checks the dictionary of a `[family dict]` array (`params`
/// is the array after the family name): a missing `WhitePoint` or
/// `Table` is `undefined`, a wrongly typed entry `typecheck`, a value
/// outside what the manual allows `rangecheck` — also an array of the
/// wrong length, and a parameter list that is not one dictionary.
pub(crate) fn parse(
    i: &mut Interp,
    family: Family,
    params: &[Object],
) -> Result<CieSpace, VmError> {
    let &[dict] = params else {
        return Err(VmError::RangeCheck);
    };
    if dict.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    let (white, black) = points(i, dict)?;
    let first = family.first_stage();
    let (range_key, decode_key, matrix_key, matrix_default): (_, _, _, &[f32]) = match family {
        Family::A => ("RangeA", "DecodeA", "MatrixA", &[1.0, 1.0, 1.0]),
        _ => ("RangeABC", "DecodeABC", "MatrixABC", &IDENTITY_3X3),
    };
    let range_abc = range(i, dict, range_key, first)?;
    let decode_abc = procedures(i, dict, decode_key, first, family == Family::A)?;
    let matrix_abc = numbers_or(i, dict, matrix_key, matrix_default.len(), matrix_default)?;
    let range_lmn = range(i, dict, "RangeLMN", 3)?;
    let decode_lmn = procedures(i, dict, "DecodeLMN", 3, false)?;
    let matrix_lmn = numbers_or(i, dict, "MatrixLMN", 9, &IDENTITY_3X3)?;
    let table = match family {
        Family::A | Family::Abc => None,
        Family::Def | Family::Defg => {
            let inputs = family.components();
            let (range_key, decode_key, hij_key) = if family == Family::Def {
                ("RangeDEF", "DecodeDEF", "RangeHIJ")
            } else {
                ("RangeDEFG", "DecodeDEFG", "RangeHIJK")
            };
            let range_def = range(i, dict, range_key, inputs)?;
            let decode_def = procedures(i, dict, decode_key, inputs, false)?;
            let range_hij = range(i, dict, hij_key, inputs)?;
            let (dims, data) = table(i, dict, inputs)?;
            Some(TableStage {
                range_def,
                decode_def,
                range_hij,
                dims,
                data,
            })
        }
    };
    Ok(CieSpace {
        family,
        white,
        black,
        range_abc,
        decode_abc,
        matrix_abc,
        range_lmn: range_lmn.try_into().expect("six numbers"),
        decode_lmn,
        matrix_lmn: matrix_lmn.try_into().expect("nine numbers"),
        table,
    })
}

// --- the space as the VM uses it ------------------------------------------------

impl CieSpace {
    pub(crate) fn components(&self) -> usize {
        self.family.components()
    }

    /// The ranges the program's components are clamped to: the table
    /// stage's for `DEF`/`DEFG`, the first stage's otherwise. Also the
    /// default `Decode` of an image in the space, which maps samples
    /// onto the components' full ranges (PLRM3 §4.10.5 leaves the
    /// suggested unit arrays to the space's parameters).
    pub(crate) fn ranges(&self) -> &[f32] {
        match &self.table {
            Some(table) => &table.range_def,
            None => &self.range_abc,
        }
    }

    /// `values` clamped to their ranges, a NaN to the minimum.
    pub(crate) fn clamp(&self, values: &[f32]) -> Vec<f32> {
        values
            .iter()
            .zip(self.ranges().chunks(2))
            .map(|(&v, pair)| {
                if v.is_nan() {
                    pair[0]
                } else {
                    v.clamp(pair[0], pair[1])
                }
            })
            .collect()
    }

    /// The initial colour: zero, or the nearest value each range allows.
    pub(crate) fn initial_components(&self) -> Vec<f32> {
        self.clamp(&vec![0.0; self.components()])
    }

    /// The boundary space when the transformation is a single gamma-and-
    /// matrix stage (`CalGray` from `A`, `CalRGB` from `ABC`); `None`
    /// when it is not, in which case the space is carried as `Lab`.
    pub(crate) fn collapse(&self, i: &Interp) -> Option<SpaceSpec> {
        if !within_unit(&self.range_abc) {
            return None;
        }
        match self.family {
            Family::A => self.collapse_gray(i),
            Family::Abc => self.collapse_rgb(i),
            Family::Def | Family::Defg => None,
        }
    }

    fn collapse_gray(&self, i: &Interp) -> Option<SpaceSpec> {
        let gamma = stage_gammas(i, self.decode_abc.as_deref(), 1)?[0];
        let white_matrix = self
            .matrix_abc
            .iter()
            .zip(self.white)
            .all(|(&m, w)| (m - w).abs() <= NEAR);
        if !white_matrix || !identity_stage(i, self.decode_lmn.as_deref(), &self.matrix_lmn) {
            return None;
        }
        let decoded = decoded_ranges(&self.range_abc, &[gamma]);
        if !lmn_admits(&self.range_lmn, &stage_image(&decoded, &self.matrix_abc)) {
            return None;
        }
        Some(SpaceSpec::CalGray {
            white: self.white,
            black: self.black,
            gamma,
        })
    }

    fn collapse_rgb(&self, i: &Interp) -> Option<SpaceSpec> {
        let (gamma, matrix) = if identity_decode(i, self.decode_lmn.as_deref()) {
            // The ABC stage carries the gammas; its matrix is followed by
            // the LMN matrix (the identity when absent), so the two fold
            // into one.
            let gamma = stage_gammas(i, self.decode_abc.as_deref(), 3)?;
            let decoded = decoded_ranges(&self.range_abc, &gamma);
            if !lmn_admits(&self.range_lmn, &stage_image(&decoded, &self.matrix_abc)) {
                return None;
            }
            (gamma, compose(&self.matrix_abc, &self.matrix_lmn))
        } else {
            // The LMN stage carries the gammas and the matrix; the ABC
            // stage must then be the identity so that the LMN values are
            // the components themselves.
            if !identity_stage(i, self.decode_abc.as_deref(), &self.matrix_abc) {
                return None;
            }
            let decoded = decoded_ranges(&self.range_abc, &[1.0; 3]);
            if !lmn_admits(&self.range_lmn, &stage_image(&decoded, &IDENTITY_3X3)) {
                return None;
            }
            (
                stage_gammas(i, self.decode_lmn.as_deref(), 3)?,
                self.matrix_lmn,
            )
        };
        Some(SpaceSpec::CalRGB {
            white: self.white,
            black: self.black,
            gamma: gamma.try_into().expect("three gammas"),
            matrix,
        })
    }

    /// The boundary space of a space that does not collapse.
    pub(crate) fn lab(&self) -> SpaceSpec {
        SpaceSpec::Lab {
            white: self.white,
            black: self.black,
            range: LAB_RANGE,
        }
    }
}

/// Whether every pair of `ranges` lies within the unit interval, so the
/// clamp a calibrated space's reader applies can never act.
fn within_unit(ranges: &[f32]) -> bool {
    ranges
        .chunks(2)
        .all(|pair| pair[0] >= 0.0 && pair[1] <= 1.0)
}

/// The exponent a decode procedure is, when it is exactly `{ n exp }`
/// with `n` positive — bound (the operator object) or not (the
/// executable name) — or empty, which is the exponent 1. Anything else
/// is not a gamma procedure.
fn gamma_of(i: &Interp, procedure: Object) -> Option<f32> {
    let body = items(i, procedure).ok()?;
    match body.as_slice() {
        [] => Some(1.0),
        &[exponent, exp] if is_exp(i, exp) => {
            exponent.as_number().filter(|&n| n.is_finite() && n > 0.0)
        }
        _ => None,
    }
}

fn is_exp(i: &Interp, object: Object) -> bool {
    match object.ty() {
        Type::Operator => i
            .operator("exp")
            .is_some_and(|exp| exp.as_operator() == object.as_operator()),
        Type::Name => {
            object.is_executable() && i.mem.name_text(object.as_name().expect("name")) == b"exp"
        }
        _ => false,
    }
}

/// The gammas of a stage's `count` decode procedures, all 1 when the
/// entry is absent; `None` when any procedure is not a gamma.
fn stage_gammas(i: &Interp, decode: Option<&[Object]>, count: usize) -> Option<Vec<f32>> {
    match decode {
        None => Some(vec![1.0; count]),
        Some(procs) => procs.iter().map(|&p| gamma_of(i, p)).collect(),
    }
}

/// Whether a stage's decode procedures are absent or all empty.
fn identity_decode(i: &Interp, decode: Option<&[Object]>) -> bool {
    decode.is_none_or(|procs| {
        procs
            .iter()
            .all(|&p| items(i, p).is_ok_and(|body| body.is_empty()))
    })
}

fn identity_stage(i: &Interp, decode: Option<&[Object]>, matrix: &[f32]) -> bool {
    identity_decode(i, decode) && is_identity(matrix)
}

fn is_identity(matrix: &[f32]) -> bool {
    matrix.len() == 9
        && matrix
            .iter()
            .zip(IDENTITY_3X3)
            .all(|(&m, e)| (m - e).abs() <= NEAR)
}

/// The interval each decoded component covers: the range raised to the
/// gamma, which is monotone on the unit interval.
fn decoded_ranges(ranges: &[f32], gammas: &[f32]) -> Vec<(f32, f32)> {
    ranges
        .chunks(2)
        .zip(gammas)
        .map(|(pair, &g)| (pair[0].powf(g), pair[1].powf(g)))
        .collect()
}

/// The interval each of the three outputs of `matrix` (three elements
/// per input) covers when the inputs range over `inputs`.
fn stage_image(inputs: &[(f32, f32)], matrix: &[f32]) -> [(f32, f32); 3] {
    let mut image = [(0.0f32, 0.0f32); 3];
    for (n, &(lo, hi)) in inputs.iter().enumerate() {
        for (j, slot) in image.iter_mut().enumerate() {
            let m = matrix[n * 3 + j];
            let (a, b) = (m * lo, m * hi);
            slot.0 += a.min(b);
            slot.1 += a.max(b);
        }
    }
    image
}

/// Whether the `RangeLMN` clamp cannot act on `image`: the range is the
/// default — a program that does not mention it does not rely on it,
/// and other interpreters carry such a space as a calibrated one — or
/// it contains the image outright.
fn lmn_admits(range_lmn: &[f32; 6], image: &[(f32, f32); 3]) -> bool {
    *range_lmn == UNIT_RANGE_3
        || image
            .iter()
            .zip(range_lmn.chunks(2))
            .all(|(&(lo, hi), pair)| lo >= pair[0] - NEAR && hi <= pair[1] + NEAR)
}

/// `first` followed by `second`, both three elements per input, in
/// double precision.
fn compose(first: &[f32], second: &[f32]) -> [f32; 9] {
    let mut out = [0.0f32; 9];
    for n in 0..3 {
        for k in 0..3 {
            let sum: f64 = (0..3)
                .map(|j| f64::from(first[n * 3 + j]) * f64::from(second[j * 3 + k]))
                .sum();
            out[n * 3 + k] = sum as f32;
        }
    }
    out
}

/// Whether `spec` is, or contains, a space that came from a CIE-based
/// dictionary, so `currentcolorspace` must answer with the original.
pub(crate) fn mentions_calibrated(spec: &SpaceSpec) -> bool {
    match spec {
        SpaceSpec::CalGray { .. } | SpaceSpec::CalRGB { .. } | SpaceSpec::Lab { .. } => true,
        SpaceSpec::Separation { alternate, .. } | SpaceSpec::DeviceN { alternate, .. } => {
            mentions_calibrated(alternate)
        }
        SpaceSpec::Indexed { base, .. } => mentions_calibrated(base),
        SpaceSpec::Pattern { base } => base.as_deref().is_some_and(mentions_calibrated),
        SpaceSpec::DeviceGray | SpaceSpec::DeviceRGB | SpaceSpec::DeviceCMYK => false,
    }
}

// --- the conversion to L*a*b* ------------------------------------------------------

/// The a*/b* range of the `Lab` space a converting CIE space is carried
/// as: wide enough for what a monitor-like RGB space reaches (its blue
/// falls below −100 in b*), and one eight-bit step per unit in an image.
const LAB_RANGE: [f32; 4] = [-128.0, 127.0, -128.0, 127.0];

/// How many distinct inputs a stage's procedure is called with per
/// component at most; beyond it the inputs are snapped to a grid of
/// this many points over the component's range before the calls.
const CACHE_LIMIT: usize = 4096;

/// The procedure stages of the transformation, in order: the table
/// families' `DecodeDEF`/`DecodeDEFG` before the lookup, `DecodeA`/
/// `DecodeABC` before the first matrix, `DecodeLMN` before the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Table,
    Abc,
    Lmn,
}

impl Stage {
    fn next(self) -> Option<Stage> {
        match self {
            Stage::Table => Some(Stage::Abc),
            Stage::Abc => Some(Stage::Lmn),
            Stage::Lmn => None,
        }
    }
}

impl CieSpace {
    fn first_stage(&self) -> Stage {
        if self.table.is_some() {
            Stage::Table
        } else {
            Stage::Abc
        }
    }

    /// The decode procedures of `stage`, none when the entry is absent.
    fn procedures(&self, stage: Stage) -> &[Object] {
        let procs = match stage {
            Stage::Table => self.table.as_ref().and_then(|t| t.decode_def.as_deref()),
            Stage::Abc => self.decode_abc.as_deref(),
            Stage::Lmn => self.decode_lmn.as_deref(),
        };
        procs.unwrap_or(&[])
    }

    /// Every decode procedure of the space, for a frame's references.
    fn all_procedures(&self) -> Vec<Object> {
        [Stage::Table, Stage::Abc, Stage::Lmn]
            .iter()
            .flat_map(|&stage| self.procedures(stage).iter().copied())
            .collect()
    }

    /// The ranges the inputs of `stage` are clamped to before its
    /// procedures run.
    fn stage_ranges(&self, stage: Stage) -> &[f32] {
        match stage {
            Stage::Table => &self.table.as_ref().expect("a table stage").range_def,
            Stage::Abc => &self.range_abc,
            Stage::Lmn => &self.range_lmn,
        }
    }

    /// What follows the procedures of `stage` natively, for one colour:
    /// the table lookup onto `RangeABC`, the first matrix to LMN, or the
    /// second matrix to XYZ and the inverse transformation to L*a*b*.
    fn after(&self, stage: Stage, decoded: &[f64]) -> [f64; 3] {
        match stage {
            Stage::Table => self
                .table
                .as_ref()
                .expect("a table stage")
                .lookup(decoded, &self.range_abc),
            Stage::Abc => through(&self.matrix_abc, decoded),
            Stage::Lmn => xyz_to_lab(through(&self.matrix_lmn, decoded), self.white, LAB_RANGE),
        }
    }
}

/// `matrix` (three elements per input) applied to `input`.
fn through(matrix: &[f32], input: &[f64]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (n, &v) in input.iter().enumerate() {
        for (k, slot) in out.iter_mut().enumerate() {
            *slot += v * f64::from(matrix[n * 3 + k]);
        }
    }
    out
}

/// `values` clamped pairwise to `ranges`, a NaN to the minimum.
fn clamp_to(values: &mut [f64], ranges: &[f32]) {
    for (v, pair) in values.iter_mut().zip(ranges.chunks(2)) {
        let (lo, hi) = (f64::from(pair[0]), f64::from(pair[1]));
        *v = if v.is_nan() { lo } else { v.clamp(lo, hi) };
    }
}

impl TableStage {
    /// The entry at the lattice point `index` (one index per dimension).
    fn entry(&self, index: &[usize]) -> [u8; 3] {
        let mut at = 0;
        for (&n, &i) in self.dims.iter().zip(index) {
            at = at * n as usize + i;
        }
        let at = 3 * at;
        [self.data[at], self.data[at + 1], self.data[at + 2]]
    }

    /// The ABC value for `hij` (three or four inputs, each clamped to its
    /// `RangeHIJ(K)` pair, which the lattice spans evenly): a multilinear
    /// interpolation among the surrounding entries' bytes, the byte
    /// scale then mapped onto `range_abc` (PLRM3 Tables 4.7 and 4.8).
    fn lookup(&self, hij: &[f64], range_abc: &[f32]) -> [f64; 3] {
        let n = self.dims.len();
        let mut base = vec![0usize; n];
        let mut frac = vec![0f64; n];
        for d in 0..n {
            let (lo, hi) = (
                f64::from(self.range_hij[2 * d]),
                f64::from(self.range_hij[2 * d + 1]),
            );
            let v = if hij[d].is_nan() {
                lo
            } else {
                hij[d].clamp(lo, hi)
            };
            let steps = f64::from(self.dims[d] - 1);
            let t = if hi > lo {
                (v - lo) / (hi - lo) * steps
            } else {
                0.0
            };
            let i = (t.floor() as usize).min(self.dims[d] as usize - 2);
            base[d] = i;
            frac[d] = (t - i as f64).clamp(0.0, 1.0);
        }
        let mut sum = [0.0f64; 3];
        let mut index = vec![0usize; n];
        for corner in 0..(1usize << n) {
            let mut weight = 1.0;
            for d in 0..n {
                let upper = (corner >> d) & 1 == 1;
                index[d] = base[d] + usize::from(upper);
                weight *= if upper { frac[d] } else { 1.0 - frac[d] };
            }
            if weight == 0.0 {
                continue;
            }
            for (slot, byte) in sum.iter_mut().zip(self.entry(&index)) {
                *slot += weight * f64::from(byte);
            }
        }
        let mut out = [0.0; 3];
        for (k, slot) in out.iter_mut().enumerate() {
            let (lo, hi) = (f64::from(range_abc[2 * k]), f64::from(range_abc[2 * k + 1]));
            *slot = lo + sum[k] / 255.0 * (hi - lo);
        }
        out
    }
}

/// The inverse of the `g` function of ISO 32000-1 §8.6.5.4 (PLRM3
/// Example 4.11): a cube root above the knee, the linear branch below
/// it; the two meet at `(6/29)³`.
fn g_inverse(y: f64) -> f64 {
    const KNEE: f64 = 6.0 / 29.0;
    if y > KNEE * KNEE * KNEE {
        y.cbrt()
    } else {
        y * (841.0 / 108.0) + 4.0 / 29.0
    }
}

/// XYZ to L*a*b* relative to `white`, inverting the two-stage
/// transformation of ISO 32000-1 §8.6.5.4: L* from Y, a* from X and Y,
/// b* from Y and Z; L* clamped to `[0, 100]`, a* and b* to `range`. A
/// NaN component counts as zero.
fn xyz_to_lab(xyz: [f64; 3], white: [f32; 3], range: [f32; 4]) -> [f64; 3] {
    let f = |v: f64, w: f32| {
        let v = if v.is_nan() { 0.0 } else { v };
        g_inverse(v / f64::from(w))
    };
    let (fx, fy, fz) = (
        f(xyz[0], white[0]),
        f(xyz[1], white[1]),
        f(xyz[2], white[2]),
    );
    [
        (116.0 * fy - 16.0).clamp(0.0, 100.0),
        (500.0 * (fx - fy)).clamp(f64::from(range[0]), f64::from(range[1])),
        (200.0 * (fy - fz)).clamp(f64::from(range[2]), f64::from(range[3])),
    ]
}

/// An L*a*b* triple as the three eight-bit samples an image or an
/// Indexed lookup carries: L* over `[0, 100]`, a* and b* over `range`
/// (the decode a `Lab` image declares, ISO 32000-1 Table 90).
fn lab_bytes(lab: &[f64], range: [f32; 4]) -> [u8; 3] {
    let scale = |v: f64, lo: f64, hi: f64| -> u8 {
        if hi > lo {
            ((v - lo) / (hi - lo) * 255.0).round().clamp(0.0, 255.0) as u8
        } else {
            0
        }
    };
    [
        scale(lab[0], 0.0, 100.0),
        scale(lab[1], f64::from(range[0]), f64::from(range[1])),
        scale(lab[2], f64::from(range[2]), f64::from(range[3])),
    ]
}

/// The decode array of an image whose samples are [`lab_bytes`].
fn lab_decode() -> Vec<f32> {
    vec![
        0.0,
        100.0,
        LAB_RANGE[0],
        LAB_RANGE[1],
        LAB_RANGE[2],
        LAB_RANGE[3],
    ]
}

/// The `CieColor` for `components` in the space `id`.
pub(crate) fn cie_color(id: u32, components: &[f32]) -> CieColor {
    let mut kept = [0.0; 4];
    for (slot, &c) in kept.iter_mut().zip(components) {
        *slot = c;
    }
    CieColor {
        space: id,
        components: kept,
    }
}

/// One procedure call a stage needs: `procedure` with `input` on the
/// operand stack, for the values of `component`.
#[derive(Clone, Debug)]
struct Call {
    procedure: Object,
    component: usize,
    input: f64,
}

/// What a job does with its L*a*b* results.
#[derive(Clone, Debug)]
pub(crate) enum Purpose {
    /// `setcolor` or `setcolorspace`: the one colour becomes the boundary
    /// colour, with the program's clamped components attached for
    /// `currentcolor`.
    Color { id: u32, kept: Vec<f32> },
    /// `image`: the samples become eight-bit L*a*b* samples handed to the
    /// backend under `spec`, already the `Lab` space with its decode.
    Image { spec: ImageSpec },
    /// `setcolorspace` of an Indexed space over the CIE space: the lookup
    /// table's entries become L*a*b* bytes and the space is set then.
    Indexed { array: Object, hival: u16 },
}

/// A conversion of colours in a CIE-based space to L*a*b*, run by the
/// interpreter loop as a `CieDecode` frame: the space's decode
/// procedures are the only PostScript in the transformation, so the job
/// lists the calls one stage needs — each procedure once per distinct
/// input of its component, the results cached against the inputs — and,
/// when the stage's results are in, continues natively (matrix, table,
/// or the final XYZ→L*a*b*) into the next stage's inputs. A procedure
/// that is empty is the identity and is never called. `values` holds
/// `width` numbers per colour: one colour for `setcolor`, every sample
/// of an image, every entry of a lookup table.
#[derive(Clone, Debug)]
pub struct CieJob {
    space: Rc<CieSpace>,
    operator: &'static str,
    /// The stage whose procedures run next; `None` once `values` are
    /// L*a*b* triples.
    stage: Option<Stage>,
    values: Vec<f64>,
    width: usize,
    /// The stage's calls, grouped by component with the inputs ascending,
    /// and the results delivered so far in the same order.
    calls: Vec<Call>,
    results: Vec<f64>,
    /// The operand-stack depth beneath the input of the call in flight.
    pending: Option<usize>,
    purpose: Purpose,
}

impl CieJob {
    fn new(
        space: Rc<CieSpace>,
        values: Vec<f64>,
        purpose: Purpose,
        operator: &'static str,
    ) -> Self {
        let mut job = CieJob {
            operator,
            stage: Some(space.first_stage()),
            width: space.components(),
            space,
            values,
            calls: Vec::new(),
            results: Vec::new(),
            pending: None,
            purpose,
        };
        job.prepare();
        job
    }

    pub(crate) fn operator(&self) -> &'static str {
        self.operator
    }

    /// The procedure the next call runs, null when none is due.
    pub(crate) fn current_procedure(&self) -> Object {
        self.calls
            .get(self.results.len())
            .map_or(Object::null(), |call| call.procedure)
    }

    /// Every object the job still needs.
    pub(crate) fn references(&self) -> Vec<Object> {
        let mut objects = self.space.all_procedures();
        if let Purpose::Indexed { array, .. } = &self.purpose {
            objects.push(*array);
        }
        objects
    }

    /// Marks the call `advance` returned as running above `depth`
    /// operands.
    pub(crate) fn issue(&mut self, depth: usize) {
        self.pending = Some(depth);
    }

    /// The depth recorded by `issue`, once.
    pub(crate) fn take_pending(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// The result of the call in flight.
    pub(crate) fn deliver(&mut self, result: f64) {
        self.results.push(result);
    }

    /// The next procedure to call with its input, completing stages
    /// natively on the way; `None` when the values are L*a*b*. Asking
    /// again before a result is delivered returns the same call.
    pub(crate) fn advance(&mut self) -> Option<(Object, f32)> {
        while let Some(stage) = self.stage {
            if let Some(call) = self.calls.get(self.results.len()) {
                return Some((call.procedure, call.input as f32));
            }
            self.complete(stage);
        }
        None
    }

    fn column(&self, component: usize) -> Vec<f64> {
        self.values
            .iter()
            .skip(component)
            .step_by(self.width)
            .copied()
            .collect()
    }

    /// Clamps the stage's inputs to their ranges and lists the calls: for
    /// each component with a procedure that is not the identity, its
    /// distinct values ascending, snapped to a grid first when there are
    /// more than the cache holds.
    fn prepare(&mut self) {
        let stage = self.stage.expect("a stage to prepare");
        let ranges = self.space.stage_ranges(stage).to_vec();
        for colour in self.values.chunks_mut(self.width) {
            clamp_to(colour, &ranges);
        }
        let procedures = self.space.procedures(stage).to_vec();
        for (component, &procedure) in procedures.iter().enumerate() {
            if procedure.length() == Some(0) {
                continue;
            }
            let (lo, hi) = (
                f64::from(ranges[2 * component]),
                f64::from(ranges[2 * component + 1]),
            );
            let mut distinct = distinct_values(&self.column(component));
            if distinct.len() > CACHE_LIMIT && hi > lo {
                let steps = (CACHE_LIMIT - 1) as f64;
                let snap = |v: f64| lo + ((v - lo) / (hi - lo) * steps).round() / steps * (hi - lo);
                for v in self.values.iter_mut().skip(component).step_by(self.width) {
                    *v = snap(*v);
                }
                distinct = distinct_values(&self.column(component));
            }
            self.calls.extend(distinct.into_iter().map(|input| Call {
                procedure,
                component,
                input,
            }));
        }
    }

    /// Replaces each input of the stage by its procedure's result, runs
    /// the native part after the stage, and prepares the next.
    fn complete(&mut self, stage: Stage) {
        let mut start = 0;
        while start < self.calls.len() {
            let component = self.calls[start].component;
            let end = start
                + self.calls[start..]
                    .iter()
                    .take_while(|call| call.component == component)
                    .count();
            let (run, results) = (&self.calls[start..end], &self.results[start..end]);
            for v in self.values.iter_mut().skip(component).step_by(self.width) {
                let at = run
                    .binary_search_by(|call| call.input.partial_cmp(v).expect("no NaN"))
                    .unwrap_or_else(|near| near.min(run.len() - 1));
                *v = results[at];
            }
            start = end;
        }
        let mut out = Vec::with_capacity(self.values.len() / self.width * 3);
        for colour in self.values.chunks(self.width) {
            out.extend(self.space.after(stage, colour));
        }
        self.values = out;
        self.width = 3;
        self.calls.clear();
        self.results.clear();
        self.stage = stage.next();
        if self.stage.is_some() {
            self.prepare();
        }
    }

    /// The finished values as eight-bit L*a*b* samples.
    fn bytes(&self) -> Vec<u8> {
        self.values
            .chunks(3)
            .flat_map(|lab| lab_bytes(lab, LAB_RANGE))
            .collect()
    }
}

fn distinct_values(column: &[f64]) -> Vec<f64> {
    let mut distinct = column.to_vec();
    distinct.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
    distinct.dedup();
    distinct
}

/// The job converting `components` (clamped) set in the space `id`.
pub(crate) fn color_job(
    space: Rc<CieSpace>,
    id: u32,
    components: &[f32],
    operator: &'static str,
) -> CieJob {
    let values = components.iter().map(|&c| f64::from(c)).collect();
    CieJob::new(
        space,
        values,
        Purpose::Color {
            id,
            kept: components.to_vec(),
        },
        operator,
    )
}

/// The job converting an image's samples: each is decoded through
/// `spec.decode` into a component value (twelve-bit samples reduced to
/// eight first, so a component's procedure runs at most 256 times) and
/// the image is handed on as eight-bit L*a*b* with the matching decode.
pub(crate) fn image_job(
    space: Rc<CieSpace>,
    mut spec: ImageSpec,
    data: &[u8],
    operator: &'static str,
) -> Result<CieJob, VmError> {
    let components = space.components();
    let bits = spec.bits_per_component;
    let reduce = bits > 8;
    let max = if reduce {
        255.0
    } else {
        f64::from((1u32 << bits) - 1)
    };
    let samples = unpack_samples(
        data,
        spec.width as usize,
        spec.height as usize,
        components,
        bits,
    );
    if spec.decode.len() != 2 * components {
        return Err(VmError::RangeCheck);
    }
    let decode: Vec<f64> = spec.decode.iter().map(|&d| f64::from(d)).collect();
    let values = samples
        .iter()
        .enumerate()
        .map(|(n, &sample)| {
            let sample = if reduce { sample >> 4 } else { sample };
            let (lo, hi) = (
                decode[2 * (n % components)],
                decode[2 * (n % components) + 1],
            );
            lo + f64::from(sample) / max * (hi - lo)
        })
        .collect();
    spec.color_space = Some(space.lab());
    spec.bits_per_component = 8;
    spec.decode = lab_decode();
    Ok(CieJob::new(
        space,
        values,
        Purpose::Image { spec },
        operator,
    ))
}

/// The samples of `data` (`height` rows of `width × components`, each
/// `bits` wide, rows padded to a byte) as integers.
fn unpack_samples(
    data: &[u8],
    width: usize,
    height: usize,
    components: usize,
    bits: u8,
) -> Vec<u16> {
    let per_row = width * components;
    let row_bytes = (per_row * usize::from(bits)).div_ceil(8);
    let mut samples = Vec::with_capacity(per_row * height);
    for row in data.chunks(row_bytes).take(height) {
        for n in 0..per_row {
            let bit = n * usize::from(bits);
            let mut value = 0u16;
            for k in 0..usize::from(bits) {
                let at = bit + k;
                let byte = row.get(at / 8).copied().unwrap_or(0);
                value = (value << 1) | u16::from((byte >> (7 - at % 8)) & 1);
            }
            samples.push(value);
        }
    }
    samples
}

/// The job converting the `lookup` entries of `[/Indexed base hival
/// lookup]` (`array`), whose bytes are unit-range components of the base
/// (PLRM3 §4.8.4), into L*a*b* bytes over the `Lab` base.
pub(crate) fn indexed_job(
    space: Rc<CieSpace>,
    array: Object,
    hival: u16,
    lookup: &[u8],
    operator: &'static str,
) -> CieJob {
    let count = (usize::from(hival) + 1) * space.components();
    let values = lookup
        .iter()
        .take(count)
        .map(|&b| f64::from(b) / 255.0)
        .collect();
    CieJob::new(space, values, Purpose::Indexed { array, hival }, operator)
}

/// Runs `job`: to completion at once when no procedure needs calling,
/// else as a `CieDecode` frame that finishes it later.
pub(crate) fn start_job(i: &mut Interp, mut job: CieJob) -> Result<(), VmError> {
    if job.advance().is_none() {
        return finish_job(i, job);
    }
    i.push_frame(Frame::Loop(LoopFrame::CieDecode { job: Box::new(job) }))
}

/// What a completed job does: sets the colour, paints the image, or sets
/// the Indexed space.
pub(crate) fn finish_job(i: &mut Interp, job: CieJob) -> Result<(), VmError> {
    debug_assert!(job.stage.is_none(), "the job has not finished");
    match &job.purpose {
        Purpose::Color { id, kept } => {
            let lab: Vec<f32> = job.values.iter().map(|&v| v as f32).collect();
            let backend = i.backend()?;
            backend.set_color(&lab)?;
            backend.set_cie_color(cie_color(*id, kept))
        }
        Purpose::Image { spec } => {
            let data = job.bytes();
            i.backend()?.image(spec, &data)
        }
        Purpose::Indexed { array, hival } => {
            let space = SpaceSpec::Indexed {
                base: Box::new(job.space.lab()),
                hival: *hival,
                lookup: job.bytes(),
            };
            i.backend()?.set_color_space(&space)?;
            let id = i.register_cie(CieEntry {
                array: *array,
                space: None,
                collapsed: false,
            })?;
            i.backend()?.set_cie_color(cie_color(id, &[]))
        }
    }
}

// --- colour rendering ------------------------------------------------------------------

/// Checks a type 1 colour rendering dictionary (PLRM3 §7.1.2, Table 7.1)
/// as far as its required entries: `ColorRenderingType` an integer
/// (`typecheck`) equal to 1 (`rangecheck`), `WhitePoint` a valid white
/// point, `TransformPQR` three procedures. An absent required entry is
/// `missing`; the optional entries are not looked at, since nothing
/// applies the dictionary.
pub(crate) fn check_rendering_dict(
    i: &mut Interp,
    dict: Object,
    missing: VmError,
) -> Result<(), VmError> {
    if dict.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    let kind = entry(i, dict, "ColorRenderingType")?.ok_or(missing)?;
    if kind.ty() != Type::Integer {
        return Err(VmError::TypeCheck);
    }
    if kind.as_i32() != Some(1) {
        return Err(VmError::RangeCheck);
    }
    let white = entry(i, dict, "WhitePoint")?.ok_or(missing)?;
    tristimulus(i, white, true)?;
    match entry(i, dict, "TransformPQR")? {
        Some(_) => procedures(i, dict, "TransformPQR", 3, false).map(|_| ()),
        None => Err(missing),
    }
}

fn setcolorrendering(i: &mut Interp) -> Result<(), VmError> {
    let dict = i.peek(0)?;
    check_rendering_dict(i, dict, VmError::Undefined)?;
    let id = i.graphics_proc_ref(dict)?;
    i.set_color_rendering(Some(id))?;
    i.pop()?;
    Ok(())
}

fn currentcolorrendering(i: &mut Interp) -> Result<(), VmError> {
    let dict = match i.color_rendering() {
        Some(id) => i.graphics_proc(id).ok_or(VmError::Undefined)?,
        None => default_instance(i)?,
    };
    i.push(dict)
}

/// `intent findcolorrendering name bool`: the name `intent.device.none`
/// — `device` the page device's `PageDeviceName` when it is a name or
/// string, `none` otherwise, and no halftone name — with `true` when the
/// `ColorRendering` category holds an instance so named; otherwise the
/// default instance's name with `false`, an alternate proposed.
fn findcolorrendering(i: &mut Interp) -> Result<(), VmError> {
    let intent = i.peek(0)?;
    let mut name = text_of(i, intent)?;
    name.push(b'.');
    let device_key = i.intern("PageDeviceName");
    let device = i
        .mem
        .dict(i.page_device())
        .and_then(|dict| dict.get(device_key))
        .and_then(|value| text_of(i, value).ok());
    name.extend_from_slice(device.as_deref().unwrap_or(b"none"));
    name.extend_from_slice(b".none");
    let key = i.mem.intern(&name).map_err(|_| VmError::LimitCheck)?;
    let category = i.color_rendering_category;
    let found = i.mem.dict_get(category.local, key)?.is_some()
        || i.mem.dict_get(category.global, key)?.is_some();
    let (answer, exact) = if found {
        (key, true)
    } else {
        (i.intern(DEFAULT_RENDERING), false)
    };
    i.pop()?;
    i.push(answer)?;
    i.push(Object::boolean(exact))
}

/// The text of a name or string operand; anything else is `typecheck`.
fn text_of(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    match object.ty() {
        Type::Name => Ok(i.mem.name_text(object.as_name().expect("name")).to_vec()),
        Type::String => bytes(i, object),
        _ => Err(VmError::TypeCheck),
    }
}

/// The `DefaultColorRendering` instance: a read-only dictionary in global
/// VM with the three required entries of Table 7.1 — type 1, a D65-like
/// white point, and `TransformPQR` procedures that return their
/// component unchanged — built on first use and kept for the
/// interpreter's life.
pub(crate) fn default_instance(i: &mut Interp) -> Result<Object, VmError> {
    if let Some(dict) = i.default_color_rendering() {
        return Ok(dict);
    }
    let dict = in_global(i, |i| -> Result<Object, VmError> {
        let dict = i.mem.new_dict(4);
        let kind = i.intern("ColorRenderingType");
        i.mem.dict_put(dict, kind, Object::integer(1))?;
        let white = i.mem.alloc_array(vec![
            Object::real(0.9505),
            Object::real(1.0),
            Object::real(1.089),
        ])?;
        let white_key = i.intern("WhitePoint");
        i.mem.dict_put(dict, white_key, white)?;
        let transform = procedure_object(i, b"{exch pop exch pop exch pop exch pop}")?;
        let transforms = i.mem.alloc_array(vec![transform; 3])?;
        let transform_key = i.intern("TransformPQR");
        i.mem.dict_put(dict, transform_key, transforms)?;
        i.mem.dict_set_access(dict, Access::ReadOnly)?;
        Ok(dict)
    })?;
    i.set_default_color_rendering(dict);
    Ok(dict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_images_follow_the_matrix() {
        let unit = [(0.0, 1.0); 3];
        assert_eq!(
            stage_image(&unit, &IDENTITY_3X3),
            [(0.0, 1.0), (0.0, 1.0), (0.0, 1.0)]
        );
        let primaries = [0.4, 0.2, 0.02, 0.35, 0.7, 0.1, 0.2, 0.1, 0.95];
        let image = stage_image(&unit, &primaries);
        assert!((image[0].1 - 0.95).abs() < 1e-6);
        assert!((image[2].1 - 1.07).abs() < 1e-6);
        assert_eq!(image[1].0, 0.0);
        let signed = stage_image(&[(0.0, 1.0)], &[-1.0, 2.0, 0.0]);
        assert_eq!(signed, [(-1.0, 0.0), (0.0, 2.0), (0.0, 0.0)]);
    }

    #[test]
    fn the_lmn_range_admits_the_default_or_a_containing_range() {
        let image = [(0.0, 0.95), (0.0, 1.0), (0.0, 1.07)];
        assert!(lmn_admits(&UNIT_RANGE_3, &image));
        assert!(lmn_admits(&[0.0, 0.95, 0.0, 1.0, 0.0, 1.07], &image));
        assert!(!lmn_admits(&[0.0, 0.9, 0.0, 1.0, 0.0, 1.07], &image));
        assert!(!lmn_admits(&[0.1, 2.0, 0.0, 2.0, 0.0, 2.0], &image));
    }

    #[test]
    fn matrices_compose_in_input_major_order() {
        let scale = [2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0];
        let shear = [1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(compose(&scale, &IDENTITY_3X3), scale);
        assert_eq!(compose(&IDENTITY_3X3, &shear), shear);
        // Input 0 scaled by 2 then sheared: contributes 2 to both outputs
        // 0 and 1.
        assert_eq!(
            compose(&scale, &shear),
            [2.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]
        );
    }

    #[test]
    fn decoded_ranges_apply_the_gamma() {
        let ranges = decoded_ranges(&[0.0, 1.0, 0.0, 0.5], &[2.0, 1.0]);
        assert_eq!(ranges, [(0.0, 1.0), (0.0, 0.5)]);
        assert!(within_unit(&[0.0, 1.0, 0.25, 0.75]));
        assert!(!within_unit(&[0.0, 1.5]));
        assert!(!within_unit(&[-0.1, 1.0]));
    }

    #[test]
    fn the_inverse_of_g_has_a_cubic_and_a_linear_branch() {
        // Above the knee the cube root; below it the line, meeting the
        // cube root at (6/29)³.
        assert!((g_inverse(0.027) - 0.3).abs() < 1e-12);
        assert!((g_inverse(0.008) - (0.008 * 841.0 / 108.0 + 4.0 / 29.0)).abs() < 1e-12);
        assert!((g_inverse(0.008) - 0.200227).abs() < 1e-6);
        let knee = 216.0 / 24389.0;
        assert!((g_inverse(knee) - 6.0 / 29.0).abs() < 1e-12);
        assert!((g_inverse(knee + 1e-9) - 6.0 / 29.0).abs() < 1e-6);
        assert!((g_inverse(0.0) - 4.0 / 29.0).abs() < 1e-12);
        assert!(g_inverse(-0.1) < 4.0 / 29.0);
    }

    #[test]
    fn xyz_to_lab_against_hand_computed_values() {
        let white = [0.9505, 1.0, 1.089];
        let close = |got: [f64; 3], want: [f64; 3], tolerance: f64| {
            got.iter().zip(want).all(|(g, w)| (g - w).abs() < tolerance)
        };
        // The white point is L* 100 with no chroma (within the single
        // precision the point is kept in); black is L* 0.
        let w = xyz_to_lab([0.9505, 1.0, 1.089], white, LAB_RANGE);
        assert!(close(w, [100.0, 0.0, 0.0], 1e-4), "{w:?}");
        let k = xyz_to_lab([0.0, 0.0, 0.0], white, LAB_RANGE);
        assert!(close(k, [0.0, 0.0, 0.0], 1e-9), "{k:?}");
        // A mid grey by the cubic branch: fy = ∛0.2 = 0.584804, fx =
        // ∛(0.2/0.9505) = 0.594782, fz = ∛(0.2/1.089) = 0.568412.
        let g = xyz_to_lab([0.2, 0.2, 0.2], white, LAB_RANGE);
        assert!(close(g, [51.837, 4.989, 3.278], 2e-3), "{g:?}");
        // A dark value by the linear branch: fy = 0.004 × 841/108 +
        // 4/29 = 0.169079, fx = fz = 4/29.
        let d = xyz_to_lab([0.0, 0.004, 0.0], white, LAB_RANGE);
        assert!(close(d, [3.613, -15.574, 6.230], 2e-3), "{d:?}");
        // Clamps: L* to 100, a* and b* to the range; NaN reads as zero.
        let c = xyz_to_lab([2.0, 2.0, 0.0], white, [-10.0, 10.0, -10.0, 10.0]);
        assert_eq!(c, [100.0, 10.0, 10.0]);
        let n = xyz_to_lab([f64::NAN, f64::NAN, f64::NAN], white, LAB_RANGE);
        assert_eq!(n, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn matrices_take_three_elements_per_input() {
        assert_eq!(through(&[0.5, 1.0, 2.0], &[0.5]), [0.25, 0.5, 1.0]);
        let m = [1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 1.0, 1.0, 1.0];
        assert_eq!(through(&m, &[1.0, 1.0, 1.0]), [2.0, 3.0, 1.0]);
        let mut v = [-1.0, 0.5, f64::NAN];
        clamp_to(&mut v, &[0.0, 1.0, 0.0, 1.0, 0.25, 1.0]);
        assert_eq!(v, [0.0, 0.5, 0.25]);
    }

    /// A 2×2×2 table whose entry at corner number `k` (h·4 + i·2 + j) is
    /// the bytes (k, 2k, 3k).
    fn cube() -> TableStage {
        let mut data = Vec::new();
        for k in 0..8u8 {
            data.extend([k, 2 * k, 3 * k]);
        }
        TableStage {
            range_def: UNIT_RANGE_3.to_vec(),
            decode_def: None,
            range_hij: vec![0.0, 1.0, 0.0, 1.0, -1.0, 1.0],
            dims: vec![2, 2, 2],
            data,
        }
    }

    #[test]
    fn a_table_interpolates_between_its_corners() {
        let table = cube();
        let unit = UNIT_RANGE_3;
        let scaled = |k: f64| [k / 255.0, 2.0 * k / 255.0, 3.0 * k / 255.0];
        let close =
            |got: [f64; 3], want: [f64; 3]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-9);
        // Corners: (1, 0, 1) is k = 5; the j axis runs −1 to 1.
        assert!(close(table.lookup(&[1.0, 0.0, 1.0], &unit), scaled(5.0)));
        assert!(close(table.lookup(&[0.0, 0.0, -1.0], &unit), scaled(0.0)));
        // The midpoint of the h edge at i = 0, j = −1: between 0 and 4.
        assert!(close(table.lookup(&[0.5, 0.0, -1.0], &unit), scaled(2.0)));
        // j = 0 lies halfway along its axis: between corners 0 and 1.
        assert!(close(table.lookup(&[0.0, 0.0, 0.0], &unit), scaled(0.5)));
        // The centre averages all eight: mean k = 3.5.
        assert!(close(table.lookup(&[0.5, 0.5, 0.0], &unit), scaled(3.5)));
        // Inputs beyond the range clamp to it; the bytes map onto RangeABC.
        assert!(close(table.lookup(&[2.0, -1.0, 5.0], &unit), scaled(5.0)));
        let wide = table.lookup(&[1.0, 1.0, 1.0], &[0.0, 2.0, -1.0, 0.0, 0.0, 1.0]);
        assert!(close(
            wide,
            [2.0 * 7.0 / 255.0, -1.0 + 14.0 / 255.0, 21.0 / 255.0]
        ));
    }

    #[test]
    fn a_four_dimensional_table_has_sixteen_corners() {
        let mut data = Vec::new();
        for k in 0..16u8 {
            data.extend([k, 0, 255 - k]);
        }
        let table = TableStage {
            range_def: [0.0, 1.0].repeat(4),
            decode_def: None,
            range_hij: [0.0, 1.0].repeat(4),
            dims: vec![2, 2, 2, 2],
            data,
        };
        let unit = UNIT_RANGE_3;
        // (1, 0, 1, 1) is k = 8 + 2 + 1 = 11.
        let corner = table.lookup(&[1.0, 0.0, 1.0, 1.0], &unit);
        assert!((corner[0] - 11.0 / 255.0).abs() < 1e-9);
        assert!((corner[2] - 244.0 / 255.0).abs() < 1e-9);
        let centre = table.lookup(&[0.5; 4], &unit);
        assert!((centre[0] - 7.5 / 255.0).abs() < 1e-9);
        assert_eq!(centre[1], 0.0);
    }

    #[test]
    fn lab_bytes_scale_over_the_declared_ranges() {
        assert_eq!(lab_bytes(&[0.0, 0.0, 0.0], LAB_RANGE), [0, 128, 128]);
        assert_eq!(lab_bytes(&[100.0, 127.0, -128.0], LAB_RANGE), [255, 255, 0]);
        assert_eq!(lab_bytes(&[50.0, -128.0, 127.0], LAB_RANGE), [128, 0, 255]);
        assert_eq!(lab_bytes(&[200.0, 300.0, -300.0], LAB_RANGE), [255, 255, 0]);
        assert_eq!(lab_decode(), [0.0, 100.0, -128.0, 127.0, -128.0, 127.0]);
    }

    #[test]
    fn samples_unpack_with_rows_padded_to_bytes() {
        // Three one-bit samples per row, two rows: each row is one byte.
        assert_eq!(
            unpack_samples(&[0b1010_0000, 0b0110_0000], 3, 2, 1, 1),
            [1, 0, 1, 0, 1, 1]
        );
        assert_eq!(unpack_samples(&[0x12, 0x34], 2, 1, 2, 4), [1, 2, 3, 4]);
        assert_eq!(unpack_samples(&[0xff, 0xf0, 0x00], 2, 1, 1, 12), [0xfff, 0]);
        assert_eq!(unpack_samples(&[7, 8, 9], 1, 1, 3, 8), [7, 8, 9]);
        // A short buffer reads as zero bits.
        assert_eq!(unpack_samples(&[0x80], 1, 2, 1, 8), [0x80]);
        assert_eq!(
            distinct_values(&[3.0, 1.0, 3.0, -0.0, 0.0]),
            [0.0, 1.0, 3.0]
        );
    }

    #[test]
    fn families_and_their_arity() {
        assert_eq!(Family::from_name(b"CIEBasedA"), Some(Family::A));
        assert_eq!(Family::from_name(b"CIEBasedDEFG"), Some(Family::Defg));
        assert_eq!(Family::from_name(b"CIEBased"), None);
        assert_eq!(Family::Def.components(), 3);
        assert_eq!(Family::Def.first_stage(), 3);
        assert_eq!(Family::A.first_stage(), 1);
    }
}
