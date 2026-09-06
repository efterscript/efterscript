// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The statement grammar. A program is a list of statements, one per
//! line; each production consumes and produces items of the model, so a
//! program is well typed by construction except where the ill-typed share
//! deliberately breaks one operand and wraps the statement in `stopped`.
//!
//! Operator statements come from a table of operator signatures; the
//! driver takes as many operands as the model's stack top offers and
//! pushes literals for the rest. Compound statements (conditionals,
//! loops, `save`/`restore`, `stopped`, dictionary scopes, `gsave`/
//! `grestore`, procedure definitions) generate their bodies against a
//! copy of the model and accept them only when their net effect is one
//! the model can describe: a loop body must leave the stack as it found
//! it, or only push onto it.

use crate::model::{Comp, Gfx, Item, Model, Ty};
use crate::profile::{Kind, Profile};
use crate::program::{self, Origin, Program};
use crate::rng::Rng;

/// The handler every `stopped` wrapper runs: it names the error on the
/// output and clears the operand stack so the model knows the state.
pub const HANDLER: &str = "{ (psgen: caught ) print $error /errorname get = clear } if";

/// Names the grammar draws literal names and dictionary keys from.
const KEYS: [&str; 6] = ["alpha", "beta", "gamma", "delta", "kappa", "omega"];

const FONTS: [&str; 6] = [
    "Helvetica",
    "Helvetica-Bold",
    "Times-Roman",
    "Times-Italic",
    "Courier",
    "Courier-Bold",
];

/// What an operator's input may be. The first group can be satisfied
/// from the stack or by a literal; the rest are always literals with a
/// value the operator needs to be safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Any,
    Ty(Ty),
    Num,
    IntOrBool,
    NumOrStr,
    /// The same class (number, boolean, string) as the operand at this
    /// position.
    Like(usize),
    /// Exactly the type of the operand at this position.
    SameTy(usize),
    /// Any composite with a length.
    Lengthy,
    ArrayTracked,
    ArrayWritable,
    StringTracked,
    StringWritable,
    DictTracked,
    /// A number small enough for a coordinate: below a thousand.
    Coord,
    NonZeroInt,
    NonZeroReal,
    Byte,
    Shift,
    SmallCount,
    /// A valid index into the operand at this position.
    IndexInto(usize),
    NameKey,
    Delta,
    Radius,
    Angle,
    Unit,
    LineWidth,
    CapJoin,
    Miter,
    Flat,
    ScaleFactor,
    Rotation,
    FontName,
    FontSize,
    Text,
    /// A count that, from the index at the second position, stays inside
    /// the composite at the first.
    IntervalCount(usize, usize),
    /// A literal array that fits into the array at the first position
    /// from the index at the second.
    ArrayFitting(usize, usize),
    /// A literal string that fits likewise.
    StringFitting(usize, usize),
    /// A literal array of one advance per byte of the string at the
    /// position, for `xshow`.
    Advances(usize),
    /// A literal string holding one to three integers, for `token`.
    NumText,
    /// A literal string holding one to three words, for `token`.
    WordText,
    /// A dash array: empty, or non-negative lengths not all zero.
    DashArray,
    Phase,
    /// A well-conditioned matrix literal, for `concat`.
    Matrix,
    /// A corner for `arcto`: its own `moveto`, two more points making a
    /// proper angle, and a radius.
    Corner,
    /// A small inline gray image: `gsave`, placement, dimensions, matrix,
    /// and sample data; the operator closes with `grestore`.
    ImageBlock,
}

impl Pat {
    /// A pattern standing for several operands at once: replacing it by
    /// one wrong literal would let the operator take the rest from the
    /// stack beneath and succeed on them.
    fn block(self) -> bool {
        matches!(self, Pat::Corner | Pat::ImageBlock)
    }

    fn stackable(self) -> bool {
        matches!(
            self,
            Pat::Any
                | Pat::Ty(_)
                | Pat::Num
                | Pat::IntOrBool
                | Pat::NumOrStr
                | Pat::Like(_)
                | Pat::SameTy(_)
                | Pat::Lengthy
                | Pat::ArrayTracked
                | Pat::ArrayWritable
                | Pat::StringTracked
                | Pat::StringWritable
                | Pat::DictTracked
                | Pat::Coord
                | Pat::Delta
                | Pat::Text
        )
    }

    /// Whether a value of `ty` is what the operator accepts here, by
    /// type alone.
    fn accepts_ty(self, ty: Ty, operands: &[Operand]) -> bool {
        match self {
            Pat::Any => true,
            Pat::Ty(t) => ty == t,
            Pat::Num | Pat::Coord | Pat::Delta => ty.is_num(),
            Pat::IntOrBool => matches!(ty, Ty::Int | Ty::Bool),
            Pat::NumOrStr => ty.is_num() || ty == Ty::String,
            Pat::Like(j) => operands
                .get(j)
                .is_some_and(|o| class(o.item.ty) == class(ty)),
            Pat::SameTy(j) => operands.get(j).is_some_and(|o| o.item.ty == ty),
            Pat::Lengthy => ty.is_composite(),
            Pat::ArrayTracked | Pat::ArrayWritable => ty == Ty::Array,
            Pat::StringTracked | Pat::StringWritable | Pat::Text => ty == Ty::String,
            Pat::DictTracked => ty == Ty::Dict,
            Pat::NonZeroInt | Pat::Byte | Pat::Shift | Pat::SmallCount | Pat::IndexInto(_) => {
                ty == Ty::Int
            }
            Pat::CapJoin | Pat::FontSize => ty == Ty::Int,
            Pat::NonZeroReal
            | Pat::Radius
            | Pat::Angle
            | Pat::Unit
            | Pat::LineWidth
            | Pat::Miter
            | Pat::Flat
            | Pat::ScaleFactor
            | Pat::Rotation => ty.is_num(),
            Pat::NameKey | Pat::FontName => ty == Ty::Name,
            Pat::IntervalCount(..) => ty == Ty::Int,
            Pat::ArrayFitting(..) | Pat::Advances(_) | Pat::DashArray | Pat::Matrix => {
                ty == Ty::Array
            }
            Pat::StringFitting(..) | Pat::NumText | Pat::WordText => ty == Ty::String,
            Pat::Phase => ty.is_num(),
            Pat::Corner | Pat::ImageBlock => false,
        }
    }

    /// Whether the model item satisfies the pattern from the stack.
    fn accepts(self, item: &Item, operands: &[Operand], model: &Model) -> bool {
        if !self.stackable() || !self.accepts_ty(item.ty, operands) {
            return false;
        }
        match self {
            Pat::Coord | Pat::Delta => item.mag <= 4,
            Pat::ArrayTracked => model.array_elems(item).is_some(),
            Pat::ArrayWritable => model.array_writable(item),
            Pat::StringTracked => model.string_len(item).is_some(),
            Pat::StringWritable => model.string_writable(item),
            Pat::DictTracked => model.dict_entries(item).is_some(),
            Pat::Lengthy => item.ty != Ty::Proc && model.length_of(item).is_some(),
            _ => true,
        }
    }
}

/// The class `Like` compares: numbers, booleans, strings, and the rest.
fn class(ty: Ty) -> u8 {
    match ty {
        Ty::Int | Ty::Real => 0,
        Ty::Bool => 1,
        Ty::String => 2,
        _ => 3,
    }
}

/// The graphics state an operator needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    None,
    Cp,
    Font,
    CpFont,
}

/// What an operator does to the graphics state the model tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    None,
    SetCp,
    ClearCp,
    SetFont,
}

#[derive(Clone, Debug)]
struct Operand {
    /// The literal's text; `None` for an operand taken from the stack.
    text: Option<String>,
    item: Item,
    /// A literal integer's value, where the result depends on it.
    value: i64,
}

type ResultFn = fn(&mut Model, &[Operand], &mut Rng) -> Option<Vec<Item>>;

/// Where an operator may appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Restrict {
    Anywhere,
    /// Only in sequential code that runs exactly once: operators that
    /// change a tracked composite, which the model applies immediately.
    Straight,
    /// Not in a procedure body, whose dictionary context at call time is
    /// unknown.
    NotInProc,
}

#[derive(Clone, Copy)]
struct Op {
    /// The text after the operands: usually the operator's name.
    text: &'static str,
    inputs: &'static [Pat],
    result: ResultFn,
    need: Need,
    effect: Effect,
    weight: u32,
    restrict: Restrict,
    /// The result jumps at some value of a numeric input (a conversion,
    /// a comparison, a decimal rendering) or amplifies its rounding error
    /// (a product, a root), so an inexact number is never taken from the
    /// stack for it.
    exact_inputs: bool,
}

const fn op(text: &'static str, inputs: &'static [Pat], result: ResultFn, weight: u32) -> Op {
    Op {
        text,
        inputs,
        result,
        need: Need::None,
        effect: Effect::None,
        weight,
        restrict: Restrict::Anywhere,
        exact_inputs: false,
    }
}

const fn exact(o: Op) -> Op {
    Op {
        exact_inputs: true,
        ..o
    }
}

const fn straight(o: Op) -> Op {
    Op {
        restrict: Restrict::Straight,
        ..o
    }
}

const fn not_in_proc(o: Op) -> Op {
    Op {
        restrict: Restrict::NotInProc,
        ..o
    }
}

const fn gop(
    text: &'static str,
    inputs: &'static [Pat],
    need: Need,
    effect: Effect,
    result: ResultFn,
    weight: u32,
) -> Op {
    Op {
        text,
        inputs,
        result,
        need,
        effect,
        weight,
        restrict: Restrict::Anywhere,
        exact_inputs: false,
    }
}

// --- result functions -------------------------------------------------------

fn nothing(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(Vec::new())
}

fn a_bool(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::scalar(Ty::Bool)])
}

fn a_name(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::scalar(Ty::Name)])
}

fn an_exec_name(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::scalar(Ty::ExecName)])
}

fn small_int(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Int, 3)])
}

fn two_reals(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Real, 4), Item::num(Ty::Real, 4)])
}

fn dup(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![o[0].item.clone(), o[0].item.clone()])
}

fn exch(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![o[1].item.clone(), o[0].item.clone()])
}

/// A number computed by a continuous function of the operands is exact
/// only when they all are.
fn number(o: &[Operand], ty: Ty, mag: u8) -> Item {
    Item {
        inexact: o.iter().any(|operand| operand.item.inexact),
        ..Item::num(ty, mag)
    }
}

/// The type and magnitude of an arithmetic result over two numbers;
/// integers stay integers only below ten digits, reals below thirty.
fn arith(o: &[Operand], mag: u8) -> Option<Vec<Item>> {
    let (a, b) = (&o[0].item, &o[1].item);
    if a.ty == Ty::Int && b.ty == Ty::Int {
        (mag <= 9).then(|| vec![number(o, Ty::Int, mag)])
    } else {
        (mag <= 30).then(|| vec![number(o, Ty::Real, mag)])
    }
}

fn add_sub(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    arith(o, o[0].item.mag.max(o[1].item.mag) + 1)
}

fn mul(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    arith(o, o[0].item.mag + o[1].item.mag)
}

fn div(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let mag = o[0].item.mag + 1;
    (mag <= 30).then(|| vec![number(o, Ty::Real, mag)])
}

fn int_same(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Int, o[0].item.mag)])
}

fn same_num(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![number(o, o[0].item.ty, o[0].item.mag)])
}

fn rounded(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let a = &o[0].item;
    if a.ty == Ty::Int {
        Some(vec![a.clone()])
    } else {
        (a.mag < 30).then(|| vec![Item::num(Ty::Real, a.mag + 1)])
    }
}

fn real_same(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![number(o, Ty::Real, o[0].item.mag)])
}

fn real_small(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![number(o, Ty::Real, 1)])
}

fn degrees(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Real, 3)])
}

fn cvi(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let a = &o[0].item;
    (a.mag <= 9).then(|| vec![Item::num(Ty::Int, a.mag)])
}

