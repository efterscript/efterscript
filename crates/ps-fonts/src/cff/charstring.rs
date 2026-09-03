// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The Type 2 charstring interpreter: the width is taken from the first
//! stack-clearing operator by the argument-count rule, hints and hint
//! masks are counted and skipped, subroutines run through an explicit
//! call stack with the bias rule, the four flex forms become two curves,
//! the accent form of `endchar` composes two glyphs through the standard
//! encoding, and the arithmetic escapes are executed. Every local and
//! global subroutine index executed is recorded, so a subset can tell
//! which subroutines its charstrings reach.

use std::collections::BTreeSet;

use super::{CffProgram, PrivateDict, Reached, Site};
use crate::encoding::STANDARD_ENCODING;
use crate::outline::{Glyph, Outline};
use crate::program::FontError;

/// The operand stack the format allows.
const MAX_STACK: usize = 48;
/// Subroutine nesting the format allows.
const MAX_CALL_DEPTH: usize = 10;
/// The transient array the `put`/`get` escapes address.
const TRANSIENT: usize = 32;

pub(crate) struct Interpreted {
    pub glyph: Glyph,
    pub components: Vec<u16>,
    pub reached: Reached,
}

/// The subroutine bias for an index of `count` entries.
pub fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// One number of the Type 2 encoding, or `None` for an operator byte.
pub(crate) fn number(code: &[u8], pc: &mut usize) -> Result<Option<f32>, FontError> {
    let v = code[*pc];
    *pc += 1;
    let value = match v {
        32..=246 => f32::from(i16::from(v) - 139),
        247..=250 => {
            let w = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
            *pc += 1;
            f32::from((i16::from(v) - 247) * 256 + i16::from(w) + 108)
        }
        251..=254 => {
            let w = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
            *pc += 1;
            f32::from(-(i16::from(v) - 251) * 256 - i16::from(w) - 108)
        }
        28 => {
            let b = code
                .get(*pc..*pc + 2)
                .ok_or(FontError::Truncated("charstring"))?;
            *pc += 2;
            f32::from(i16::from_be_bytes([b[0], b[1]]))
        }
        255 => {
            let b = code
                .get(*pc..*pc + 4)
                .ok_or(FontError::Truncated("charstring"))?;
            *pc += 4;
            i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32 / 65536.0
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

/// Appends an integer in the Type 2 encoding, which holds sixteen-bit
/// values only.
pub fn encode_number(v: i32, out: &mut Vec<u8>) {
    assert!(
        i16::try_from(v).is_ok(),
        "{v} is outside the Type 2 integer range"
    );
    match v {
        -107..=107 => out.push((v + 139) as u8),
        108..=1131 => {
            let w = v - 108;
            out.push((w / 256 + 247) as u8);
            out.push((w % 256) as u8);
        }
        -1131..=-108 => {
            let w = -v - 108;
            out.push((w / 256 + 251) as u8);
            out.push((w % 256) as u8);
        }
        _ => {
            out.push(28);
            out.extend_from_slice(&(v as i16).to_be_bytes());
        }
    }
}

/// Appends a number in the 16.16 fixed-point form.
pub fn encode_fixed(v: f32, out: &mut Vec<u8>) {
    out.push(255);
    out.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
}

/// One unit of a charstring: a number, a one-byte operator, a two-byte
/// operator, or a hint mask's bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Token<'a> {
    Num(f32),
    Op(u8),
    Esc(u8),
    /// The bytes following `hintmask` or `cntrmask`.
    Mask(&'a [u8]),
}

/// Interprets the charstring at `gid`.
pub(crate) fn interpret(program: &CffProgram, gid: u16) -> Result<Interpreted, FontError> {
    let code = program.charstring(gid)?;
    let private = program.private_for(gid)?;
    let mut machine = Machine::new(program, private);
    machine.run(code, Site::Glyph(gid))?;
    if let Some(seac) = machine.seac.take() {
        return machine.compose(seac);
    }
    let reached = std::mem::take(&mut machine.reached);
    Ok(Interpreted {
        glyph: machine.into_glyph(),
        components: Vec::new(),
        reached,
    })
}

struct Seac {
    adx: f32,
    ady: f32,
    base: u16,
    accent: u16,
}

struct Machine<'a> {
    program: &'a CffProgram,
    private: &'a PrivateDict,
    stack: Vec<f32>,
    transient: [f32; TRANSIENT],
    outline: Outline,
    x: f32,
    y: f32,
    /// Whether the current subpath has segments to close.
    open: bool,
    width: Option<f32>,
    stems: usize,
    seac: Option<Seac>,
    reached: Reached,
}

enum Step<'a> {
    Continue,
    Call(&'a [u8], Site),
    Return,
    End,
}

impl<'a> Machine<'a> {
    fn new(program: &'a CffProgram, private: &'a PrivateDict) -> Self {
        Machine {
            program,
            private,
            stack: Vec::new(),
            transient: [0.0; TRANSIENT],
            outline: Outline::new(),
            x: 0.0,
            y: 0.0,
            open: false,
            width: None,
            stems: 0,
            seac: None,
            reached: Reached::default(),
        }
    }

    fn into_glyph(self) -> Glyph {
        Glyph {
            advance: (self.width.unwrap_or(self.private.default_width_x), 0.0),
            outline: self.outline,
        }
    }

    /// Runs `code` (the charstring at `site`) and the subroutines it
    /// calls until `endchar` or the end of the top-level charstring.
    fn run(&mut self, code: &'a [u8], site: Site) -> Result<(), FontError> {
        let mut frames: Vec<(&[u8], usize, Site)> = vec![(code, 0, site)];
        loop {
            let Some(&(code, mut pc, site)) = frames.last() else {
                return Ok(());
            };
            if pc >= code.len() {
                frames.pop();
                if frames.is_empty() {
                    // The format requires `endchar`; running off the end
                    // of the glyph's own charstring is the malformed case.
                    return Err(FontError::Truncated("charstring"));
                }
                continue;
            }
            let step = self.step(code, &mut pc, frames.len(), site)?;
            frames.last_mut().expect("frame present").1 = pc;
            match step {
                Step::Continue => {}
                Step::Call(subr, site) => frames.push((subr, 0, site)),
                Step::Return => {
                    frames.pop();
                }
                Step::End => return Ok(()),
            }
        }
    }

    fn push(&mut self, value: f32) -> Result<(), FontError> {
        if self.stack.len() >= MAX_STACK {
            return Err(FontError::Malformed("charstring stack"));
        }
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self, op: &'static str) -> Result<f32, FontError> {
        self.stack.pop().ok_or(FontError::Operands(op))
    }

    fn step(
        &mut self,
        code: &'a [u8],
        pc: &mut usize,
        depth: usize,
        site: Site,
    ) -> Result<Step<'a>, FontError> {
        if let Some(value) = number(code, pc)? {
            self.push(value)?;
            return Ok(Step::Continue);
        }
        let op = code[*pc - 1];
        match op {
            12 => {
                let e = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
                *pc += 1;
                self.escaped(e)
            }
            10 => {
                let index = self.pop("callsubr")?;
                self.call(index, &self.private.subrs, depth, true)
            }
            29 => {
                let index = self.pop("callgsubr")?;
                self.call(index, self.program.global_subrs(), depth, false)
            }
            11 => Ok(Step::Return),
            19 | 20 => {
                // Arguments before the first mask are an implicit vstem.
                self.take_width(!self.stack.len().is_multiple_of(2));
                self.stems += self.stack.len() / 2;
                self.stack.clear();
                let bytes = self.stems.div_ceil(8);
                if code.len() < *pc + bytes {
                    return Err(FontError::Truncated("hint mask"));
                }
                self.reached.masks.insert((site, *pc - 1, bytes));
                *pc += bytes;
                Ok(Step::Continue)
            }
            _ => self.plain(op),
        }
    }

    fn call(
        &mut self,
        index: f32,
        subrs: &'a [Vec<u8>],
        depth: usize,
        local: bool,
    ) -> Result<Step<'a>, FontError> {
        let index = index as i32 + bias(subrs.len());
        let k = usize::try_from(index)
            .ok()
            .filter(|k| *k < subrs.len())
            .ok_or(FontError::SubrIndex(index))?;
        if depth > MAX_CALL_DEPTH {
            return Err(FontError::CallDepth);
        }
        let site = if local {
            self.reached.local.insert(k);
            Site::Local(k)
        } else {
            self.reached.global.insert(k);
            Site::Global(k)
        };
        Ok(Step::Call(subrs[k].as_slice(), site))
    }

    /// The width rule: the first stack-clearing operator may carry one
    /// leading argument beyond its own, the width relative to the
    /// nominal width; `has_width` says whether the argument count shows
    /// one. Later operators never take a width.
    fn take_width(&mut self, has_width: bool) {
        if self.width.is_some() {
            return;
        }
        self.width = Some(if has_width && !self.stack.is_empty() {
            self.private.nominal_width_x + self.stack.remove(0)
        } else {
            self.private.default_width_x
        });
    }

    fn close_subpath(&mut self) {
        if self.open {
            self.outline.close();
            self.open = false;
        }
    }

    fn move_to(&mut self, dx: f32, dy: f32) {
        self.close_subpath();
        self.x += dx;
        self.y += dy;
        self.outline.move_to(self.x, self.y);
    }

    fn line_to(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
        self.outline.line_to(self.x, self.y);
        self.open = true;
    }

    fn curve_to(&mut self, d: [f32; 6]) {
        let x1 = self.x + d[0];
        let y1 = self.y + d[1];
        let x2 = x1 + d[2];
        let y2 = y1 + d[3];
        self.x = x2 + d[4];
        self.y = y2 + d[5];
        self.outline.curve_to(x1, y1, x2, y2, self.x, self.y);
        self.open = true;
    }

    /// Alternating horizontal and vertical lines.
    fn alternating_lines(&mut self, horizontal_first: bool) {
        let mut horizontal = horizontal_first;
        for k in 0..self.stack.len() {
            let d = self.stack[k];
            if horizontal {
                self.line_to(d, 0.0);
            } else {
                self.line_to(0.0, d);
            }
            horizontal = !horizontal;
        }
    }

    /// Curves alternating between a vertical and a horizontal start; a
    /// fifth argument on the last curve sets the other end coordinate.
    fn alternating_curves(&mut self, vertical_first: bool) -> Result<(), FontError> {
        let args = std::mem::take(&mut self.stack);
        if args.len() < 4 || !matches!(args.len() % 4, 0 | 1) {
            return Err(FontError::Operands("vhcurveto"));
        }
        let mut vertical = vertical_first;
        let mut k = 0;
        while k + 4 <= args.len() {
            let last = if args.len() - k == 5 {
                args[k + 4]
            } else {
                0.0
            };
            let a = &args[k..k + 4];
            if vertical {
                self.curve_to([0.0, a[0], a[1], a[2], a[3], last]);
            } else {
                self.curve_to([a[0], 0.0, a[1], a[2], last, a[3]]);
            }
            vertical = !vertical;
            k += 4;
        }
        Ok(())
    }

    fn plain(&mut self, op: u8) -> Result<Step<'a>, FontError> {
        match op {
            // hstem, vstem, hstemhm, vstemhm
            1 | 3 | 18 | 23 => {
                self.take_width(!self.stack.len().is_multiple_of(2));
                self.stems += self.stack.len() / 2;
                self.stack.clear();
            }
            4 => {
                self.take_width(self.stack.len() > 1);
                let dy = self.pop("vmoveto")?;
                self.move_to(0.0, dy);
                self.stack.clear();
            }
            5 => {
                if self.stack.is_empty() || !self.stack.len().is_multiple_of(2) {
                    return Err(FontError::Operands("rlineto"));
                }
                for k in (0..self.stack.len()).step_by(2) {
                    let (dx, dy) = (self.stack[k], self.stack[k + 1]);
                    self.line_to(dx, dy);
                }
                self.stack.clear();
            }
            6 | 7 => {
                if self.stack.is_empty() {
                    return Err(FontError::Operands("hlineto"));
                }
                self.alternating_lines(op == 6);
                self.stack.clear();
            }
            8 => {
                if self.stack.is_empty() || !self.stack.len().is_multiple_of(6) {
                    return Err(FontError::Operands("rrcurveto"));
                }
                for k in (0..self.stack.len()).step_by(6) {
                    let a = [
                        self.stack[k],
                        self.stack[k + 1],
                        self.stack[k + 2],
                        self.stack[k + 3],
                        self.stack[k + 4],
                        self.stack[k + 5],
                    ];
                    self.curve_to(a);
                }
                self.stack.clear();
            }
            14 => {
                let n = self.stack.len();
                self.take_width(n == 1 || n == 5);
                if self.stack.len() == 4 {
                    let a = std::mem::take(&mut self.stack);
                    let gid = |code: f32| {
                        u8::try_from(code as i32)
                            .ok()
                            .and_then(|c| STANDARD_ENCODING[usize::from(c)])
                            .and_then(|name| self.program.gid(name.as_bytes()))
                            .ok_or_else(|| {
                                FontError::MissingComponent(format!("{code}").into_bytes())
                            })
                    };
                    self.seac = Some(Seac {
                        adx: a[0],
                        ady: a[1],
                        base: gid(a[2])?,
                        accent: gid(a[3])?,
                    });
                }
                self.stack.clear();
                self.close_subpath();
                return Ok(Step::End);
            }
            21 => {
                self.take_width(self.stack.len() > 2);
                let dy = self.pop("rmoveto")?;
                let dx = self.pop("rmoveto")?;
                self.move_to(dx, dy);
                self.stack.clear();
            }
            22 => {
                self.take_width(self.stack.len() > 1);
                let dx = self.pop("hmoveto")?;
                self.move_to(dx, 0.0);
                self.stack.clear();
            }
            24 => {
                let n = self.stack.len();
                if n < 8 || !(n - 2).is_multiple_of(6) {
                    return Err(FontError::Operands("rcurveline"));
                }
                let args = std::mem::take(&mut self.stack);
                for k in (0..n - 2).step_by(6) {
                    self.curve_to([
                        args[k],
                        args[k + 1],
                        args[k + 2],
                        args[k + 3],
                        args[k + 4],
                        args[k + 5],
                    ]);
                }
                self.line_to(args[n - 2], args[n - 1]);
            }
            25 => {
                let n = self.stack.len();
                if n < 8 || !(n - 6).is_multiple_of(2) {
                    return Err(FontError::Operands("rlinecurve"));
                }
                let args = std::mem::take(&mut self.stack);
                for k in (0..n - 6).step_by(2) {
                    self.line_to(args[k], args[k + 1]);
                }
                let a = &args[n - 6..];
                self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
            }
            26 | 27 => {
                let args = std::mem::take(&mut self.stack);
                if args.len() < 4 || !matches!(args.len() % 4, 0 | 1) {
                    return Err(FontError::Operands("vvcurveto"));
                }
                let mut k = 0;
                let mut d1 = 0.0;
                if args.len() % 4 == 1 {
                    d1 = args[0];
                    k = 1;
                }
                while k + 4 <= args.len() {
                    let a = &args[k..k + 4];
                    if op == 26 {
                        self.curve_to([d1, a[0], a[1], a[2], 0.0, a[3]]);
                    } else {
                        self.curve_to([a[0], d1, a[1], a[2], a[3], 0.0]);
                    }
                    d1 = 0.0;
                    k += 4;
                }
            }
            30 => self.alternating_curves(true)?,
            31 => self.alternating_curves(false)?,
            _ => return Err(FontError::UnknownOperator(op, None)),
        }
        Ok(Step::Continue)
    }

    fn escaped(&mut self, e: u8) -> Result<Step<'a>, FontError> {
        match e {
            // dotsection: obsolete, accepted.
            0 => self.stack.clear(),
            3 => self.binary("and", |a, b| bool_num(a != 0.0 && b != 0.0))?,
            4 => self.binary("or", |a, b| bool_num(a != 0.0 || b != 0.0))?,
            5 => {
                let a = self.pop("not")?;
                self.push(bool_num(a == 0.0))?;
            }
            9 => {
                let a = self.pop("abs")?;
                self.push(a.abs())?;
            }
            10 => self.binary("add", |a, b| a + b)?,
            11 => self.binary("sub", |a, b| a - b)?,
            12 => {
                let b = self.pop("div")?;
                let a = self.pop("div")?;
                if b == 0.0 {
                    return Err(FontError::Malformed("division by zero"));
                }
                self.push(a / b)?;
            }
            14 => {
                let a = self.pop("neg")?;
                self.push(-a)?;
            }
            15 => self.binary("eq", |a, b| bool_num(a == b))?,
            18 => {
                self.pop("drop")?;
            }
            20 => {
                let i = self.pop("put")?;
                let v = self.pop("put")?;
                let slot = self.transient_slot(i)?;
                self.transient[slot] = v;
            }
            21 => {
                let i = self.pop("get")?;
                let slot = self.transient_slot(i)?;
                self.push(self.transient[slot])?;
            }
            22 => {
                let v2 = self.pop("ifelse")?;
                let v1 = self.pop("ifelse")?;
                let s2 = self.pop("ifelse")?;
                let s1 = self.pop("ifelse")?;
                self.push(if v1 <= v2 { s1 } else { s2 })?;
            }
            // random: a value the outline must not depend on; zero.
            23 => self.push(0.0)?,
            24 => self.binary("mul", |a, b| a * b)?,
            26 => {
                let a = self.pop("sqrt")?;
                if a < 0.0 {
                    return Err(FontError::Malformed("sqrt of a negative"));
                }
                self.push(a.sqrt())?;
            }
            27 => {
                let a = self.pop("dup")?;
                self.push(a)?;
                self.push(a)?;
            }
            28 => {
                let b = self.pop("exch")?;
                let a = self.pop("exch")?;
                self.push(b)?;
                self.push(a)?;
            }
            29 => {
                let i = self.pop("index")?;
                let i = if i < 0.0 { 0 } else { i as usize };
                let n = self.stack.len();
                let v = *self
                    .stack
                    .get(n.checked_sub(i + 1).ok_or(FontError::Operands("index"))?)
                    .ok_or(FontError::Operands("index"))?;
                self.push(v)?;
            }
            30 => {
                let j = self.pop("roll")? as i32;
                let n = self.pop("roll")?;
                let n = usize::try_from(n as i32).map_err(|_| FontError::Operands("roll"))?;
                if n == 0 || n > self.stack.len() {
                    return Err(FontError::Operands("roll"));
                }
                let start = self.stack.len() - n;
                let shift = j.rem_euclid(n as i32) as usize;
                self.stack[start..].rotate_right(shift);
            }
            34 => {
                let a = self.args(7, "hflex")?;
                let y0 = self.y;
                self.curve_to([a[0], 0.0, a[1], a[2], a[3], 0.0]);
                self.curve_to([a[4], 0.0, a[5], y0 - self.y, a[6], 0.0]);
            }
            35 => {
                let a = self.args(13, "flex")?;
                self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
                self.curve_to([a[6], a[7], a[8], a[9], a[10], a[11]]);
            }
            36 => {
                let a = self.args(9, "hflex1")?;
                let y0 = self.y;
                self.curve_to([a[0], a[1], a[2], a[3], a[4], 0.0]);
                // The last point returns to the starting y.
                let dy6 = y0 - (self.y + a[7]);
                self.curve_to([a[5], 0.0, a[6], a[7], a[8], dy6]);
            }
            37 => {
                let a = self.args(11, "flex1")?;
                let (x0, y0) = (self.x, self.y);
                let dx: f32 = a[0] + a[2] + a[4] + a[6] + a[8];
                let dy: f32 = a[1] + a[3] + a[5] + a[7] + a[9];
                self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
                // The larger overall delta keeps the last argument; the
                // other coordinate returns to its start.
                let x5 = self.x + a[6] + a[8];
                let y5 = self.y + a[7] + a[9];
                let (dx6, dy6) = if dx.abs() > dy.abs() {
                    (a[10], y0 - y5)
                } else {
                    (x0 - x5, a[10])
                };
                self.curve_to([a[6], a[7], a[8], a[9], dx6, dy6]);
            }
            _ => return Err(FontError::UnknownOperator(12, Some(e))),
        }
        Ok(Step::Continue)
    }

    fn binary(
        &mut self,
        op: &'static str,
        f: impl FnOnce(f32, f32) -> f32,
    ) -> Result<(), FontError> {
        let b = self.pop(op)?;
        let a = self.pop(op)?;
        self.push(f(a, b))
    }

    fn transient_slot(&self, i: f32) -> Result<usize, FontError> {
        usize::try_from(i as i32)
            .ok()
            .filter(|k| *k < TRANSIENT)
            .ok_or(FontError::Malformed("transient index"))
    }

    /// The last `n` operands of a flex operator; the stack is cleared.
    fn args(&mut self, n: usize, op: &'static str) -> Result<Vec<f32>, FontError> {
        if self.stack.len() < n {
            return Err(FontError::Operands(op));
        }
        let args = self.stack[self.stack.len() - n..].to_vec();
        self.stack.clear();
        Ok(args)
    }

    /// The composite glyph: the base as it is, the accent displaced by
    /// `(adx, ady)`; the advance is the composite's own.
    fn compose(mut self, seac: Seac) -> Result<Interpreted, FontError> {
        let program = self.program;
        let component = |gid: u16| -> Result<(Glyph, Reached), FontError> {
            let mut machine = Machine::new(program, program.private_for(gid)?);
            machine.run(program.charstring(gid)?, Site::Glyph(gid))?;
            if machine.seac.is_some() {
                return Err(FontError::Malformed("nested accent"));
            }
            let reached = std::mem::take(&mut machine.reached);
            Ok((machine.into_glyph(), reached))
        };
        let (base, base_reached) = component(seac.base)?;
        let (accent, accent_reached) = component(seac.accent)?;
        self.reached.local.extend(base_reached.local);
        self.reached.local.extend(accent_reached.local);
        self.reached.global.extend(base_reached.global);
        self.reached.global.extend(accent_reached.global);
        self.reached.masks.extend(base_reached.masks);
        self.reached.masks.extend(accent_reached.masks);
        let mut outline = base.outline;
        outline
            .ops
            .extend(accent.outline.translated(seac.adx, seac.ady).ops);
        let advance = (self.width.unwrap_or(self.private.default_width_x), 0.0);
        Ok(Interpreted {
            glyph: Glyph { advance, outline },
            components: vec![seac.base, seac.accent],
            reached: self.reached,
        })
    }
}