fn shifted(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let mag = o[0].item.mag + 3;
    (mag <= 9).then(|| vec![Item::num(Ty::Int, mag)])
}

fn bitwise(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let (a, b) = (&o[0].item, &o[1].item);
    if a.ty == Ty::Bool {
        Some(vec![Item::scalar(Ty::Bool)])
    } else {
        let mag = a.mag.max(b.mag) + 1;
        (mag <= 9).then(|| vec![Item::num(Ty::Int, mag)])
    }
}

fn not(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let a = &o[0].item;
    Some(vec![if a.ty == Ty::Bool {
        Item::scalar(Ty::Bool)
    } else {
        Item::num(Ty::Int, a.mag)
    }])
}

fn opaque_string(m: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::opaque(Ty::String, m.epoch)])
}

fn new_string(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let len = usize::try_from(o[0].value).ok()?;
    Some(vec![m.alloc(Comp::Str {
        len,
        aliased: false,
    })])
}

fn new_array(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let len = usize::try_from(o[0].value).ok()?;
    Some(vec![m.alloc(Comp::Array {
        elems: vec![Item::scalar(Ty::Null); len],
        aliased: false,
    })])
}

fn new_dict(m: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![m.alloc(Comp::Dict {
        entries: Vec::new(),
    })])
}

fn array_get(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let elems = m.array_elems(&o[0].item)?;
    let index = usize::try_from(o[1].value).ok()?;
    Some(vec![elems.get(index)?.clone()])
}

fn array_put(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let id = o[0].item.id?;
    let index = usize::try_from(o[1].value).ok()?;
    let value = o[2].item.clone();
    match m.comp_mut(id) {
        Comp::Array { elems, .. } => {
            *elems.get_mut(index)? = value;
            Some(Vec::new())
        }
        _ => None,
    }
}

fn aload(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let mut items = m.array_elems(&o[0].item)?.to_vec();
    items.push(o[0].item.clone());
    Some(items)
}

fn aload_pop(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(m.array_elems(&o[0].item)?.to_vec())
}

fn forall_sum(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let elems = m.array_elems(&o[0].item)?;
    if elems.len() > 6 || elems.iter().any(|e| e.ty != Ty::Int || e.mag > 5) {
        return None;
    }
    Some(vec![Item::num(Ty::Int, 6)])
}

fn dict_known(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    m.dict_entries(&o[0].item)?;
    Some(vec![Item::scalar(Ty::Bool)])
}

fn dict_put(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let id = o[0].item.id?;
    let key = o[1].text.as_deref()?.strip_prefix('/')?.to_string();
    m.dict_put(id, &key, o[2].item.clone());
    Some(Vec::new())
}

fn dict_undef(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let id = o[0].item.id?;
    let key = o[1].text.as_deref()?.strip_prefix('/')?.to_string();
    m.dict_remove(id, &key);
    Some(Vec::new())
}

/// Whether the item is `userdict`, whose entry count the save/restore
/// relation changes with its named save: nothing may count it.
fn is_userdict(item: &Item) -> bool {
    item.ty == Ty::Dict && item.id == Some(0)
}

fn length(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    if is_userdict(&o[0].item) {
        return None;
    }
    m.length_of(&o[0].item)?;
    Some(vec![Item::num(Ty::Int, 3)])
}

fn current_dict(m: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item {
        id: Some(m.current_dict()),
        ..Item::scalar(Ty::Dict)
    }])
}

fn string_get(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Int, 3)])
}

/// `scale` by two literal factors in hundredths, kept inside the band.
fn scaled(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let (a, b) = (o[0].value, o[1].value);
    m.gfx = m.gfx.stretched(a.min(b), a.max(b))?;
    Some(Vec::new())
}

/// `concat` by a literal matrix whose stretch bounds the literal encodes
/// as `lo + 1000·hi` hundredths.
fn concatenated(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let (lo, hi) = (o[0].value % 1000, o[0].value / 1000);
    m.gfx = m.gfx.stretched(lo, hi)?;
    Some(Vec::new())
}

/// The operands in the order `index`… lists, so `roll`, `index`, and
/// `copy` forms are one table each.
fn permute(o: &[Operand], order: &[usize]) -> Option<Vec<Item>> {
    order
        .iter()
        .map(|&i| Some(o.get(i)?.item.clone()))
        .collect()
}

fn roll_3_1(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[2, 0, 1])
}

fn roll_3_back(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[1, 2, 0])
}

fn roll_4_1(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[3, 0, 1, 2])
}

fn roll_4_2(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[2, 3, 0, 1])
}

fn index_1(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[0, 1, 0])
}

fn index_2(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[0, 1, 2, 0])
}

fn copy_2(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[0, 1, 0, 1])
}

fn copy_3(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    permute(o, &[0, 1, 2, 0, 1, 2])
}

/// `astore` into a fresh array of the operands' length.
fn astore(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let elems = o.iter().map(|operand| operand.item.clone()).collect();
    Some(vec![m.alloc(Comp::Array {
        elems,
        aliased: false,
    })])
}

/// A sub-array sharing the source's storage: both become read-only for
/// the grammar, and the result is as old as its source, since `restore`
/// sees the storage, not the header.
fn array_getinterval(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let index = usize::try_from(o[1].value).ok()?;
    let count = usize::try_from(o[2].value).ok()?;
    let elems = m
        .array_elems(&o[0].item)?
        .get(index..index + count)?
        .to_vec();
    m.mark_aliased(o[0].item.id?);
    let mut item = m.alloc(Comp::Array {
        elems,
        aliased: true,
    });
    item.epoch = o[0].item.epoch;
    Some(vec![item])
}

fn string_getinterval(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let index = usize::try_from(o[1].value).ok()?;
    let count = usize::try_from(o[2].value).ok()?;
    if index + count > m.string_len(&o[0].item)? {
        return None;
    }
    m.mark_aliased(o[0].item.id?);
    let mut item = m.alloc(Comp::Str {
        len: count,
        aliased: true,
    });
    item.epoch = o[0].item.epoch;
    Some(vec![item])
}

fn array_putinterval(m: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let id = o[0].item.id?;
    let index = usize::try_from(o[1].value).ok()?;
    let source = m.array_elems(&o[2].item)?.to_vec();
    match m.comp_mut(id) {
        Comp::Array { elems, .. } => {
            let slots = elems.get_mut(index..index + source.len())?;
            slots.clone_from_slice(&source);
            Some(Vec::new())
        }
        _ => None,
    }
}

fn dict_count(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    if is_userdict(&o[0].item) {
        return None;
    }
    Some(vec![Item::num(Ty::Int, 2)])
}

fn byte_sum(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::num(Ty::Int, 4)])
}

/// Readings through the CTM: two or four reals a translation of the
/// program may move in the last digit.
fn two_readings(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::inexact_real(4); 2])
}

fn four_readings(_: &mut Model, _: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![Item::inexact_real(4); 4])
}

/// `ln` and `log` of a magnitude the model bounds stay below a hundred.
fn logarithm(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    Some(vec![number(o, Ty::Real, 2)])
}

/// A square root keeps the magnitude bound; a square doubles it.
fn squared(_: &mut Model, o: &[Operand], _: &mut Rng) -> Option<Vec<Item>> {
    let mag = o[0].item.mag * 2;
    (mag <= 30).then(|| vec![number(o, Ty::Real, mag)])
}

// --- the operator table ------------------------------------------------------

static CORE_OPS: &[Op] = &[
    op("pop", &[Pat::Any], nothing, 6),
    op("dup", &[Pat::Any], dup, 6),
    op("exch", &[Pat::Any, Pat::Any], exch, 6),
    op("count", &[], small_int, 1),
    op("add", &[Pat::Num, Pat::Num], add_sub, 8),
    op("sub", &[Pat::Num, Pat::Num], add_sub, 6),
    exact(op("mul", &[Pat::Num, Pat::Num], mul, 6)),
    exact(op("div", &[Pat::Num, Pat::NonZeroReal], div, 4)),
    op("idiv", &[Pat::Ty(Ty::Int), Pat::NonZeroInt], int_same, 3),
    op("mod", &[Pat::Ty(Ty::Int), Pat::NonZeroInt], int_same, 3),
    op("neg", &[Pat::Num], same_num, 2),
    op("abs", &[Pat::Num], same_num, 2),
    exact(op("abs sqrt", &[Pat::Num], real_same, 2)),
    op("sin", &[Pat::Num], real_small, 1),
    op("cos", &[Pat::Num], real_small, 1),
    exact(op("atan", &[Pat::Num, Pat::NonZeroInt], degrees, 1)),
    exact(op("round", &[Pat::Num], rounded, 2)),
    exact(op("truncate", &[Pat::Num], rounded, 1)),
    exact(op("floor", &[Pat::Num], rounded, 1)),
    exact(op("ceiling", &[Pat::Num], rounded, 1)),
    exact(op("cvi", &[Pat::Num], cvi, 3)),
    op("cvr", &[Pat::Num], real_same, 3),
    op("bitshift", &[Pat::Ty(Ty::Int), Pat::Shift], shifted, 2),
    op("and", &[Pat::IntOrBool, Pat::SameTy(0)], bitwise, 2),
    op("or", &[Pat::IntOrBool, Pat::SameTy(0)], bitwise, 2),
    op("xor", &[Pat::IntOrBool, Pat::SameTy(0)], bitwise, 2),
    op("not", &[Pat::IntOrBool], not, 2),
    exact(op("eq", &[Pat::Any, Pat::Any], a_bool, 3)),
    exact(op("ne", &[Pat::Any, Pat::Any], a_bool, 2)),
    exact(op("lt", &[Pat::NumOrStr, Pat::Like(0)], a_bool, 2)),
    exact(op("le", &[Pat::NumOrStr, Pat::Like(0)], a_bool, 2)),
    exact(op("gt", &[Pat::NumOrStr, Pat::Like(0)], a_bool, 2)),
    exact(op("ge", &[Pat::NumOrStr, Pat::Like(0)], a_bool, 2)),
    exact(op("32 string cvs", &[Pat::Any], opaque_string, 3)),
    // Radix 10 takes any integer; other radices only a non-negative one,
    // since the digits of a negative differ between interpreters.
    exact(op(
        "10 32 string cvrs",
        &[Pat::Ty(Ty::Int)],
        opaque_string,
        1,
    )),
    exact(op(
        "abs 16 32 string cvrs",
        &[Pat::Ty(Ty::Int)],
        opaque_string,
        1,
    )),
    op("abs 1 add ln", &[Pat::Num], logarithm, 1),
    op("abs 1 add log", &[Pat::Num], logarithm, 1),
    exact(op("abs 0.5 exp", &[Pat::Num], real_same, 1)),
    exact(op("abs 2 exp", &[Pat::Num], squared, 1)),
    op("3 1 roll", &[Pat::Any, Pat::Any, Pat::Any], roll_3_1, 2),
    op("3 -1 roll", &[Pat::Any, Pat::Any, Pat::Any], roll_3_back, 2),
    op("4 1 roll", &[Pat::Any; 4], roll_4_1, 1),
    op("4 2 roll", &[Pat::Any; 4], roll_4_2, 1),
    op("1 index", &[Pat::Any, Pat::Any], index_1, 2),
    op("2 index", &[Pat::Any, Pat::Any, Pat::Any], index_2, 1),
    op("2 copy", &[Pat::Any, Pat::Any], copy_2, 2),
    op("3 copy", &[Pat::Any, Pat::Any, Pat::Any], copy_3, 1),
    op("cvn", &[Pat::Ty(Ty::String)], a_name, 2),
    op("type", &[Pat::Any], an_exec_name, 2),
    op("xcheck", &[Pat::Any], a_bool, 1),
    op("length", &[Pat::Lengthy], length, 4),
    op(
        "get",
        &[Pat::StringTracked, Pat::IndexInto(0)],
        string_get,
        3,
    ),
    op(
        "put",
        &[Pat::StringWritable, Pat::IndexInto(0), Pat::Byte],
        nothing,
        3,
    ),
    op("string", &[Pat::SmallCount], new_string, 3),
    straight(op(
        "getinterval",
        &[
            Pat::StringTracked,
            Pat::IndexInto(0),
            Pat::IntervalCount(0, 1),
        ],
        string_getinterval,
        2,
    )),
    op(
        "putinterval",
        &[
            Pat::StringWritable,
            Pat::IndexInto(0),
            Pat::StringFitting(0, 1),
        ],
        nothing,
        2,
    ),
    op("{ pop } forall", &[Pat::Ty(Ty::String)], nothing, 1),
    op("0 exch { add } forall", &[Pat::StringTracked], byte_sum, 1),
    // Only literal text reaches `token`: a byte written by `put` could
    // open a string or a procedure the scanner never sees closed.
    op("token pop exch pop", &[Pat::NumText], small_int, 1),
    op("token { pop pop } if", &[Pat::WordText], nothing, 1),
    op(
        "search { pop pop pop (found) } { pop (missing) } ifelse",
        &[Pat::Ty(Ty::String), Pat::Text],
        opaque_string,
        2,
    ),
    op(
        "anchorsearch { pop pop (found) } { pop (missing) } ifelse",
        &[Pat::Ty(Ty::String), Pat::Text],
        opaque_string,
        1,
    ),
    op("array", &[Pat::SmallCount], new_array, 3),
    op("get", &[Pat::ArrayTracked, Pat::IndexInto(0)], array_get, 4),
    straight(op(
        "put",
        &[Pat::ArrayWritable, Pat::IndexInto(0), Pat::Any],
        array_put,
        4,
    )),
    straight(op(
        "getinterval",
        &[
            Pat::ArrayTracked,
            Pat::IndexInto(0),
            Pat::IntervalCount(0, 1),
        ],
        array_getinterval,
        2,
    )),
    straight(op(
        "putinterval",
        &[
            Pat::ArrayWritable,
            Pat::IndexInto(0),
            Pat::ArrayFitting(0, 1),
        ],
        array_putinterval,
        2,
    )),
    op("aload", &[Pat::ArrayTracked], aload, 2),
    op("aload pop", &[Pat::ArrayTracked], aload_pop, 3),
    op("0 array astore", &[], astore, 1),
    op("1 array astore", &[Pat::Any], astore, 1),
    op("2 array astore", &[Pat::Any, Pat::Any], astore, 2),
    op("3 array astore", &[Pat::Any, Pat::Any, Pat::Any], astore, 1),
    op("0 exch { add } forall", &[Pat::ArrayTracked], forall_sum, 2),
    op("{ pop } forall", &[Pat::ArrayTracked], nothing, 1),
    op("dict", &[Pat::SmallCount], new_dict, 3),
    op("maxlength", &[Pat::DictTracked], small_int, 1),
    op("known", &[Pat::DictTracked, Pat::NameKey], dict_known, 2),
    // Enumeration only counts, so insertion order stays unobserved.
    op("{ pop pop } forall", &[Pat::Ty(Ty::Dict)], nothing, 1),
    op(
        "0 exch { pop pop 1 add } forall",
        &[Pat::DictTracked],
        dict_count,
        1,
    ),
    straight(op(
        "put",
        &[Pat::DictTracked, Pat::NameKey, Pat::Any],
        dict_put,
        4,
    )),
    straight(op(
        "undef",
        &[Pat::DictTracked, Pat::NameKey],
        dict_undef,
        1,
    )),
    not_in_proc(op("currentdict", &[], current_dict, 1)),
    op("countdictstack", &[], small_int, 1),
    op("=", &[Pat::Any], nothing, 6),
    op("==", &[Pat::Any], nothing, 6),
    op("print", &[Pat::Ty(Ty::String)], nothing, 3),
];

static GRAPHICS_OPS: &[Op] = &[
    gop(
        "moveto",
        &[Pat::Coord, Pat::Coord],
        Need::None,
        Effect::SetCp,
        nothing,
        10,
    ),
    gop(
        "lineto",
        &[Pat::Coord, Pat::Coord],
        Need::Cp,
        Effect::SetCp,
        nothing,
        10,
    ),
    gop(
        "rmoveto",
        &[Pat::Delta, Pat::Delta],
        Need::Cp,
        Effect::SetCp,
        nothing,
        3,
    ),
    gop(
        "rlineto",
        &[Pat::Delta, Pat::Delta],
        Need::Cp,
        Effect::SetCp,
        nothing,
        5,
    ),
    gop(
        "curveto",
        &[
            Pat::Coord,
            Pat::Coord,
            Pat::Coord,
            Pat::Coord,
            Pat::Coord,
            Pat::Coord,
        ],
        Need::Cp,
        Effect::SetCp,
        nothing,
        4,
    ),
    gop(
        "rcurveto",
        &[
            Pat::Delta,
            Pat::Delta,
            Pat::Delta,
            Pat::Delta,
            Pat::Delta,
            Pat::Delta,
        ],
        Need::Cp,
        Effect::SetCp,
        nothing,
        2,
    ),
    gop(
        "arc",
        &[Pat::Coord, Pat::Coord, Pat::Radius, Pat::Angle, Pat::Angle],
        Need::None,
        Effect::SetCp,
        nothing,
        4,
    ),
    gop(
        "arcn",
        &[Pat::Coord, Pat::Coord, Pat::Radius, Pat::Angle, Pat::Angle],
        Need::None,
        Effect::SetCp,
        nothing,
        2,
    ),
    gop(
        "arcto",
        &[Pat::Corner],
        Need::None,
        Effect::SetCp,
        four_readings,
        2,
    ),
    gop("closepath", &[], Need::Cp, Effect::None, nothing, 4),
    gop("newpath", &[], Need::None, Effect::ClearCp, nothing, 3),
    gop("fill", &[], Need::None, Effect::ClearCp, nothing, 6),
    gop("eofill", &[], Need::None, Effect::ClearCp, nothing, 3),
    gop("stroke", &[], Need::None, Effect::ClearCp, nothing, 7),
    gop(
        "rectfill",
        &[Pat::Coord, Pat::Coord, Pat::Radius, Pat::Radius],
        Need::None,
        Effect::None,
        nothing,
        4,
    ),
    gop(
        "rectstroke",
        &[Pat::Coord, Pat::Coord, Pat::Radius, Pat::Radius],
        Need::None,
        Effect::None,
        nothing,
        3,
    ),
    gop("clip", &[], Need::None, Effect::None, nothing, 2),
    gop("eoclip", &[], Need::None, Effect::None, nothing, 1),
    gop("initclip", &[], Need::None, Effect::None, nothing, 1),
    gop(
        "setgray",
        &[Pat::Unit],
        Need::None,
        Effect::None,
        nothing,
        4,
    ),
    gop(
        "setrgbcolor",
        &[Pat::Unit, Pat::Unit, Pat::Unit],
        Need::None,
        Effect::None,
        nothing,
        4,
    ),
    gop(
        "setcmykcolor",
        &[Pat::Unit, Pat::Unit, Pat::Unit, Pat::Unit],
        Need::None,
        Effect::None,
        nothing,
        2,
    ),
    gop(
        "setlinewidth",
        &[Pat::LineWidth],
        Need::None,
        Effect::None,
        nothing,
        4,
    ),
    gop(
        "setlinecap",
        &[Pat::CapJoin],
        Need::None,
        Effect::None,
        nothing,
        2,
    ),
    gop(
        "setlinejoin",
        &[Pat::CapJoin],
        Need::None,
        Effect::None,
        nothing,
        2,
    ),
    gop(
        "setmiterlimit",
        &[Pat::Miter],
        Need::None,
        Effect::None,
        nothing,
        1,
    ),
    gop(
        "setflat",
        &[Pat::Flat],
        Need::None,
        Effect::None,
        nothing,
        1,
    ),
    gop(
        "setdash",
        &[Pat::DashArray, Pat::Phase],
        Need::None,
        Effect::None,
        nothing,
        2,
    ),
    straight(gop(
        "concat",
        &[Pat::Matrix],
        Need::None,
        Effect::None,
        concatenated,
        2,
    )),
    gop(
        "image grestore",
        &[Pat::ImageBlock],
        Need::None,
        Effect::None,
        nothing,
        2,
    ),
    // Scale changes only in sequential code, where the band is exact;
    // a loop body would compound them.
    straight(gop(
        "scale",
        &[Pat::ScaleFactor, Pat::ScaleFactor],
        Need::None,
        Effect::None,
        scaled,
        3,
    )),
    gop(
        "rotate",
        &[Pat::Rotation],
        Need::None,
        Effect::None,
        nothing,
        3,
    ),
    gop(
        "translate",
        &[Pat::Delta, Pat::Delta],
        Need::None,
        Effect::None,
        nothing,
        3,
    ),
    gop(
        "matrix currentmatrix setmatrix",
        &[],
        Need::None,
        Effect::None,
        nothing,
        1,
    ),
    gop(
        "matrix currentmatrix pop",
        &[],
        Need::None,
        Effect::None,
        nothing,
        1,
    ),
    gop(
        "findfont exch scalefont setfont",
        &[Pat::FontSize, Pat::FontName],
        Need::None,
        Effect::SetFont,
        nothing,
        5,
    ),
    gop(
        "show",
        &[Pat::Text],
        Need::CpFont,
        Effect::SetCp,
        nothing,
        6,
    ),
    gop(
        "stringwidth",
        &[Pat::Text],
        Need::Font,
        Effect::None,
        two_reals,
        2,
    ),
    gop(
        "true charpath",
        &[Pat::Text],
        Need::CpFont,
        Effect::SetCp,
        nothing,
        2,
    ),
    gop(
        "false charpath",
        &[Pat::Text],
        Need::CpFont,
        Effect::SetCp,
        nothing,
        2,
    ),
    gop(
        "xshow",
        &[Pat::StringTracked, Pat::Advances(0)],
        Need::CpFont,
        Effect::SetCp,
        nothing,
        2,
    ),
    gop(
        "{ pop pop } exch kshow",
        &[Pat::Text],
        Need::CpFont,
        Effect::SetCp,
        nothing,
        1,
    ),
    gop(
        "currentpoint pop pop",
        &[],
        Need::Cp,
        Effect::None,
        nothing,
        1,
    ),
    // User-space readings: a translation prepended to the program moves
    // the user space with them, so the relation tolerates both.
    gop("currentpoint", &[], Need::Cp, Effect::None, two_readings, 1),
    gop("pathbbox", &[], Need::Cp, Effect::None, four_readings, 1),
    gop(
        "pathbbox pop pop pop pop",
        &[],
        Need::Cp,
        Effect::None,
        nothing,
        1,
    ),
];

/// What a generated body did to the stack it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BodyEffect {
    /// The stack is as it was.
    Balanced,
    /// The stack is as it was with these items on top.
    Pushes(Vec<Item>),
}

#[derive(Clone, Copy, Debug)]
struct Ctx {
    depth: usize,
    /// Whether definitions, save blocks, dictionary scopes, and ill-typed
    /// statements may appear: true in sequential code, false in bodies
    /// that may run zero or several times.
    straight: bool,
    /// Inside a procedure body: no name references, no graphics.
    in_proc: bool,
}

impl Ctx {
    const TOP: Ctx = Ctx {
        depth: 0,
        straight: true,
        in_proc: false,
    };

    fn nested(self) -> Ctx {
        Ctx {
            depth: self.depth + 1,
            ..self
        }
    }

    fn body(self) -> Ctx {
        Ctx {
            depth: self.depth + 1,
            straight: false,
            in_proc: self.in_proc,
        }
    }
}

pub struct Generator<'a> {
    rng: Rng,
    profile: &'a Profile,
    model: Model,
    next_var: u32,
    next_proc: u32,
    next_save: u32,
    next_counter: u32,
}