fn bool_num(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

/// The tokens of `code`, in order; a hint mask's bytes are counted from
/// the stems declared so far in this charstring.
pub fn tokens(code: &[u8]) -> Result<Vec<Token<'_>>, FontError> {
    tokens_with_masks(code, |_| None)
}

/// The tokens of `code`, a hint mask's byte count taken from
/// `mask_len(offset of the mask operator)` when it answers — what the
/// trace recorded when the charstring ran — and from the stems declared
/// so far in this charstring otherwise.
pub fn tokens_with_masks(
    code: &[u8],
    mask_len: impl Fn(usize) -> Option<usize>,
) -> Result<Vec<Token<'_>>, FontError> {
    let mut out = Vec::new();
    let mut pc = 0;
    let mut stems = 0usize;
    let mut args = 0usize;
    while pc < code.len() {
        if let Some(v) = number(code, &mut pc)? {
            out.push(Token::Num(v));
            args += 1;
            continue;
        }
        let op = code[pc - 1];
        match op {
            12 => {
                let e = *code.get(pc).ok_or(FontError::Truncated("charstring"))?;
                pc += 1;
                out.push(Token::Esc(e));
            }
            1 | 3 | 18 | 23 => {
                stems += args / 2;
                out.push(Token::Op(op));
            }
            19 | 20 => {
                stems += args / 2;
                out.push(Token::Op(op));
                let n = mask_len(pc - 1).unwrap_or_else(|| stems.div_ceil(8));
                let mask = code
                    .get(pc..pc + n)
                    .ok_or(FontError::Truncated("hint mask"))?;
                pc += n;
                out.push(Token::Mask(mask));
            }
            _ => out.push(Token::Op(op)),
        }
        args = 0;
    }
    Ok(out)
}

/// `tokens` back in the charstring encoding; numbers are re-encoded in
/// their shortest form, fixed-point ones keeping their fraction.
pub fn encode(tokens: &[Token<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for token in tokens {
        match *token {
            Token::Num(v) if v.fract() == 0.0 && v.abs() < 32768.0 => {
                encode_number(v as i32, &mut out);
            }
            Token::Num(v) => encode_fixed(v, &mut out),
            Token::Op(op) => out.push(op),
            Token::Esc(e) => out.extend_from_slice(&[12, e]),
            Token::Mask(bytes) => out.extend_from_slice(bytes),
        }
    }
    out
}

/// The set the trace collects, as a helper for tests and the writer.
pub fn set(list: &[usize]) -> BTreeSet<usize> {
    list.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::OutlineOp::{Close, CurveTo, LineTo, MoveTo};
    use crate::testing::{CffFont, Type2Builder};

    fn font() -> CffFont {
        CffFont::new("T").widths(0, 500)
    }

    fn glyph(font: &CffFont, name: &str) -> Glyph {
        let program = font.program().unwrap();
        (*program.glyph(name.as_bytes()).unwrap().unwrap()).clone()
    }

    fn error(font: &CffFont, name: &str) -> FontError {
        font.program().unwrap().glyph(name.as_bytes()).unwrap_err()
    }

    #[test]
    fn numbers_decode_in_every_encoding() {
        let mut bytes = Vec::new();
        for v in [
            0, 107, -107, 108, 1131, -108, -1131, 5000, -5000, 32767, -32768,
        ] {
            encode_number(v, &mut bytes);
        }
        encode_fixed(1.5, &mut bytes);
        let mut pc = 0;
        let mut values = Vec::new();
        while pc < bytes.len() {
            values.push(number(&bytes, &mut pc).unwrap().unwrap());
        }
        assert_eq!(
            values,
            [
                0.0, 107.0, -107.0, 108.0, 1131.0, -108.0, -1131.0, 5000.0, -5000.0, 32767.0,
                -32768.0, 1.5
            ]
        );
        for truncated in [&[255u8, 0][..], &[28], &[247]] {
            assert_eq!(
                number(truncated, &mut 0),
                Err(FontError::Truncated("charstring"))
            );
        }
        assert_eq!(bias(0), 107);
        assert_eq!(bias(1239), 107);
        assert_eq!(bias(1240), 1131);
        assert_eq!(bias(33899), 1131);
        assert_eq!(bias(33900), 32768);
    }

    #[test]
    fn tokens_round_trip_and_read_masks_by_stem_count() {
        let code = Type2Builder::new()
            .num(100)
            .hstem(0, 10)
            .num(0)
            .num(10)
            .num(20)
            .num(10)
            .hintmask(&[0b1110_0000])
            .rmoveto(1, 2)
            .fixed(1.5)
            .num(-2000)
            .callsubr(-107)
            .callgsubr(0)
            .hflex(1, 2, 3, 4, 5, 6, 7)
            .endchar()
            .bytes();
        let list = tokens(&code).unwrap();
        assert_eq!(list[0], Token::Num(100.0));
        assert_eq!(list[3], Token::Op(1));
        assert_eq!(list[8], Token::Op(19));
        assert_eq!(list[9], Token::Mask(&[0b1110_0000]));
        assert_eq!(list[13], Token::Num(1.5));
        assert_eq!(list[list.len() - 1], Token::Op(14));
        assert_eq!(list[list.len() - 2], Token::Esc(34));
        assert_eq!(encode(&list), code);
        assert_eq!(tokens(&[19]), Ok(vec![Token::Op(19), Token::Mask(&[])]));
        assert_eq!(
            tokens(&[139, 139, 1, 19]),
            Err(FontError::Truncated("hint mask"))
        );
        assert_eq!(tokens(&[12]), Err(FontError::Truncated("charstring")));
        assert_eq!(set(&[3, 1]), [1usize, 3].into_iter().collect());
    }

    #[test]
    fn widths_follow_the_argument_count_of_the_first_operator() {
        // Each glyph starts with a different stack-clearing operator, with
        // and without the leading width.
        let f = font()
            .charstring(
                "stem_w",
                Type2Builder::new().num(100).hstem(0, 10).endchar().bytes(),
            )
            .charstring("stem", Type2Builder::new().hstem(0, 10).endchar().bytes())
            .charstring(
                "mask_w",
                Type2Builder::new()
                    .num(50)
                    .num(0)
                    .num(10)
                    .hintmask(&[0x80])
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "mask",
                Type2Builder::new()
                    .num(0)
                    .num(10)
                    .hintmask(&[0x80])
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "rmove_w",
                Type2Builder::new()
                    .num(-100)
                    .rmoveto(1, 1)
                    .endchar()
                    .bytes(),
            )
            .charstring("rmove", Type2Builder::new().rmoveto(1, 1).endchar().bytes())
            .charstring(
                "hmove_w",
                Type2Builder::new().num(20).hmoveto(1).endchar().bytes(),
            )
            .charstring("hmove", Type2Builder::new().hmoveto(1).endchar().bytes())
            .charstring(
                "vmove_w",
                Type2Builder::new().num(30).vmoveto(1).endchar().bytes(),
            )
            .charstring("vmove", Type2Builder::new().vmoveto(1).endchar().bytes())
            .charstring("end_w", Type2Builder::new().num(40).endchar().bytes())
            .charstring("end", Type2Builder::new().endchar().bytes())
            .charstring(
                "cntr",
                Type2Builder::new()
                    .num(7)
                    .vstemhm(0, 10)
                    .cntrmask(&[0x80])
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "second",
                // The width is taken by the stem, not by the later
                // odd-count rmoveto.
                Type2Builder::new()
                    .num(11)
                    .hstemhm(0, 10)
                    .num(5)
                    .rmoveto(1, 1)
                    .endchar()
                    .bytes(),
            );
        let width = |name: &str| glyph(&f, name).advance.0;
        assert_eq!(width("stem_w"), 600.0);
        assert_eq!(width("stem"), 0.0);
        assert_eq!(width("mask_w"), 550.0);
        assert_eq!(width("mask"), 0.0);
        assert_eq!(width("rmove_w"), 400.0);
        assert_eq!(width("rmove"), 0.0);
        assert_eq!(width("hmove_w"), 520.0);
        assert_eq!(width("hmove"), 0.0);
        assert_eq!(width("vmove_w"), 530.0);
        assert_eq!(width("vmove"), 0.0);
        assert_eq!(width("end_w"), 540.0);
        assert_eq!(width("end"), 0.0);
        assert_eq!(width("cntr"), 507.0);
        assert_eq!(width("second"), 511.0);
        assert_eq!(glyph(&f, "second").outline.ops, vec![MoveTo(1.0, 1.0)]);
        let defaulted = CffFont::new("D").widths(250, 500);
        let program = defaulted.program().unwrap();
        assert_eq!(
            program.glyph(b".notdef").unwrap().unwrap().advance,
            (250.0, 0.0)
        );
    }

    #[test]
    fn lines_and_curves_in_every_form() {
        let f = font()
            .charstring(
                "lines",
                Type2Builder::new()
                    .rmoveto(10, 10)
                    .rlineto(5, 5)
                    .num(5)
                    .num(6)
                    .num(7)
                    .op(6)
                    .num(1)
                    .num(2)
                    .op(7)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "curves",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .rrcurveto(1, 2, 3, 4, 5, 6)
                    .num(1)
                    .num(2)
                    .num(3)
                    .num(4)
                    .num(5)
                    .num(6)
                    .num(7)
                    .num(8)
                    .op(24)
                    .num(1)
                    .num(2)
                    .num(3)
                    .num(4)
                    .num(5)
                    .num(6)
                    .num(7)
                    .num(8)
                    .op(25)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "vv",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .num(1)
                    .num(10)
                    .num(2)
                    .num(3)
                    .num(4)
                    .op(26)
                    .num(10)
                    .num(2)
                    .num(3)
                    .num(4)
                    .op(26)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "hh",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .num(1)
                    .num(10)
                    .num(2)
                    .num(3)
                    .num(4)
                    .op(27)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "vh",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .num(10)
                    .num(20)
                    .num(30)
                    .num(40)
                    .num(1)
                    .num(2)
                    .num(3)
                    .num(4)
                    .num(9)
                    .op(30)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "hv",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .num(10)
                    .num(20)
                    .num(30)
                    .num(40)
                    .num(9)
                    .op(31)
                    .endchar()
                    .bytes(),
            );
        assert_eq!(
            glyph(&f, "lines").outline.ops,
            vec![
                MoveTo(10.0, 10.0),
                LineTo(15.0, 15.0),
                LineTo(20.0, 15.0),
                LineTo(20.0, 21.0),
                LineTo(27.0, 21.0),
                LineTo(27.0, 22.0),
                LineTo(29.0, 22.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "curves").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(1.0, 2.0, 4.0, 6.0, 9.0, 12.0),
                CurveTo(10.0, 14.0, 13.0, 18.0, 18.0, 24.0),
                LineTo(25.0, 32.0),
                LineTo(26.0, 34.0),
                CurveTo(29.0, 38.0, 34.0, 44.0, 41.0, 52.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "vv").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(1.0, 10.0, 3.0, 13.0, 3.0, 17.0),
                CurveTo(3.0, 27.0, 5.0, 30.0, 5.0, 34.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "hh").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(10.0, 1.0, 12.0, 4.0, 16.0, 4.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "vh").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(0.0, 10.0, 20.0, 40.0, 60.0, 40.0),
                CurveTo(61.0, 40.0, 63.0, 43.0, 72.0, 47.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "hv").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(10.0, 0.0, 30.0, 30.0, 39.0, 70.0),
                Close,
            ]
        );
    }

    #[test]
    fn subpaths_close_at_moves_and_the_end() {
        let f = font().charstring(
            "two",
            Type2Builder::new()
                .rmoveto(0, 0)
                .rlineto(10, 0)
                .rmoveto(0, 20)
                .rlineto(5, 5)
                .rmoveto(1, 1)
                .endchar()
                .bytes(),
        );
        assert_eq!(
            glyph(&f, "two").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                LineTo(10.0, 0.0),
                Close,
                MoveTo(10.0, 20.0),
                LineTo(15.0, 25.0),
                Close,
                MoveTo(16.0, 26.0),
            ]
        );
    }

    #[test]
    fn hints_and_masks_are_counted_and_skipped() {
        // Three stems: two from hstemhm, one implicit vstem before the
        // mask, so the mask is one byte; a second mask follows six more
        // stems, so it is two bytes.
        let f = font().charstring(
            "h",
            Type2Builder::new()
                .num(100)
                .num(0)
                .num(10)
                .num(20)
                .num(10)
                .op(18)
                .num(0)
                .num(10)
                .hintmask(&[0b1110_0000])
                .rmoveto(1, 1)
                .num(0)
                .num(1)
                .num(2)
                .num(1)
                .num(4)
                .num(1)
                .num(6)
                .num(1)
                .num(8)
                .num(1)
                .num(10)
                .num(1)
                .op(23)
                .hintmask(&[0xff, 0x80])
                .rlineto(2, 2)
                .cntrmask(&[0x00, 0x00])
                .endchar()
                .bytes(),
        );
        let g = glyph(&f, "h");
        assert_eq!(g.advance.0, 600.0);
        assert_eq!(
            g.outline.ops,
            vec![MoveTo(1.0, 1.0), LineTo(3.0, 3.0), Close]
        );
        // The trace records each mask's offset and byte count.
        let program = f.program().unwrap();
        let crate::program::Program::Cff(cff) = &program else {
            unreachable!()
        };
        assert_eq!(
            cff.reached_subrs(1).unwrap().masks,
            [
                (Site::Glyph(1), 8, 1),
                (Site::Glyph(1), 26, 2),
                (Site::Glyph(1), 32, 2)
            ]
            .into_iter()
            .collect()
        );
        let short = font().charstring("s", Type2Builder::new().num(0).num(10).op(19).bytes());
        assert_eq!(error(&short, "s"), FontError::Truncated("hint mask"));
    }

    #[test]
    fn subroutines_use_the_bias_and_are_traced() {
        let f = font()
            .subr(Type2Builder::new().rlineto(10, 0).r#return().bytes())
            .subr(Type2Builder::new().callgsubr(-107).r#return().bytes())
            .gsubr(Type2Builder::new().rlineto(0, 10).r#return().bytes())
            .gsubr(Type2Builder::new().rlineto(1, 1).bytes())
            .charstring(
                "s",
                Type2Builder::new()
                    .num(100)
                    .rmoveto(0, 0)
                    .callsubr(-107)
                    .callsubr(-106)
                    .callgsubr(-106)
                    .endchar()
                    .bytes(),
            );
        let program = f.program().unwrap();
        let g = program.glyph(b"s").unwrap().unwrap();
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                LineTo(10.0, 0.0),
                LineTo(10., 10.0),
                LineTo(11.0, 11.0),
                Close
            ]
        );
        let crate::program::Program::Cff(cff) = &program else {
            unreachable!()
        };
        let reached = cff.reached_subrs(cff.gid(b"s").unwrap()).unwrap();
        assert_eq!(reached.local, set(&[0, 1]));
        assert_eq!(reached.global, set(&[0, 1]));
        let bad = font().charstring("b", Type2Builder::new().callsubr(0).bytes());
        assert_eq!(error(&bad, "b"), FontError::SubrIndex(107));
        let recursive = font()
            .subr(Type2Builder::new().callsubr(-107).r#return().bytes())
            .charstring("r", Type2Builder::new().callsubr(-107).bytes());
        assert_eq!(error(&recursive, "r"), FontError::CallDepth);
        // A subroutine may end without `return` when `endchar` ends it.
        let ended = font()
            .subr(Type2Builder::new().rmoveto(3, 3).endchar().bytes())
            .charstring(
                "e",
                Type2Builder::new().callsubr(-107).rlineto(1, 1).bytes(),
            );
        assert_eq!(glyph(&ended, "e").outline.ops, vec![MoveTo(3.0, 3.0)]);
    }

    #[test]
    fn the_four_flex_forms_become_two_curves() {
        let f = font()
            .charstring(
                "hflex",
                Type2Builder::new()
                    .rmoveto(0, 100)
                    .hflex(10, 10, 20, 10, 10, 10, 10)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "flex",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .flex(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 50)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "hflex1",
                Type2Builder::new()
                    .rmoveto(0, 100)
                    .hflex1(10, 5, 10, 15, 10, 10, 10, -15, 10)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "flex1_h",
                Type2Builder::new()
                    .rmoveto(0, 100)
                    .flex1(10, 5, 10, 15, 10, 0, 10, 0, 10, -15, 10)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "flex1_v",
                Type2Builder::new()
                    .rmoveto(100, 0)
                    .flex1(5, 10, 15, 10, 0, 10, 0, 10, -15, 10, 10)
                    .endchar()
                    .bytes(),
            );
        assert_eq!(
            glyph(&f, "hflex").outline.ops,
            vec![
                MoveTo(0.0, 100.0),
                CurveTo(10.0, 100.0, 20.0, 120.0, 30.0, 120.0),
                CurveTo(40.0, 120.0, 50.0, 100.0, 60.0, 100.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "flex").outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(1.0, 2.0, 4.0, 6.0, 9.0, 12.0),
                CurveTo(16.0, 20.0, 25.0, 30.0, 36.0, 42.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "hflex1").outline.ops,
            vec![
                MoveTo(0.0, 100.0),
                CurveTo(10.0, 105.0, 20.0, 120.0, 30.0, 120.0),
                CurveTo(40.0, 120.0, 50.0, 105.0, 60.0, 100.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "flex1_h").outline.ops,
            vec![
                MoveTo(0.0, 100.0),
                CurveTo(10.0, 105.0, 20.0, 120.0, 30.0, 120.0),
                CurveTo(40.0, 120.0, 50.0, 105.0, 60.0, 100.0),
                Close,
            ]
        );
        assert_eq!(
            glyph(&f, "flex1_v").outline.ops,
            vec![
                MoveTo(100.0, 0.0),
                CurveTo(105.0, 10.0, 120.0, 20.0, 120.0, 30.0),
                CurveTo(120.0, 40.0, 105.0, 50.0, 100.0, 60.0),
                Close,
            ]
        );
    }

    #[test]
    fn the_accent_form_of_endchar_composes_through_the_standard_encoding() {
        let f = font()
            .charstring(
                "e",
                Type2Builder::new()
                    .num(0)
                    .rmoveto(0, 0)
                    .rlineto(400, 0)
                    .rlineto(0, 400)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "acute",
                Type2Builder::new()
                    .num(-200)
                    .rmoveto(0, 500)
                    .rlineto(100, 100)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "eacute",
                Type2Builder::new()
                    .num(0)
                    .num(150)
                    .num(20)
                    .num(101)
                    .num(194)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "missing",
                Type2Builder::new()
                    .num(0)
                    .num(0)
                    .num(101)
                    .num(66)
                    .endchar()
                    .bytes(),
            );
        let g = glyph(&f, "eacute");
        assert_eq!(g.advance, (500.0, 0.0));
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                LineTo(400.0, 0.0),
                LineTo(400.0, 400.0),
                Close,
                MoveTo(150.0, 520.0),
                LineTo(250.0, 620.0),
                Close,
            ]
        );
        let program = f.program().unwrap();
        let crate::program::Program::Cff(cff) = &program else {
            unreachable!()
        };
        let gid = cff.gid(b"eacute").unwrap();
        assert_eq!(
            cff.components(gid).unwrap(),
            vec![cff.gid(b"e").unwrap(), cff.gid(b"acute").unwrap()]
        );
        assert_eq!(
            cff.components(cff.gid(b"e").unwrap()).unwrap(),
            Vec::<u16>::new()
        );
        assert_eq!(
            error(&f, "missing"),
            FontError::MissingComponent(b"66".to_vec())
        );
    }

    #[test]
    fn arithmetic_escapes_compute_on_the_stack() {
        let f = font()
            .charstring(
                "a",
                Type2Builder::new()
                    .rmoveto(0, 0)
                    .num(3)
                    .num(4)
                    .esc(10) // add: 7
                    .num(2)
                    .esc(24) // mul: 14
                    .num(4)
                    .esc(11) // sub: 10
                    .num(4)
                    .esc(12) // div: 2.5
                    .esc(14) // neg: -2.5
                    .esc(9) // abs: 2.5
                    .num(16)
                    .esc(26) // sqrt: 4
                    .esc(27) // dup: 2.5 4 4
                    .esc(18) // drop: 2.5 4
                    .esc(28) // exch: 4 2.5
                    .num(1)
                    .esc(29) // index 1: 4 2.5 4
                    .num(3)
                    .num(1)
                    .esc(30) // roll 3 1: 4 4 2.5
                    .num(0)
                    .esc(20) // put 2.5 -> t[0]: 4 4
                    .esc(15) // eq: 1
                    .num(0)
                    .esc(21) // get t[0]: 1 2.5
                    .num(0)
                    .esc(3) // and: 1 0
                    .num(1)
                    .esc(4) // or: 1 1
                    .esc(5) // not: 1 0
                    .num(7)
                    .num(8)
                    .num(1)
                    .num(2)
                    .esc(22) // ifelse 1 <= 2 -> 7: 1 0 7
                    .esc(23) // random: 1 0 7 0
                    .op(5) // rlineto pairs: (1,0) (7,0)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "zero",
                Type2Builder::new().num(1).num(0).esc(12).endchar().bytes(),
            )
            .charstring(
                "slot",
                Type2Builder::new().num(1).num(40).esc(20).endchar().bytes(),
            )
            .charstring(
                "neg_sqrt",
                Type2Builder::new().num(-4).esc(26).endchar().bytes(),
            )
            .charstring("under", Type2Builder::new().esc(10).endchar().bytes())
            .charstring("unknown", Type2Builder::new().esc(40).endchar().bytes())
            .charstring("op", Type2Builder::new().op(2).endchar().bytes())
            .charstring("dot", Type2Builder::new().num(5).esc(0).endchar().bytes());
        assert_eq!(
            glyph(&f, "a").outline.ops,
            vec![MoveTo(0.0, 0.0), LineTo(1.0, 0.0), LineTo(8.0, 0.0), Close]
        );
        assert_eq!(error(&f, "zero"), FontError::Malformed("division by zero"));
        assert_eq!(error(&f, "slot"), FontError::Malformed("transient index"));
        assert_eq!(
            error(&f, "neg_sqrt"),
            FontError::Malformed("sqrt of a negative")
        );
        assert_eq!(error(&f, "under"), FontError::Operands("add"));
        assert_eq!(
            error(&f, "unknown"),
            FontError::UnknownOperator(12, Some(40))
        );
        assert_eq!(error(&f, "op"), FontError::UnknownOperator(2, None));
        assert_eq!(glyph(&f, "dot").advance.0, 0.0);
    }

    #[test]
    fn malformed_charstrings_are_errors() {
        let cases: Vec<(Vec<u8>, FontError)> = vec![
            (
                Type2Builder::new().num(1).bytes(),
                FontError::Truncated("charstring"),
            ),
            (vec![139, 255, 0, 0], FontError::Truncated("charstring")),
            (
                Type2Builder::new().op(5).bytes(),
                FontError::Operands("rlineto"),
            ),
            (
                Type2Builder::new().num(1).op(5).bytes(),
                FontError::Operands("rlineto"),
            ),
            (
                Type2Builder::new().op(6).bytes(),
                FontError::Operands("hlineto"),
            ),
            (
                Type2Builder::new().num(1).op(8).bytes(),
                FontError::Operands("rrcurveto"),
            ),
            (
                Type2Builder::new().num(1).op(24).bytes(),
                FontError::Operands("rcurveline"),
            ),
            (
                Type2Builder::new().num(1).op(25).bytes(),
                FontError::Operands("rlinecurve"),
            ),
            (
                Type2Builder::new().num(1).op(26).bytes(),
                FontError::Operands("vvcurveto"),
            ),
            (
                Type2Builder::new().num(1).op(30).bytes(),
                FontError::Operands("vhcurveto"),
            ),
            (
                Type2Builder::new().num(1).esc(34).bytes(),
                FontError::Operands("hflex"),
            ),
            (
                Type2Builder::new().op(21).bytes(),
                FontError::Operands("rmoveto"),
            ),
            (
                Type2Builder::new()
                    .num(1)
                    .num(2)
                    .num(3)
                    .esc(29)
                    .op(14)
                    .bytes(),
                FontError::Operands("index"),
            ),
            (
                Type2Builder::new()
                    .num(1)
                    .num(0)
                    .num(0)
                    .esc(30)
                    .op(14)
                    .bytes(),
                FontError::Operands("roll"),
            ),
            (vec![12], FontError::Truncated("charstring")),
        ];
        for (code, want) in cases {
            let f = font().charstring("m", code.clone());
            assert_eq!(error(&f, "m"), want, "{code:?}");
        }
        let overflow: Vec<u8> = std::iter::repeat_n(139u8, MAX_STACK + 1).collect();
        let f = font().charstring("o", overflow);
        assert_eq!(error(&f, "o"), FontError::Malformed("charstring stack"));
        // `index` with a negative operand copies the top.
        let f = font().charstring(
            "i",
            Type2Builder::new()
                .rmoveto(0, 0)
                .num(5)
                .num(-1)
                .esc(29)
                .op(5)
                .endchar()
                .bytes(),
        );
        assert_eq!(
            glyph(&f, "i").outline.ops,
            vec![MoveTo(0.0, 0.0), LineTo(5.0, 5.0), Close]
        );
    }
}