/// Generates program `index` of run `seed` under `profile`.
pub fn generate(profile: &Profile, seed: u64, index: u64) -> Program {
    let origin = Origin {
        profile: profile.name.to_string(),
        seed,
        index,
    };
    let mut generator = Generator {
        rng: Rng::new(seed, index),
        profile,
        model: Model::new(),
        next_var: 0,
        next_proc: 0,
        next_save: 0,
        next_counter: 0,
    };
    let statements = generator.program();
    Program {
        header: program::header(&origin),
        statements,
    }
}

impl<'a> Generator<'a> {
    fn bounds(&self) -> &'a crate::profile::Bounds {
        &self.profile.bounds
    }

    fn program(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        if self.profile.kind == Kind::Graphics {
            let sizes = [(612, 792), (595, 842), (400, 400), (300, 500)];
            let (w, h) = *self.rng.pick(&sizes);
            out.push(format!("<< /PageSize [{w} {h}] >> setpagedevice"));
        }
        let (lo, hi) = self.bounds().statements;
        let n = self.rng.range(lo as i64, hi as i64) as usize;
        for _ in 0..n {
            if let Some(statement) = self.statement(Ctx::TOP) {
                out.push(statement);
            }
        }
        if self.profile.kind == Kind::Graphics {
            out.push("showpage".to_string());
        }
        out.push("pstack".to_string());
        out
    }

    // --- statements ---------------------------------------------------------

    /// One statement, or none when nothing applies after a few tries.
    fn statement(&mut self, ctx: Ctx) -> Option<String> {
        for _ in 0..8 {
            let saved = self.model.clone();
            let candidate = self.try_statement(ctx);
            match candidate {
                Some(text) if self.model.depth() <= self.bounds().max_stack => return Some(text),
                _ => self.model = saved,
            }
        }
        None
    }

    fn try_statement(&mut self, ctx: Ctx) -> Option<String> {
        let graphics = self.profile.kind == Kind::Graphics && !ctx.in_proc;
        if ctx.straight && self.rng.chance(self.profile.ill_typed) {
            return self.ill_typed_statement(ctx, graphics);
        }
        let roll = self.rng.below(100);
        let compound_allowed = ctx.depth + 1 < self.bounds().depth;
        match roll {
            0..=11 if !graphics || self.rng.chance(500) => self.literal_statement(),
            12..=16 if ctx.straight && !ctx.in_proc => self.def_statement(ctx),
            17..=19 if ctx.straight && !ctx.in_proc => self.proc_def(ctx),
            20..=27 if !ctx.in_proc => self.name_statement(),
            28..=41 if compound_allowed => self.compound(ctx),
            _ => {
                if graphics && self.rng.chance(650) {
                    self.op_from(ctx, GRAPHICS_OPS, false)
                } else {
                    self.op_from(ctx, CORE_OPS, false)
                }
            }
        }
    }

    fn literal_statement(&mut self) -> Option<String> {
        let ty = *self.rng.pick(&[
            Ty::Int,
            Ty::Int,
            Ty::Real,
            Ty::Bool,
            Ty::String,
            Ty::Name,
            Ty::Array,
            Ty::Dict,
            Ty::Proc,
            Ty::Null,
        ]);
        let operand = self.literal_of(ty)?;
        self.model.push(operand.item);
        operand.text
    }

    /// An operator statement from a table, weighted, among the operators
    /// the context allows.
    fn op_from(&mut self, ctx: Ctx, table: &[Op], ill: bool) -> Option<String> {
        let allowed: Vec<&Op> = table
            .iter()
            .filter(|o| match o.restrict {
                Restrict::Anywhere => true,
                Restrict::Straight => ctx.straight,
                Restrict::NotInProc => !ctx.in_proc,
            })
            .collect();
        let total: u32 = allowed.iter().map(|o| o.weight).sum();
        let mut pick = self.rng.below(total as usize) as u32;
        let op = allowed
            .iter()
            .find(|o| {
                if pick < o.weight {
                    true
                } else {
                    pick -= o.weight;
                    false
                }
            })
            .expect("weights sum to total");
        self.op_statement(op, ill)
    }

    fn gfx_ok(&self, need: Need) -> bool {
        let g = self.model.gfx;
        match need {
            Need::None => true,
            Need::Cp => g.current_point,
            Need::Font => g.font,
            Need::CpFont => g.current_point && g.font,
        }
    }

    fn op_statement(&mut self, op: &Op, ill: bool) -> Option<String> {
        if !ill && !self.gfx_ok(op.need) {
            return None;
        }
        let n = op.inputs.len();
        let k = if ill { 0 } else { self.operands_from_stack(op) };
        let mut operands: Vec<Operand> = Vec::with_capacity(n);
        for j in 0..k {
            let item = self.model.peek(k - 1 - j)?.clone();
            operands.push(Operand {
                text: None,
                item,
                value: 0,
            });
        }
        for &pat in &op.inputs[k..] {
            let operand = self.literal_for(pat, &operands)?;
            operands.push(operand);
        }
        if ill {
            let position = self.ill_position(op, &operands)?;
            let wrong = self.wrong_literal(op.inputs[position], &operands)?;
            operands[position] = wrong;
        }
        let results = if ill {
            Vec::new()
        } else {
            (op.result)(&mut self.model, &operands, &mut self.rng)?
        };
        let mut text: Vec<String> = operands.iter().filter_map(|o| o.text.clone()).collect();
        text.push(op.text.to_string());
        let text = text.join(" ");
        if ill {
            // Cleared whether or not the operator objected, so the model
            // knows the stack either way.
            self.model.stack.clear();
            return Some(format!("{{ {text} }} stopped {HANDLER} clear"));
        }
        for _ in 0..k {
            self.model.pop();
        }
        for item in results {
            self.model.push(item);
        }
        match op.effect {
            Effect::None => {}
            Effect::SetCp => self.model.gfx.current_point = true,
            Effect::ClearCp => self.model.gfx.current_point = false,
            Effect::SetFont => self.model.gfx.font = true,
        }
        Some(text)
    }

    /// How many of the operator's first inputs to take from the stack:
    /// the most it can, usually, else a smaller feasible count.
    fn operands_from_stack(&mut self, op: &Op) -> usize {
        let n = op.inputs.len().min(self.model.depth());
        let mut feasible = Vec::new();
        for k in 0..=n {
            let mut operands: Vec<Operand> = Vec::new();
            let mut ok = true;
            for j in 0..k {
                let item = self.model.peek(k - 1 - j).expect("within depth").clone();
                if !op.inputs[j].accepts(&item, &operands, &self.model)
                    || (op.exact_inputs && item.inexact)
                {
                    ok = false;
                    break;
                }
                operands.push(Operand {
                    text: None,
                    item,
                    value: 0,
                });
            }
            if ok {
                feasible.push(k);
            }
        }
        let max = *feasible.last().unwrap_or(&0);
        if self.rng.chance(700) {
            max
        } else {
            *self.rng.pick(&feasible)
        }
    }

    fn ill_typed_statement(&mut self, ctx: Ctx, graphics: bool) -> Option<String> {
        if graphics && self.rng.chance(500) {
            self.op_from(ctx, GRAPHICS_OPS, true)
        } else {
            self.op_from(ctx, CORE_OPS, true)
        }
    }

    /// A position whose pattern rejects some type.
    fn ill_position(&mut self, op: &Op, operands: &[Operand]) -> Option<usize> {
        let restrictive: Vec<usize> = (0..op.inputs.len())
            .filter(|&j| !op.inputs[j].block())
            .filter(|&j| {
                WRONG_CANDIDATES
                    .iter()
                    .any(|&ty| !op.inputs[j].accepts_ty(ty, &operands[..j]))
            })
            .collect();
        if restrictive.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&restrictive))
        }
    }

    fn wrong_literal(&mut self, pat: Pat, operands: &[Operand]) -> Option<Operand> {
        let choices: Vec<Ty> = WRONG_CANDIDATES
            .iter()
            .copied()
            .filter(|&ty| !pat.accepts_ty(ty, operands))
            .collect();
        let ty = *self.rng.pick(&choices);
        self.literal_of(ty)
    }

    // --- literals -----------------------------------------------------------

    fn int_text(&mut self, lo: i64, hi: i64) -> (String, i64) {
        let v = self.rng.range(lo, hi);
        (v.to_string(), v)
    }

    /// A real with two decimals from a count of hundredths.
    fn real_text(hundredths: i64) -> String {
        let sign = if hundredths < 0 { "-" } else { "" };
        let h = hundredths.abs();
        format!("{sign}{}.{:02}", h / 100, h % 100)
    }

    fn literal_of(&mut self, ty: Ty) -> Option<Operand> {
        let max = self.bounds().int_max;
        let operand = match ty {
            Ty::Int => {
                let (text, value) = self.int_text(-max, max);
                Operand {
                    text: Some(text),
                    item: Item::num(Ty::Int, 4),
                    value,
                }
            }
            Ty::Real => {
                let h = self.rng.range(-max * 100, max * 100);
                Operand {
                    text: Some(Self::real_text(h)),
                    item: Item::num(Ty::Real, 4),
                    value: 0,
                }
            }
            Ty::Bool => Operand {
                text: Some(
                    if self.rng.chance(500) {
                        "true"
                    } else {
                        "false"
                    }
                    .to_string(),
                ),
                item: Item::scalar(Ty::Bool),
                value: 0,
            },
            Ty::String => {
                let len = self.rng.below(self.bounds().string_len + 1);
                self.string_literal(len)
            }
            Ty::Name => Operand {
                text: Some(format!("/{}", self.rng.pick(&KEYS))),
                item: Item::scalar(Ty::Name),
                value: 0,
            },
            Ty::Array => {
                let len = self.rng.below(self.bounds().array_len + 1);
                self.array_literal(len)?
            }
            Ty::Dict => {
                let len = self.rng.below(4);
                let mut texts = vec!["<<".to_string()];
                let mut entries: Vec<(String, Item)> = Vec::new();
                for _ in 0..len {
                    let key = self.rng.pick(&KEYS).to_string();
                    let ty = *self.rng.pick(&[Ty::Int, Ty::Real, Ty::Bool, Ty::String]);
                    let value = self.literal_of(ty)?;
                    texts.push(format!("/{key}"));
                    texts.push(value.text?);
                    match entries.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value.item,
                        None => entries.push((key, value.item)),
                    }
                }
                texts.push(">>".to_string());
                let item = self.model.alloc(Comp::Dict { entries });
                Operand {
                    text: Some(texts.join(" ")),
                    item,
                    value: len as i64,
                }
            }
            Ty::Proc => {
                // A procedure literal as data, with the signature its
                // body declares, so `exec` may call it later.
                let (text, inputs, outputs) = self.proc_literal(Ctx::TOP.body())?;
                let item = self.model.alloc(Comp::Proc { inputs, outputs });
                Operand {
                    text: Some(text),
                    item,
                    value: 0,
                }
            }
            Ty::Null => Operand {
                text: Some("null".to_string()),
                item: Item::scalar(Ty::Null),
                value: 0,
            },
            Ty::Mark => Operand {
                text: Some("mark".to_string()),
                item: Item::scalar(Ty::Mark),
                value: 0,
            },
            Ty::Any | Ty::ExecName => return None,
        };
        Some(operand)
    }

    fn literal_for(&mut self, pat: Pat, operands: &[Operand]) -> Option<Operand> {
        let max = self.bounds().int_max;
        let coord = self.bounds().coord_max.max(1);
        let int_operand = |text: String, value: i64, mag: u8| Operand {
            text: Some(text),
            item: Item::num(Ty::Int, mag),
            value,
        };
        let real_operand = |hundredths: i64, mag: u8| Operand {
            text: Some(Self::real_text(hundredths)),
            item: Item::num(Ty::Real, mag),
            value: 0,
        };
        let operand = match pat {
            Pat::Any => {
                let ty = *self.rng.pick(&[
                    Ty::Int,
                    Ty::Real,
                    Ty::Bool,
                    Ty::String,
                    Ty::Name,
                    Ty::Array,
                    Ty::Null,
                ]);
                self.literal_of(ty)?
            }
            Pat::Ty(ty) => self.literal_of(ty)?,
            Pat::Num => {
                let ty = if self.rng.chance(600) {
                    Ty::Int
                } else {
                    Ty::Real
                };
                self.literal_of(ty)?
            }
            Pat::IntOrBool => {
                let ty = if self.rng.chance(600) {
                    Ty::Int
                } else {
                    Ty::Bool
                };
                self.literal_of(ty)?
            }
            Pat::NumOrStr => {
                let ty = *self.rng.pick(&[Ty::Int, Ty::Real, Ty::String]);
                self.literal_of(ty)?
            }
            Pat::Like(j) => {
                let ty = match operands.get(j)?.item.ty {
                    Ty::Int | Ty::Real => {
                        if self.rng.chance(600) {
                            Ty::Int
                        } else {
                            Ty::Real
                        }
                    }
                    other => other,
                };
                self.literal_of(ty)?
            }
            Pat::SameTy(j) => {
                let ty = operands.get(j)?.item.ty;
                self.literal_of(ty)?
            }
            Pat::Lengthy => {
                let ty = *self.rng.pick(&[Ty::String, Ty::Array, Ty::Dict]);
                self.literal_of(ty)?
            }
            Pat::ArrayTracked | Pat::ArrayWritable => self.literal_of(Ty::Array)?,
            Pat::StringTracked | Pat::StringWritable | Pat::Text => self.literal_of(Ty::String)?,
            Pat::DictTracked => self.literal_of(Ty::Dict)?,
            Pat::Coord => {
                if self.rng.chance(700) {
                    let (text, value) = self.int_text(0, coord);
                    int_operand(text, value, 3)
                } else {
                    let h = self.rng.range(0, coord * 100);
                    real_operand(h, 3)
                }
            }
            Pat::Delta => {
                let (text, value) = self.int_text(-50, 50);
                int_operand(text, value, 2)
            }
            Pat::NonZeroInt => {
                let mut v = self.rng.range(-max, max - 1);
                if v >= 0 {
                    v += 1;
                }
                int_operand(v.to_string(), v, 4)
            }
            Pat::NonZeroReal => {
                let mut h = self.rng.range(-max * 100, max * 100 - 100);
                if h > -50 {
                    h += 100;
                }
                real_operand(h, 4)
            }
            Pat::Byte => {
                let (text, value) = self.int_text(0, 255);
                int_operand(text, value, 3)
            }
            Pat::Shift => {
                let (text, value) = self.int_text(-8, 8);
                int_operand(text, value, 1)
            }
            Pat::SmallCount => {
                let (text, value) = self.int_text(0, self.bounds().array_len as i64);
                int_operand(text, value, 1)
            }
            Pat::IndexInto(j) => {
                let len = self.model.length_of(&operands.get(j)?.item)?;
                if len == 0 {
                    return None;
                }
                let (text, value) = self.int_text(0, len as i64 - 1);
                int_operand(text, value, 1)
            }
            Pat::NameKey => self.literal_of(Ty::Name)?,
            Pat::Radius => {
                let (text, value) = self.int_text(1, 120);
                int_operand(text, value, 3)
            }
            Pat::Angle => {
                let v = self.rng.range(0, 24) * 15;
                int_operand(v.to_string(), v, 3)
            }
            Pat::Unit => {
                let h = self.rng.range(0, 100);
                real_operand(h, 1)
            }
            Pat::LineWidth => {
                if self.rng.chance(500) {
                    let (text, value) = self.int_text(0, 8);
                    int_operand(text, value, 1)
                } else {
                    let h = self.rng.range(0, 800);
                    real_operand(h, 1)
                }
            }
            Pat::CapJoin => {
                let (text, value) = self.int_text(0, 2);
                int_operand(text, value, 1)
            }
            Pat::Miter => {
                let (text, value) = self.int_text(1, 10);
                int_operand(text, value, 2)
            }
            Pat::Flat => {
                let h = self.rng.range(20, 500);
                real_operand(h, 1)
            }
            Pat::ScaleFactor => {
                let h = self.rng.range(50, 200);
                Operand {
                    value: h,
                    ..real_operand(h, 1)
                }
            }
            Pat::Rotation => {
                let v = self.rng.range(-6, 6) * 15;
                int_operand(v.to_string(), v, 2)
            }
            Pat::FontName => Operand {
                text: Some(format!("/{}", self.rng.pick(&FONTS))),
                item: Item::scalar(Ty::Name),
                value: 0,
            },
            Pat::FontSize => {
                let (text, value) = self.int_text(6, 36);
                int_operand(text, value, 2)
            }
            Pat::IntervalCount(i, j) => {
                let len = self.model.length_of(&operands.get(i)?.item)?;
                let index = usize::try_from(operands.get(j)?.value).ok()?;
                let room = len.checked_sub(index)?;
                let (text, value) = self.int_text(0, room as i64);
                int_operand(text, value, 1)
            }
            Pat::ArrayFitting(i, j) | Pat::StringFitting(i, j) => {
                let len = self.model.length_of(&operands.get(i)?.item)?;
                let index = usize::try_from(operands.get(j)?.value).ok()?;
                let room = len.checked_sub(index)?;
                let count = self.rng.below(room + 1);
                if matches!(pat, Pat::ArrayFitting(..)) {
                    self.array_literal(count)?
                } else {
                    self.string_literal(count)
                }
            }
            Pat::Advances(i) => {
                let len = self.model.length_of(&operands.get(i)?.item)?;
                let advances: Vec<String> = (0..len)
                    .map(|_| {
                        if self.rng.chance(700) {
                            self.rng.range(0, 20).to_string()
                        } else {
                            Self::real_text(self.rng.range(0, 2000))
                        }
                    })
                    .collect();
                Operand {
                    text: Some(format!("[ {} ]", advances.join(" "))),
                    item: Item::opaque(Ty::Array, self.model.epoch),
                    value: len as i64,
                }
            }
            Pat::NumText => {
                let count = self.rng.range(1, 3);
                let numbers: Vec<String> = (0..count)
                    .map(|_| self.rng.range(0, 999).to_string())
                    .collect();
                self.text_operand(numbers.join(" "))
            }
            Pat::WordText => {
                let count = self.rng.range(1, 3);
                let words: Vec<String> = (0..count)
                    .map(|_| self.rng.pick(&KEYS).to_string())
                    .collect();
                self.text_operand(words.join(" "))
            }
            Pat::DashArray => {
                let count = self.rng.range(0, 3);
                let mut lengths: Vec<i64> = (0..count).map(|_| self.rng.range(0, 12)).collect();
                if !lengths.is_empty() && lengths.iter().all(|&l| l == 0) {
                    lengths[0] = 1;
                }
                let texts: Vec<String> = lengths.iter().map(i64::to_string).collect();
                Operand {
                    text: Some(format!("[ {} ]", texts.join(" "))),
                    item: Item::opaque(Ty::Array, self.model.epoch),
                    value: count,
                }
            }
            Pat::Phase => {
                let (text, value) = self.int_text(0, 5);
                int_operand(text, value, 1)
            }
            Pat::Matrix => {
                let scale = |g: &mut Self| -> i64 { *g.rng.pick(&[50, 75, 100, 150, 200]) };
                let skew = |g: &mut Self| -> i64 { *g.rng.pick(&[0, 0, 0, 25, -25]) };
                let a = scale(self);
                let d = scale(self);
                let mut b = skew(self);
                let mut c = skew(self);
                // Bounds on the singular values of a diagonally dominant
                // matrix: the diagonal less or plus the off-diagonal;
                // a skew that would leave the matrix nearly singular is
                // dropped.
                if a.min(d) - b.abs() - c.abs() < 25 {
                    b = 0;
                    c = 0;
                }
                let lo = a.min(d) - b.abs() - c.abs();
                let hi = a.max(d) + b.abs() + c.abs();
                let e = self.rng.range(-50, 50);
                let f = self.rng.range(-50, 50);
                Operand {
                    text: Some(format!(
                        "[ {} {} {} {} {e} {f} ]",
                        Self::real_text(a),
                        Self::real_text(b),
                        Self::real_text(c),
                        Self::real_text(d)
                    )),
                    item: Item::opaque(Ty::Array, self.model.epoch),
                    value: lo + 1000 * hi,
                }
            }
            Pat::Corner => {
                // The corner's angle stays between 45 and 135 degrees and
                // is never a right angle: an acute corner puts the
                // tangent points far along the lines, where rounding of
                // the angle moves them, and a quarter-turn sweep sits on
                // the boundary where the arc's piece count flips with
                // rounding of the current point.
                let x = self.rng.range(0, coord);
                let y = self.rng.range(0, coord);
                let sign = |g: &mut Self| if g.rng.chance(500) { 1 } else { -1 };
                let dx = self.rng.range(10, 100) * sign(self);
                let ex = self.rng.range(10, 100) * sign(self);
                let dy = self.rng.range(ex.abs(), 100) * sign(self);
                let r = self.rng.range(1, 60);
                Operand {
                    text: Some(format!(
                        "{x} {y} moveto {} {y} {} {} {r}",
                        x + dx,
                        x + dx + ex,
                        y + dy
                    )),
                    item: Item::scalar(Ty::Null),
                    value: 0,
                }
            }
            Pat::ImageBlock => {
                let x = self.rng.range(0, coord);
                let y = self.rng.range(0, coord);
                let sw = self.rng.range(10, 120);
                let sh = self.rng.range(10, 120);
                let w = self.rng.range(1, 4);
                let h = self.rng.range(1, 4);
                let hex: String = (0..w * h)
                    .map(|_| format!("{:02x}", self.rng.below(256)))
                    .collect();
                let source = if self.rng.chance(600) {
                    format!("<{hex}>")
                } else {
                    format!("{{ <{hex}> }}")
                };
                Operand {
                    text: Some(format!(
                        "gsave {x} {y} translate {sw} {sh} scale {w} {h} 8 [ {w} 0 0 -{h} 0 {h} ] {source}"
                    )),
                    item: Item::scalar(Ty::Null),
                    value: 0,
                }
            }
        };
        Some(operand)
    }

    /// A string literal of the given text.
    fn text_operand(&mut self, text: String) -> Operand {
        let item = self.model.alloc(Comp::Str {
            len: text.len(),
            aliased: false,
        });
        Operand {
            text: Some(format!("({text})")),
            item,
            value: text.len() as i64,
        }
    }

    /// A string literal of `len` letters.
    fn string_literal(&mut self, len: usize) -> Operand {
        let text: String = (0..len)
            .map(|_| (b'a' + self.rng.below(26) as u8) as char)
            .collect();
        self.text_operand(text)
    }

    /// An array literal of `len` scalar elements.
    fn array_literal(&mut self, len: usize) -> Option<Operand> {
        let mut texts = vec!["[".to_string()];
        let mut elems = Vec::new();
        for _ in 0..len {
            let ty = *self
                .rng
                .pick(&[Ty::Int, Ty::Int, Ty::Real, Ty::Bool, Ty::Name]);
            let elem = self.literal_of(ty)?;
            texts.push(elem.text?);
            elems.push(elem.item);
        }
        texts.push("]".to_string());
        let item = self.model.alloc(Comp::Array {
            elems,
            aliased: false,
        });
        Some(Operand {
            text: Some(texts.join(" ")),
            item,
            value: len as i64,
        })
    }

    // --- names --------------------------------------------------------------

    /// `/vN <closed expression> def` or `/vN exch def`.
    fn def_statement(&mut self, ctx: Ctx) -> Option<String> {
        let name = format!("v{}", self.next_var);
        self.next_var += 1;
        if self.model.peek(0).is_some_and(|i| i.ty != Ty::ExecName) && self.rng.chance(300) {
            let item = self.model.pop()?;
            self.model.define(&name, item);
            return Some(format!("/{name} exch def"));
        }
        let (text, item) = self.closed_expression(ctx)?;
        self.model.define(&name, item);
        Some(format!("/{name} {text} def"))
    }

    /// An expression that pushes exactly one value without touching the
    /// stack beneath it: a literal, or a literal operator application.
    fn closed_expression(&mut self, _ctx: Ctx) -> Option<(String, Item)> {
        if self.rng.chance(700) {
            let ty = *self.rng.pick(&[
                Ty::Int,
                Ty::Real,
                Ty::Bool,
                Ty::String,
                Ty::Name,
                Ty::Array,
                Ty::Dict,
                Ty::Null,
            ]);
            let operand = self.literal_of(ty)?;
            return Some((operand.text?, operand.item));
        }
        let saved = self.model.stack.clone();
        self.model.stack.clear();
        let candidates: Vec<&Op> = CORE_OPS
            .iter()
            .filter(|o| {
                matches!(
                    o.text,
                    "add"
                        | "sub"
                        | "mul"
                        | "div"
                        | "idiv"
                        | "mod"
                        | "neg"
                        | "abs"
                        | "cvi"
                        | "cvr"
                        | "string"
                        | "array"
                        | "dict"
                        | "length"
                        | "eq"
                        | "lt"
                        | "gt"
                        | "abs 1 add ln"
                        | "abs 1 add log"
                        | "abs 0.5 exp"
                        | "abs 2 exp"
                        | "10 32 string cvrs"
                )
            })
            .collect();
        let op = self.rng.pick(&candidates);
        let text = self.op_statement(op, false);
        let result = self.model.stack.pop();
        self.model.stack = saved;
        let (text, item) = (text?, result?);
        Some((text, item))
    }

    /// A reference to a visible name: a variable pushes its value, a
    /// procedure runs with its signature, `load exec` runs it too.
    fn name_statement(&mut self) -> Option<String> {
        let names = self.model.visible_names();
        let mut candidates = Vec::new();
        for name in names {
            let item = self.model.lookup(&name)?.clone();
            match item.ty {
                Ty::Proc => {
                    if self.call_matches(&item) {
                        candidates.push((name, item));
                    }
                }
                Ty::ExecName => {}
                _ => candidates.push((name, item)),
            }
        }
        if candidates.is_empty() {
            return None;
        }
        let (name, item) = self.rng.pick(&candidates).clone();
        if item.ty == Ty::Proc {
            let (inputs, outputs) = self.model.proc_sig(&item)?;
            let (inputs, outputs) = (inputs.to_vec(), outputs.to_vec());
            for _ in 0..inputs.len() {
                self.model.pop();
            }
            let epoch = self.model.epoch;
            for out in outputs {
                self.model.push(Item {
                    id: None,
                    epoch: if out.ty.is_composite() { epoch } else { 0 },
                    ..out
                });
            }
            let form = if self.rng.chance(750) {
                name
            } else {
                format!("/{name} load exec")
            };
            return Some(form);
        }
        self.model.push(item);
        Some(if self.rng.chance(800) {
            name
        } else {
            format!("/{name} load")
        })
    }

    /// Whether the stack top satisfies a procedure's declared inputs.
    fn call_matches(&self, item: &Item) -> bool {
        let Some((inputs, _)) = self.model.proc_sig(item) else {
            return false;
        };
        let n = inputs.len();
        if self.model.depth() < n {
            return false;
        }
        inputs.iter().enumerate().all(|(j, input)| {
            let actual = self.model.peek(n - 1 - j).expect("within depth");
            actual.ty == input.ty
                && (!input.ty.is_num() || actual.mag <= input.mag)
                && !actual.inexact
        })
    }

    /// `/pN { body } bind def` with a declared signature.
    fn proc_def(&mut self, ctx: Ctx) -> Option<String> {
        let (text, inputs, outputs) = self.proc_literal(ctx.body())?;
        let name = format!("p{}", self.next_proc);
        self.next_proc += 1;
        let item = self.model.alloc(Comp::Proc { inputs, outputs });
        self.model.define(&name, item);
        Some(format!("/{name} {text} bind def"))
    }

    /// A procedure body generated against its declared inputs alone; the
    /// outputs are whatever it leaves, with composites opaque.
    fn proc_literal(&mut self, ctx: Ctx) -> Option<(String, Vec<Item>, Vec<Item>)> {
        let ctx = Ctx {
            in_proc: true,
            ..ctx
        };
        let n = self.rng.below(self.bounds().proc_inputs + 1);
        let inputs: Vec<Item> = (0..n)
            .map(|_| match self.rng.below(5) {
                0 => Item::num(Ty::Real, 4),
                1 => Item::scalar(Ty::Bool),
                2 => Item::opaque(Ty::String, 0),
                _ => Item::num(Ty::Int, 4),
            })
            .collect();
        let saved = self.model.clone();
        self.model.stack = inputs.clone();
        let (lo, hi) = self.bounds().proc_body;
        let count = self.rng.range(lo as i64, hi as i64) as usize;
        let mut statements = Vec::new();
        for _ in 0..count {
            if let Some(s) = self.statement(ctx) {
                statements.push(s);
            }
        }
        let outputs: Vec<Item> = self
            .model
            .stack
            .iter()
            .map(|item| Item {
                id: None,
                ..item.clone()
            })
            .collect();
        self.model = saved;
        if outputs.len() > 6 {
            return None;
        }
        Some((format!("{{ {} }}", statements.join(" ")), inputs, outputs))
    }

    // --- compound statements ------------------------------------------------

    fn compound(&mut self, ctx: Ctx) -> Option<String> {
        let graphics = self.profile.kind == Kind::Graphics && !ctx.in_proc;
        let roll = self.rng.below(100);
        match roll {
            0..=14 => self.if_statement(ctx),
            15..=24 => self.ifelse_statement(ctx),
            25..=39 => self.repeat_statement(ctx),
            40..=54 => self.for_statement(ctx),
            55..=62 => self.loop_statement(ctx),
            63..=72 => self.stopped_block(ctx),
            73..=82 if ctx.straight && !ctx.in_proc => self.save_block(ctx),
            83..=89 if ctx.straight && !ctx.in_proc => self.dict_scope(ctx),
            _ if graphics => self.gsave_block(ctx),
            _ => self.stopped_block(ctx),
        }
    }

    /// A sequence of statements for a nested block.
    fn block(&mut self, ctx: Ctx) -> Vec<String> {
        let (lo, hi) = self.bounds().block;
        let count = self.rng.range(lo as i64, hi as i64) as usize;
        let mut statements = Vec::new();
        for _ in 0..count {
            if let Some(s) = self.statement(ctx) {
                statements.push(s);
            }
        }
        statements
    }

    /// A body run against the current stack plus `extra`, accepted when
    /// it leaves that stack unchanged. A body that may also push is
    /// generated against an empty stack instead, so that it cannot read
    /// what an earlier iteration pushed; it is then accepted when it
    /// leaves nothing or only pushes. A body that clears a current point
    /// it started with is rejected, since its next iteration would find
    /// none. The model's stack is left as before; the caller applies the
    /// effect. The graphics state becomes the conservative merge.
    fn body(&mut self, ctx: Ctx, extra: &[Item], allow_push: bool) -> Option<(String, BodyEffect)> {
        let empty_base = allow_push && self.rng.chance(400);
        for _ in 0..3 {
            let saved = self.model.clone();
            if empty_base {
                self.model.stack.clear();
            }
            let base = self.model.stack.clone();
            self.model.stack.extend(extra.iter().cloned());
            let statements = self.block(ctx.body());
            let after = self.model.stack.clone();
            let gfx_after = self.model.gfx;
            let prefix =
                after.len() >= base.len() && after.iter().zip(&base).all(|(a, b)| a.same_as(b));
            let effect = if !prefix || (saved.gfx.current_point && !gfx_after.current_point) {
                None
            } else if after.len() == base.len() {
                Some(BodyEffect::Balanced)
            } else if empty_base {
                Some(BodyEffect::Pushes(
                    after[base.len()..]
                        .iter()
                        .map(|item| Item {
                            id: None,
                            ..item.clone()
                        })
                        .collect(),
                ))
            } else {
                None
            };
            self.model = saved;
            if let Some(effect) = effect {
                self.model.gfx = Gfx {
                    current_point: self.model.gfx.current_point && gfx_after.current_point,
                    font: self.model.gfx.font && gfx_after.font,
                    ..self.model.gfx
                };
                return Some((format!("{{ {} }}", statements.join(" ")), effect));
            }
        }
        None
    }

    /// The condition of a conditional: the boolean on top, or a literal.
    fn condition(&mut self) -> Option<String> {
        if self.model.peek(0).is_some_and(|i| i.ty == Ty::Bool) && self.rng.chance(700) {
            self.model.pop();
            return Some(String::new());
        }
        let operand = self.literal_of(Ty::Bool)?;
        operand.text
    }

    fn if_statement(&mut self, ctx: Ctx) -> Option<String> {
        // The condition is taken first: the body runs on what is left.
        let cond = self.condition()?;
        let (body, _) = self.body(ctx, &[], false)?;
        Some(format!("{cond} {body} if").trim_start().to_string())
    }

    fn ifelse_statement(&mut self, ctx: Ctx) -> Option<String> {
        let cond = self.condition()?;
        let (yes, effect_yes) = self.body(ctx, &[], true)?;
        let (no, effect_no) = self.body(ctx, &[], true)?;
        let pushes = match (effect_yes, effect_no) {
            (BodyEffect::Balanced, BodyEffect::Balanced) => Vec::new(),
            (BodyEffect::Pushes(a), BodyEffect::Pushes(b))
                if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.ty == y.ty) =>
            {
                a.iter()
                    .zip(&b)
                    .map(|(x, y)| Item {
                        mag: x.mag.max(y.mag),
                        inexact: x.inexact || y.inexact,
                        ..x.clone()
                    })
                    .collect()
            }
            _ => return None,
        };
        for item in pushes {
            self.model.push(item);
        }
        Some(format!("{cond} {yes} {no} ifelse").trim_start().to_string())
    }

    fn repeat_statement(&mut self, ctx: Ctx) -> Option<String> {
        let count = self.rng.range(0, self.bounds().loop_count);
        let (body, effect) = self.body(ctx, &[], true)?;
        if let BodyEffect::Pushes(items) = effect {
            if items.len() as i64 * count > 20 {
                return None;
            }
            for _ in 0..count {
                for item in &items {
                    self.model.push(item.clone());
                }
            }
        }
        Some(format!("{count} {body} repeat"))
    }

    fn for_statement(&mut self, ctx: Ctx) -> Option<String> {
        let max = self.bounds().loop_count;
        let real = self.rng.chance(250);
        let (control, iterations, ctrl_ty) = if real {
            let n = self.rng.range(0, max);
            (
                format!("0 0.5 {}", Self::real_text(n * 50)),
                n + 1,
                Ty::Real,
            )
        } else {
            let start = self.rng.range(-3, 3);
            let step = *self.rng.pick(&[1, 1, 2, -1]);
            let n = self.rng.range(0, max);
            let limit = start + step * (n - 1);
            (format!("{start} {step} {limit}"), n, Ty::Int)
        };
        let ctrl = Item::num(ctrl_ty, 4);
        let (body, effect) = self.body(ctx, std::slice::from_ref(&ctrl), true)?;
        if let BodyEffect::Pushes(items) = effect {
            if items.len() as i64 * iterations > 20 {
                return None;
            }
            for _ in 0..iterations {
                for item in &items {
                    self.model.push(item.clone());
                }
            }
        }
        Some(format!("{control} {body} for"))
    }

    /// A `loop` that counts in a named variable and exits after a few
    /// iterations; the body must be balanced.
    fn loop_statement(&mut self, ctx: Ctx) -> Option<String> {
        let counter = format!("c{}", self.next_counter);
        self.next_counter += 1;
        let limit = self.rng.range(0, self.bounds().loop_count);
        let (body, _) = self.body(ctx, &[], false)?;
        let inner = body
            .strip_prefix("{ ")
            .and_then(|b| b.strip_suffix(" }"))
            .unwrap_or("");
        Some(format!(
            "/{counter} 0 def {{ /{counter} {counter} 1 add def {counter} {limit} gt {{ exit }} if {inner} }} loop"
        ))
    }

    fn stopped_block(&mut self, ctx: Ctx) -> Option<String> {
        let statements = self.block(ctx.nested());
        if statements.is_empty() {
            return None;
        }
        Some(format!("{{ {} }} stopped {HANDLER}", statements.join(" ")))
    }

    /// `/sN save def … sN restore`: the block's definitions and every
    /// composite it created are gone afterwards, so items created inside
    /// are popped first, which is also what keeps `restore` valid.
    fn save_block(&mut self, ctx: Ctx) -> Option<String> {
        let name = format!("s{}", self.next_save);
        self.next_save += 1;
        let snapshot = self.model.clone();
        self.model.epoch += 1;
        let mut statements = vec![format!("/{name} save def")];
        statements.extend(self.block(ctx.nested()));
        let deepest_new = self
            .model
            .stack
            .iter()
            .position(|item| item.epoch > snapshot.epoch);
        if let Some(index) = deepest_new {
            let pops = self.model.stack.len() - index;
            self.model.stack.truncate(index);
            statements.extend(std::iter::repeat_n("pop".to_string(), pops));
        }
        statements.push(format!("{name} restore"));
        let stack = std::mem::take(&mut self.model.stack);
        self.model = snapshot;
        // A sub-array or substring of an old composite is as old as its
        // storage and survives, but its header was tracked inside the
        // block: it stays as an opaque composite.
        let known = self.model.comps.len();
        self.model.stack = stack
            .into_iter()
            .map(|item| Item {
                id: item.id.filter(|&id| id < known),
                ..item
            })
            .collect();
        Some(statements.join(" "))
    }

    /// `<dict> begin … end` over a visible dictionary or a new one.
    fn dict_scope(&mut self, ctx: Ctx) -> Option<String> {
        let dicts: Vec<(String, Item)> = self
            .model
            .visible_names()
            .into_iter()
            .filter_map(|name| {
                let item = self.model.lookup(&name)?.clone();
                (item.ty == Ty::Dict && item.id.is_some()).then_some((name, item))
            })
            .collect();
        let (head, item) = if !dicts.is_empty() && self.rng.chance(600) {
            self.rng.pick(&dicts).clone()
        } else {
            let item = self.model.alloc(Comp::Dict {
                entries: Vec::new(),
            });
            ("3 dict".to_string(), item)
        };
        self.model.begin(item.id?);
        let mut statements = vec![format!("{head} begin")];
        statements.extend(self.block(ctx.nested()));
        statements.push("end".to_string());
        self.model.end();
        Some(statements.join(" "))
    }

    fn gsave_block(&mut self, ctx: Ctx) -> Option<String> {
        let gfx = self.model.gfx;
        let mut statements = vec!["gsave".to_string()];
        statements.extend(self.block(ctx.nested()));
        statements.push("grestore".to_string());
        self.model.gfx = gfx;
        Some(statements.join(" "))
    }
}

/// Types the ill-typed replacement chooses among.
const WRONG_CANDIDATES: [Ty; 8] = [
    Ty::Int,
    Ty::Real,
    Ty::Bool,
    Ty::String,
    Ty::Name,
    Ty::Array,
    Ty::Dict,
    Ty::Null,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{CORE, GRAPHICS};

    #[test]
    fn generation_is_a_function_of_profile_seed_and_index() {
        let a = generate(&CORE, 42, 3);
        let b = generate(&CORE, 42, 3);
        assert_eq!(a, b);
        assert_ne!(a, generate(&CORE, 42, 4));
        assert_ne!(a, generate(&CORE, 43, 3));
        assert_eq!(a.header[0], "%!PS");
        assert_eq!(a.header[3], "% psgen: profile=core seed=42 index=3");
        assert_eq!(a.statements.last().map(String::as_str), Some("pstack"));
    }

    #[test]
    fn graphics_programs_set_up_a_page_and_show_it() {
        let p = generate(&GRAPHICS, 1, 0);
        assert!(p.statements[0].ends_with("setpagedevice"));
        let n = p.statements.len();
        assert_eq!(p.statements[n - 2], "showpage");
        assert_eq!(p.statements[n - 1], "pstack");
    }

    #[test]
    fn ill_typed_statements_are_wrapped_and_handled() {
        let profile = CORE.with_ill_typed(1000);
        let p = generate(&profile, 5, 0);
        let wrapped = p
            .statements
            .iter()
            .filter(|s| s.starts_with("{ ") && s.ends_with(" clear"))
            .count();
        assert!(wrapped > 0, "{p:?}");
        // Every wrapped statement holds one operator application whose
        // operands are literals; the well-typed scenario in the
        // integration tests shows the clean profile raises nothing.
        assert!(
            p.statements
                .iter()
                .filter(|s| s.ends_with(" clear"))
                .all(|s| s.starts_with("{ ") && s.contains(HANDLER)),
        );
    }

    #[test]
    fn reals_are_written_from_hundredths() {
        assert_eq!(Generator::real_text(5), "0.05");
        assert_eq!(Generator::real_text(-150), "-1.50");
        assert_eq!(Generator::real_text(12345), "123.45");
    }

    fn operands(items: &[Item]) -> Vec<Operand> {
        items
            .iter()
            .map(|item| Operand {
                text: None,
                item: item.clone(),
                value: 0,
            })
            .collect()
    }

    fn literal(item: Item, value: i64) -> Operand {
        Operand {
            text: Some(value.to_string()),
            item,
            value,
        }
    }

    #[test]
    fn stack_forms_permute_the_model() {
        let mut model = Model::new();
        let mut rng = Rng::new(1, 1);
        let a = Item::num(Ty::Int, 1);
        let b = Item::scalar(Ty::Bool);
        let c = Item::scalar(Ty::Name);
        let d = Item::num(Ty::Real, 2);
        let three = operands(&[a.clone(), b.clone(), c.clone()]);
        let four = operands(&[a.clone(), b.clone(), c.clone(), d.clone()]);
        let mut result = |f: ResultFn, o: &[Operand]| f(&mut model, o, &mut rng).unwrap();
        assert_eq!(result(roll_3_1, &three), [c.clone(), a.clone(), b.clone()]);
        assert_eq!(
            result(roll_3_back, &three),
            [b.clone(), c.clone(), a.clone()]
        );
        assert_eq!(
            result(roll_4_1, &four),
            [d.clone(), a.clone(), b.clone(), c.clone()]
        );
        assert_eq!(
            result(roll_4_2, &four),
            [c.clone(), d.clone(), a.clone(), b.clone()]
        );
        assert_eq!(
            result(index_1, &three[..2]),
            [a.clone(), b.clone(), a.clone()]
        );
        assert_eq!(
            result(index_2, &three),
            [a.clone(), b.clone(), c.clone(), a.clone()]
        );
        assert_eq!(
            result(copy_2, &three[..2]),
            [a.clone(), b.clone(), a.clone(), b.clone()]
        );
        assert_eq!(result(copy_3, &three).len(), 6);
        assert_eq!(permute(&three, &[3]), None);
    }

    #[test]
    fn intervals_and_astore_track_storage() {
        let mut model = Model::new();
        let mut rng = Rng::new(1, 2);
        model.epoch = 1;
        let source = model.alloc(Comp::Array {
            elems: vec![
                Item::num(Ty::Int, 1),
                Item::scalar(Ty::Bool),
                Item::scalar(Ty::Name),
            ],
            aliased: false,
        });
        model.epoch = 2;
        let o = vec![
            operands(std::slice::from_ref(&source)).remove(0),
            literal(Item::num(Ty::Int, 1), 1),
            literal(Item::num(Ty::Int, 1), 2),
        ];
        let sub = array_getinterval(&mut model, &o, &mut rng)
            .unwrap()
            .remove(0);
        assert_eq!(sub.epoch, 1, "shares storage created before the save");
        assert_eq!(
            model.array_elems(&sub).unwrap(),
            [Item::scalar(Ty::Bool), Item::scalar(Ty::Name)]
        );
        assert!(!model.array_writable(&sub));
        assert!(!model.array_writable(&source));
        let beyond = vec![
            o[0].clone(),
            literal(Item::num(Ty::Int, 1), 2),
            literal(Item::num(Ty::Int, 1), 2),
        ];
        assert_eq!(array_getinterval(&mut model, &beyond, &mut rng), None);

        let target = model.alloc(Comp::Array {
            elems: vec![Item::scalar(Ty::Null); 4],
            aliased: false,
        });
        let patch = model.alloc(Comp::Array {
            elems: vec![Item::num(Ty::Int, 3), Item::scalar(Ty::Bool)],
            aliased: false,
        });
        let o = vec![
            operands(std::slice::from_ref(&target)).remove(0),
            literal(Item::num(Ty::Int, 1), 1),
            operands(&[patch]).remove(0),
        ];
        assert_eq!(array_putinterval(&mut model, &o, &mut rng), Some(vec![]));
        assert_eq!(
            model.array_elems(&target).unwrap(),
            [
                Item::scalar(Ty::Null),
                Item::num(Ty::Int, 3),
                Item::scalar(Ty::Bool),
                Item::scalar(Ty::Null)
            ]
        );

        let text = model.alloc(Comp::Str {
            len: 5,
            aliased: false,
        });
        let o = vec![
            operands(std::slice::from_ref(&text)).remove(0),
            literal(Item::num(Ty::Int, 1), 3),
            literal(Item::num(Ty::Int, 1), 2),
        ];
        let sub = string_getinterval(&mut model, &o, &mut rng)
            .unwrap()
            .remove(0);
        assert_eq!(model.string_len(&sub), Some(2));
        assert!(!model.string_writable(&text));

        let stored = astore(
            &mut model,
            &operands(&[Item::num(Ty::Int, 2), sub]),
            &mut rng,
        )
        .unwrap()
        .remove(0);
        assert_eq!(model.array_elems(&stored).unwrap().len(), 2);
        assert!(model.array_writable(&stored));
        assert_eq!(stored.epoch, 2);
    }

    #[test]
    fn value_rules_of_the_literal_patterns() {
        let mut generator = Generator {
            rng: Rng::new(3, 0),
            profile: &GRAPHICS,
            model: Model::new(),
            next_var: 0,
            next_proc: 0,
            next_save: 0,
            next_counter: 0,
        };
        for _ in 0..200 {
            let dash = generator.literal_for(Pat::DashArray, &[]).unwrap();
            let text = dash.text.unwrap();
            let lengths: Vec<i64> = text
                .trim_matches(|c| c == '[' || c == ']' || c == ' ')
                .split_whitespace()
                .map(|t| t.parse().unwrap())
                .collect();
            assert!(lengths.iter().all(|&l| l >= 0), "{text}");
            assert!(
                lengths.is_empty() || lengths.iter().any(|&l| l > 0),
                "{text}"
            );
            let corner = generator.literal_for(Pat::Corner, &[]).unwrap();
            let text = corner.text.unwrap();
            let fields: Vec<&str> = text.split_whitespace().collect();
            assert_eq!(fields[2], "moveto", "{text}");
            let n = |i: usize| fields[i].parse::<i64>().unwrap();
            assert_ne!(n(0), n(3), "{text}");
            assert_eq!(n(1), n(4), "{text}");
            assert_ne!(n(3), n(5), "{text}");
            assert_ne!(n(4), n(6), "{text}");
            assert!((n(6) - n(4)).abs() >= (n(5) - n(3)).abs(), "{text}");
            assert!(n(7) >= 1, "{text}");
            let matrix = generator.literal_for(Pat::Matrix, &[]).unwrap();
            let text = matrix.text.unwrap();
            let m: Vec<f64> = text
                .trim_matches(|c| c == '[' || c == ']' || c == ' ')
                .split_whitespace()
                .map(|t| t.parse().unwrap())
                .collect();
            assert!(m[0] * m[3] - m[1] * m[2] >= 0.1, "{text}");
            let (lo, hi) = (matrix.value % 1000, matrix.value / 1000);
            assert!(lo >= 25 && lo <= hi && hi <= 250, "{text}: {lo} {hi}");
            let image = generator.literal_for(Pat::ImageBlock, &[]).unwrap();
            let text = image.text.unwrap();
            assert!(
                text.starts_with("gsave ") && text.contains(" 8 [ "),
                "{text}"
            );
            let hex = text.rsplit('<').next().unwrap();
            let hex = hex.split('>').next().unwrap();
            assert!(hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit()));
            let numbers = generator.literal_for(Pat::NumText, &[]).unwrap();
            let text = numbers.text.unwrap();
            assert!(
                text[1..text.len() - 1]
                    .split(' ')
                    .all(|t| t.parse::<u32>().is_ok()),
                "{text}"
            );
        }
        // Interval counts and fitting literals stay inside the operand.
        let array = generator.literal_of(Ty::Array).unwrap();
        let len = generator.model.length_of(&array.item).unwrap();
        if len > 0 {
            let index = generator
                .literal_for(Pat::IndexInto(0), std::slice::from_ref(&array))
                .unwrap();
            let both = vec![array.clone(), index.clone()];
            for _ in 0..50 {
                let count = generator
                    .literal_for(Pat::IntervalCount(0, 1), &both)
                    .unwrap();
                assert!(index.value + count.value <= len as i64);
                let fitting = generator
                    .literal_for(Pat::ArrayFitting(0, 1), &both)
                    .unwrap();
                assert!(index.value + fitting.value <= len as i64);
            }
        }
        let text = generator.literal_of(Ty::String).unwrap();
        let advances = generator
            .literal_for(Pat::Advances(0), std::slice::from_ref(&text))
            .unwrap();
        assert_eq!(advances.value, text.value);
        assert_eq!(
            advances.text.unwrap().split_whitespace().count(),
            text.value as usize + 2
        );
    }

    #[test]
    fn the_scale_band_bounds_scale_and_concat() {
        let mut generator = Generator {
            rng: Rng::new(8, 0),
            profile: &GRAPHICS,
            model: Model::new(),
            next_var: 0,
            next_proc: 0,
            next_save: 0,
            next_counter: 0,
        };
        let scale = GRAPHICS_OPS.iter().find(|o| o.text == "scale").unwrap();
        let concat = GRAPHICS_OPS.iter().find(|o| o.text == "concat").unwrap();
        assert_eq!(scale.restrict, Restrict::Straight);
        assert_eq!(concat.restrict, Restrict::Straight);
        let mut applied = 0;
        for _ in 0..200 {
            if generator.op_statement(scale, false).is_some() {
                applied += 1;
            }
            if generator.op_statement(concat, false).is_some() {
                applied += 1;
            }
            let g = generator.model.gfx;
            assert!(g.lo >= Gfx::BAND.0 && g.hi <= Gfx::BAND.1, "{g:?}");
        }
        assert!(applied > 0);
        // A `gsave` block hands the band back.
        let band = generator.model.gfx;
        generator.gsave_block(Ctx::TOP).unwrap();
        assert_eq!(generator.model.gfx, band);
        // No loop body scales.
        for index in 0..300 {
            let p = generate(&GRAPHICS.with_ill_typed(0), 33, index);
            for s in &p.statements {
                if let Some(body) = s.strip_suffix(" repeat").or(s.strip_suffix(" for")) {
                    // An image block scales inside its own gsave/grestore.
                    let mut body = body.to_string();
                    while let Some(end) = body.find(" image grestore") {
                        let start = body[..end].rfind("gsave").unwrap();
                        body.replace_range(start..end + " image grestore".len(), "");
                    }
                    assert!(
                        !body
                            .split_whitespace()
                            .any(|t| t == "scale" || t == "concat"),
                        "{s}"
                    );
                }
            }
        }
    }

    #[test]
    fn block_patterns_are_never_ill_typed() {
        let mut generator = Generator {
            rng: Rng::new(9, 0),
            profile: &GRAPHICS,
            model: Model::new(),
            next_var: 0,
            next_proc: 0,
            next_save: 0,
            next_counter: 0,
        };
        for text in ["arcto", "image grestore"] {
            let op = GRAPHICS_OPS.iter().find(|o| o.text == text).unwrap();
            assert_eq!(generator.op_statement(op, true), None, "{text}");
        }
        let p = generate(&GRAPHICS.with_ill_typed(1000), 9, 0);
        for s in &p.statements {
            if s.ends_with(" clear") {
                assert!(!s.contains("arcto") && !s.contains("image"), "{s}");
            }
        }
    }

    #[test]
    fn sub_arrays_of_old_storage_outlive_a_save_block() {
        // Once panicked in the model: the sub-array kept its source's
        // save level with a composite id the block's snapshot lacked.
        let p = generate(&CORE, 113, 252);
        assert!(p.statements.len() > 1);
        let mut generator = Generator {
            rng: Rng::new(2, 0),
            profile: &CORE,
            model: Model::new(),
            next_var: 0,
            next_proc: 0,
            next_save: 0,
            next_counter: 0,
        };
        let array = generator.model.alloc(Comp::Array {
            elems: vec![Item::num(Ty::Int, 1); 3],
            aliased: false,
        });
        generator.model.push(array);
        let snapshot = generator.model.clone();
        generator.model.epoch += 1;
        let op = CORE_OPS
            .iter()
            .find(|o| o.text == "getinterval" && o.inputs[0] == Pat::ArrayTracked)
            .unwrap();
        assert!(generator.op_statement(op, false).is_some());
        let sub = generator.model.peek(0).unwrap().clone();
        assert_eq!(sub.epoch, 0);
        assert!(sub.id.unwrap() >= snapshot.comps.len());
        let stack = std::mem::take(&mut generator.model.stack);
        generator.model = snapshot;
        let known = generator.model.comps.len();
        generator.model.stack = stack
            .into_iter()
            .map(|item| Item {
                id: item.id.filter(|&id| id < known),
                ..item
            })
            .collect();
        let survivor = generator.model.peek(0).unwrap();
        assert_eq!(survivor.ty, Ty::Array);
        assert_eq!(survivor.id, None);
        for s in generator.model.stack.clone() {
            assert!(s.id.is_none_or(|id| id < generator.model.comps.len()));
        }
    }

    #[test]
    fn userdict_is_never_counted() {
        let mut model = Model::new();
        let mut rng = Rng::new(1, 0);
        let userdict = current_dict(&mut model, &[], &mut rng).unwrap().remove(0);
        let other = model.alloc(Comp::Dict {
            entries: vec![("alpha".to_string(), Item::scalar(Ty::Bool))],
        });
        let of = |item: &Item| {
            vec![Operand {
                text: None,
                item: item.clone(),
                value: 0,
            }]
        };
        assert_eq!(length(&mut model, &of(&userdict), &mut rng), None);
        assert_eq!(dict_count(&mut model, &of(&userdict), &mut rng), None);
        assert_eq!(length(&mut model, &of(&other), &mut rng).unwrap().len(), 1);
        assert_eq!(
            dict_count(&mut model, &of(&other), &mut rng).unwrap().len(),
            1
        );
        for index in 0..300 {
            let p = generate(&CORE.with_ill_typed(0), 50, index);
            let text = p.statements.join("\n");
            assert!(!text.contains("currentdict length"), "{text}");
            assert!(
                !text.contains("currentdict 0 exch { pop pop 1 add } forall"),
                "{text}"
            );
        }
    }

    #[test]
    fn inexact_readings_avoid_discrete_operators() {
        let mut generator = Generator {
            rng: Rng::new(5, 0),
            profile: &GRAPHICS,
            model: Model::new(),
            next_var: 0,
            next_proc: 0,
            next_save: 0,
            next_counter: 0,
        };
        generator.model.push(Item::inexact_real(4));
        let find = |text: &str| CORE_OPS.iter().find(|o| o.text == text).unwrap();
        for text in [
            "cvi",
            "round",
            "eq",
            "lt",
            "32 string cvs",
            "atan",
            "mul",
            "div",
            "abs sqrt",
            "abs 2 exp",
        ] {
            let op = find(text);
            assert!(op.exact_inputs, "{text}");
            for _ in 0..20 {
                assert_eq!(generator.operands_from_stack(op), 0, "{text}");
            }
        }
        for text in ["add", "sub", "neg", "abs", "sin", "cvr", "abs 1 add ln"] {
            let op = find(text);
            assert!(!op.exact_inputs, "{text}");
            let mut taken = false;
            for _ in 0..20 {
                taken |= generator.operands_from_stack(op) == 1;
            }
            assert!(taken, "{text}");
            let mut model = Model::new();
            let mut rng = Rng::new(1, 0);
            let operands = [
                Operand {
                    text: None,
                    item: Item::inexact_real(4),
                    value: 0,
                },
                Operand {
                    text: Some("2".to_string()),
                    item: Item::num(Ty::Int, 1),
                    value: 2,
                },
            ];
            let results = (op.result)(&mut model, &operands[..op.inputs.len()], &mut rng).unwrap();
            assert!(results.iter().all(|item| item.inexact), "{text}");
        }
        // A procedure never takes an inexact argument, since its body
        // was generated without knowing.
        let proc_item = generator.model.alloc(Comp::Proc {
            inputs: vec![Item::num(Ty::Real, 4)],
            outputs: Vec::new(),
        });
        assert!(!generator.call_matches(&proc_item));
        generator.model.stack.clear();
        generator.model.push(Item::num(Ty::Real, 4));
        assert!(generator.call_matches(&proc_item));
        // Whole programs: no statement hands a reading to a discrete
        // operator directly.
        for index in 0..300 {
            let p = generate(&GRAPHICS.with_ill_typed(0), 21, index);
            for s in &p.statements {
                for reading in ["currentpoint", "pathbbox", "arcto"] {
                    if let Some(rest) = s.split_once(reading).map(|(_, r)| r) {
                        let next = rest.split_whitespace().next().unwrap_or("");
                        assert!(
                            !matches!(
                                next,
                                "cvi" | "round" | "eq" | "lt" | "gt" | "cvs" | "mul" | "div"
                            ),
                            "{s}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_widened_grammar_emits_every_addition() {
        let expected = [
            "3 1 roll",
            "3 -1 roll",
            "4 1 roll",
            "4 2 roll",
            "1 index",
            "2 index",
            "2 copy",
            "3 copy",
            " getinterval",
            " putinterval",
            "array astore",
            "{ pop pop } forall",
            "{ pop pop 1 add } forall",
            "abs 1 add ln",
            "abs 1 add log",
            "abs 0.5 exp",
            "abs 2 exp",
            "string cvrs",
            "token pop exch pop",
            "token { pop pop } if",
        ];
        let graphics_only = [
            " setdash",
            " concat",
            " arcto",
            " xshow",
            " kshow",
            " image grestore",
            "currentpoint",
            "pathbbox",
        ];
        for (profile, extra) in [(&CORE, &[][..]), (&GRAPHICS, &graphics_only[..])] {
            let text: String = (0..300)
                .map(|index| generate(&profile.with_ill_typed(0), 99, index).render())
                .collect();
            for needle in expected.iter().chain(extra) {
                assert!(
                    text.contains(needle),
                    "{}: `{needle}` never emitted",
                    profile.name
                );
            }
            if profile.kind == Kind::Core {
                for needle in &graphics_only {
                    assert!(!text.contains(needle), "core emitted `{needle}`");
                }
            }
        }
    }
}
