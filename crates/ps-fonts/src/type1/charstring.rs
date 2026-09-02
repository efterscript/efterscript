// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The Type 1 charstring interpreter: the path operators build an
//! outline, `hsbw`/`sbw` give the advance, subroutines run through an
//! explicit call stack, flex is reconstructed from the other-subroutine
//! convention, hints are accepted and dropped, and `seac` composes two
//! other charstrings.

use super::Type1Program;
use crate::encoding::STANDARD_ENCODING;
use crate::outline::{Glyph, Outline};
use crate::program::FontError;

/// Deeper subroutine nesting than this is a fault, not a font.
const MAX_CALL_DEPTH: usize = 30;
/// The charstring operand stack the format allows.
const MAX_STACK: usize = 24;

pub(crate) struct Interpreted {
    pub glyph: Glyph,
    pub components: Vec<Vec<u8>>,
}

/// Interprets the charstring of `name`; `None` when there is none.
pub(crate) fn interpret(
    program: &Type1Program,
    name: &[u8],
) -> Result<Option<Interpreted>, FontError> {
    let Some(code) = program.charstring(name) else {
        return Ok(None);
    };
    let mut machine = Machine::new(program);
    machine.run(code)?;
    if let Some(seac) = machine.seac.take() {
        return machine.compose(seac).map(Some);
    }
    Ok(Some(Interpreted {
        glyph: machine.into_glyph(),
        components: Vec::new(),
    }))
}

struct Seac {
    asb: f32,
    adx: f32,
    ady: f32,
    base: Vec<u8>,
    accent: Vec<u8>,
}

struct Machine<'a> {
    program: &'a Type1Program,
    stack: Vec<f32>,
    /// Values other-subroutines leave for `pop`.
    ps_stack: Vec<f32>,
    outline: Outline,
    x: f32,
    y: f32,
    sbx: f32,
    advance: (f32, f32),
    /// The points collected while a flex is in progress.
    flex: Option<Vec<(f32, f32)>>,
    seac: Option<Seac>,
}

enum Step<'a> {
    Continue,
    Call(&'a [u8]),
    Return,
    End,
}

/// One number of the charstring encoding, or `None` for an operator byte.
fn number(code: &[u8], pc: &mut usize) -> Result<Option<f32>, FontError> {
    let v = code[*pc];
    *pc += 1;
    let value = match v {
        32..=246 => i32::from(v) - 139,
        247..=250 => {
            let w = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
            *pc += 1;
            (i32::from(v) - 247) * 256 + i32::from(w) + 108
        }
        251..=254 => {
            let w = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
            *pc += 1;
            -(i32::from(v) - 251) * 256 - i32::from(w) - 108
        }
        255 => {
            let bytes = code
                .get(*pc..*pc + 4)
                .ok_or(FontError::Truncated("charstring"))?;
            *pc += 4;
            i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        _ => return Ok(None),
    };
    Ok(Some(value as f32))
}

impl<'a> Machine<'a> {
    fn new(program: &'a Type1Program) -> Self {
        Machine {
            program,
            stack: Vec::new(),
            ps_stack: Vec::new(),
            outline: Outline::new(),
            x: 0.0,
            y: 0.0,
            sbx: 0.0,
            advance: (0.0, 0.0),
            flex: None,
            seac: None,
        }
    }

    fn into_glyph(self) -> Glyph {
        Glyph {
            advance: self.advance,
            outline: self.outline,
        }
    }

    /// Runs `code` and the subroutines it calls until `endchar`, `seac`,
    /// or the end of the top-level charstring.
    fn run(&mut self, code: &[u8]) -> Result<(), FontError> {
        let mut frames: Vec<(&[u8], usize)> = vec![(code, 0)];
        loop {
            let Some(&(code, mut pc)) = frames.last() else {
                return Ok(());
            };
            if pc >= code.len() {
                frames.pop();
                if frames.is_empty() {
                    // The format requires an explicit end; a charstring
                    // that runs off its end is the malformed case.
                    return Err(FontError::Truncated("charstring"));
                }
                continue;
            }
            let step = self.step(code, &mut pc, frames.len())?;
            frames.last_mut().expect("frame present").1 = pc;
            match step {
                Step::Continue => {}
                Step::Call(subr) => frames.push((subr, 0)),
                Step::Return => {
                    frames.pop();
                }
                Step::End => return Ok(()),
            }
        }
    }

    /// One number or operator at `pc`.
    fn step(&mut self, code: &[u8], pc: &mut usize, depth: usize) -> Result<Step<'a>, FontError> {
        if let Some(value) = number(code, pc)? {
            if self.stack.len() >= MAX_STACK {
                return Err(FontError::Malformed("charstring stack"));
            }
            self.stack.push(value);
            return Ok(Step::Continue);
        }
        let op = code[*pc - 1];
        match op {
            12 => {
                let esc = *code.get(*pc).ok_or(FontError::Truncated("charstring"))?;
                *pc += 1;
                self.escaped(esc)
            }
            10 => {
                let index = self.stack.pop().ok_or(FontError::Operands("callsubr"))?;
                let subr = usize::try_from(index as i32)
                    .ok()
                    .and_then(|k| self.program.subrs().get(k))
                    .ok_or(FontError::SubrIndex(index as i32))?;
                if depth >= MAX_CALL_DEPTH {
                    return Err(FontError::CallDepth);
                }
                Ok(Step::Call(subr.as_slice()))
            }
            11 => Ok(Step::Return),
            _ => self.plain(op),
        }
    }

    /// The first `n` operands, taken from the bottom of the stack as the
    /// format specifies; the stack is cleared.
    fn args(&mut self, n: usize, op: &'static str) -> Result<Vec<f32>, FontError> {
        if self.stack.len() < n {
            return Err(FontError::Operands(op));
        }
        let args = self.stack[..n].to_vec();
        self.stack.clear();
        Ok(args)
    }

    fn move_to(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
        match &mut self.flex {
            Some(points) => points.push((self.x, self.y)),
            None => self.outline.move_to(self.x, self.y),
        }
    }

    fn line_to(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
        self.outline.line_to(self.x, self.y);
    }

    fn curve_to(&mut self, d: [f32; 6]) {
        let x1 = self.x + d[0];
        let y1 = self.y + d[1];
        let x2 = x1 + d[2];
        let y2 = y1 + d[3];
        self.x = x2 + d[4];
        self.y = y2 + d[5];
        self.outline.curve_to(x1, y1, x2, y2, self.x, self.y);
    }

    fn plain(&mut self, op: u8) -> Result<Step<'a>, FontError> {
        match op {
            // hstem, vstem
            1 | 3 => {
                self.args(2, "stem")?;
            }
            4 => {
                let a = self.args(1, "vmoveto")?;
                self.move_to(0.0, a[0]);
            }
            5 => {
                let a = self.args(2, "rlineto")?;
                self.line_to(a[0], a[1]);
            }
            6 => {
                let a = self.args(1, "hlineto")?;
                self.line_to(a[0], 0.0);
            }
            7 => {
                let a = self.args(1, "vlineto")?;
                self.line_to(0.0, a[0]);
            }
            8 => {
                let a = self.args(6, "rrcurveto")?;
                self.curve_to([a[0], a[1], a[2], a[3], a[4], a[5]]);
            }
            9 => {
                self.stack.clear();
                self.outline.close();
            }
            13 => {
                let a = self.args(2, "hsbw")?;
                self.sbx = a[0];
                self.x = a[0];
                self.y = 0.0;
                self.advance = (a[1], 0.0);
            }
            14 => {
                self.stack.clear();
                return Ok(Step::End);
            }
            21 => {
                let a = self.args(2, "rmoveto")?;
                self.move_to(a[0], a[1]);
            }
            22 => {
                let a = self.args(1, "hmoveto")?;
                self.move_to(a[0], 0.0);
            }
            30 => {
                let a = self.args(4, "vhcurveto")?;
                self.curve_to([0.0, a[0], a[1], a[2], a[3], 0.0]);
            }
            31 => {
                let a = self.args(4, "hvcurveto")?;
                self.curve_to([a[0], 0.0, a[1], a[2], 0.0, a[3]]);
            }
            _ => return Err(FontError::UnknownOperator(op, None)),
        }
        Ok(Step::Continue)
    }

    fn escaped(&mut self, esc: u8) -> Result<Step<'a>, FontError> {
        match esc {
            // dotsection
            0 => self.stack.clear(),
            // vstem3, hstem3
            1 | 2 => {
                self.args(6, "stem3")?;
            }
            6 => {
                let a = self.args(5, "seac")?;
                let name = |code: f32| {
                    u8::try_from(code as i32)
                        .ok()
                        .and_then(|c| STANDARD_ENCODING[usize::from(c)])
                        .map(|n| n.as_bytes().to_vec())
                        .ok_or(FontError::Malformed("seac code"))
                };
                self.seac = Some(Seac {
                    asb: a[0],
                    adx: a[1],
                    ady: a[2],
                    base: name(a[3])?,
                    accent: name(a[4])?,
                });
                return Ok(Step::End);
            }
            7 => {
                let a = self.args(4, "sbw")?;
                self.sbx = a[0];
                self.x = a[0];
                self.y = a[1];
                self.advance = (a[2], a[3]);
            }
            12 => {
                let b = self.stack.pop().ok_or(FontError::Operands("div"))?;
                let a = self.stack.pop().ok_or(FontError::Operands("div"))?;
                if b == 0.0 {
                    return Err(FontError::Malformed("division by zero"));
                }
                self.stack.push(a / b);
            }
            16 => self.call_other_subr()?,
            17 => {
                let value = self.ps_stack.pop().ok_or(FontError::Operands("pop"))?;
                if self.stack.len() >= MAX_STACK {
                    return Err(FontError::Malformed("charstring stack"));
                }
                self.stack.push(value);
            }
            33 => {
                let a = self.args(2, "setcurrentpoint")?;
                self.x = a[0];
                self.y = a[1];
            }
            _ => return Err(FontError::UnknownOperator(12, Some(esc))),
        }
        Ok(Step::Continue)
    }

    /// `callothersubr`: flex (0–2) and hint replacement (3) have their
    /// documented meanings; any other other-subroutine hands its
    /// arguments to the PostScript stack for `pop` to retrieve.
    fn call_other_subr(&mut self) -> Result<(), FontError> {
        let which = self
            .stack
            .pop()
            .ok_or(FontError::Operands("callothersubr"))? as i32;
        let n = self
            .stack
            .pop()
            .ok_or(FontError::Operands("callothersubr"))? as i32;
        let n = usize::try_from(n).map_err(|_| FontError::Malformed("callothersubr count"))?;
        if self.stack.len() < n {
            return Err(FontError::Operands("callothersubr"));
        }
        let args = self.stack.split_off(self.stack.len() - n);
        match which {
            0 => {
                let points = self.flex.take().ok_or(FontError::Malformed("flex end"))?;
                if args.len() != 3 || points.len() != 7 {
                    return Err(FontError::Malformed("flex"));
                }
                let p = &points[1..];
                self.outline
                    .curve_to(p[0].0, p[0].1, p[1].0, p[1].1, p[2].0, p[2].1);
                self.outline
                    .curve_to(p[3].0, p[3].1, p[4].0, p[4].1, p[5].0, p[5].1);
                self.x = args[1];
                self.y = args[2];
                // `pop pop setcurrentpoint` follows: x must come off first.
                self.ps_stack.push(args[2]);
                self.ps_stack.push(args[1]);
            }
            1 => {
                if self.flex.is_some() {
                    return Err(FontError::Malformed("nested flex"));
                }
                self.flex = Some(Vec::new());
            }
            2 => {}
            3 => {
                let subr = args.first().copied().unwrap_or(3.0);
                self.ps_stack.push(subr);
            }
            _ => self.ps_stack.extend(args),
        }
        Ok(())
    }

    /// The composite glyph: the base as it is, the accent displaced so its
    /// sidebearing point lands at `adx − asb` from this glyph's own.
    fn compose(self, seac: Seac) -> Result<Interpreted, FontError> {
        let component = |name: &[u8]| -> Result<Glyph, FontError> {
            let code = self
                .program
                .charstring(name)
                .ok_or_else(|| FontError::MissingComponent(name.to_vec()))?;
            let mut machine = Machine::new(self.program);
            machine.run(code)?;
            if machine.seac.is_some() {
                return Err(FontError::Malformed("nested seac"));
            }
            Ok(machine.into_glyph())
        };
        let base = component(&seac.base)?;
        let accent = component(&seac.accent)?;
        let mut outline = base.outline;
        let dx = self.sbx - seac.asb + seac.adx;
        outline
            .ops
            .extend(accent.outline.translated(dx, seac.ady).ops);
        Ok(Interpreted {
            glyph: Glyph {
                advance: self.advance,
                outline,
            },
            components: vec![seac.base, seac.accent],
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::outline::OutlineOp::{Close, CurveTo, LineTo, MoveTo};
    use crate::testing::CharstringBuilder;

    fn program(glyphs: Vec<(&str, Vec<u8>)>, subrs: Vec<Vec<u8>>) -> Type1Program {
        let charstrings: BTreeMap<Vec<u8>, Vec<u8>> = glyphs
            .into_iter()
            .map(|(name, code)| (name.as_bytes().to_vec(), code))
            .collect();
        Type1Program::from_decrypted(4, subrs, charstrings)
    }

    fn glyph(p: &Type1Program, name: &str) -> Glyph {
        (*p.glyph(name.as_bytes()).unwrap().unwrap()).clone()
    }

    #[test]
    fn numbers_decode_in_every_encoding() {
        let mut bytes = Vec::new();
        for v in [
            0, 107, -107, 108, 1131, -108, -1131, 5000, -5000, 40000, -40000,
        ] {
            crate::testing::encode_number(v, &mut bytes);
        }
        let mut pc = 0;
        let mut values = Vec::new();
        while pc < bytes.len() {
            values.push(number(&bytes, &mut pc).unwrap().unwrap() as i32);
        }
        assert_eq!(
            values,
            [
                0, 107, -107, 108, 1131, -108, -1131, 5000, -5000, 40000, -40000
            ]
        );
        assert_eq!(
            number(&[255, 0], &mut 0),
            Err(FontError::Truncated("charstring"))
        );
        assert_eq!(
            number(&[247], &mut 0),
            Err(FontError::Truncated("charstring"))
        );
    }

    #[test]
    fn a_square_from_lines_and_the_sidebearing() {
        let square = CharstringBuilder::new()
            .hsbw(50, 600)
            .rmoveto(0, 0)
            .hlineto(500)
            .vlineto(500)
            .rlineto(-500, 0)
            .closepath()
            .endchar()
            .bytes();
        let p = program(vec![("a", square)], Vec::new());
        let g = glyph(&p, "a");
        assert_eq!(g.advance, (600.0, 0.0));
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(50.0, 0.0),
                LineTo(550.0, 0.0),
                LineTo(550.0, 500.0),
                LineTo(50.0, 500.0),
                Close
            ]
        );
        assert!(p.glyph(b"zz").unwrap().is_none());
        assert!(std::rc::Rc::ptr_eq(
            &p.glyph(b"a").unwrap().unwrap(),
            &p.glyph(b"a").unwrap().unwrap()
        ));
    }

    #[test]
    fn curves_moves_and_the_vertical_variants() {
        let code = CharstringBuilder::new()
            .sbw(10, 20, 700, 5)
            .vmoveto(100)
            .hmoveto(30)
            .rrcurveto(1, 2, 3, 4, 5, 6)
            .vhcurveto(10, 20, 30, 40)
            .hvcurveto(10, 20, 30, 40)
            .hstem(1, 2)
            .vstem(3, 4)
            .dotsection()
            .vstem3(1, 2, 3, 4, 5, 6)
            .hstem3(1, 2, 3, 4, 5, 6)
            .endchar()
            .bytes();
        let p = program(vec![("c", code)], Vec::new());
        let g = glyph(&p, "c");
        assert_eq!(g.advance, (700.0, 5.0));
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(10.0, 120.0),
                MoveTo(40.0, 120.0),
                CurveTo(41.0, 122.0, 44.0, 126.0, 49.0, 132.0),
                CurveTo(49.0, 142.0, 69.0, 172.0, 109.0, 172.0),
                CurveTo(119.0, 172.0, 139.0, 202.0, 139.0, 242.0),
            ]
        );
    }

    #[test]
    fn subroutines_div_and_setcurrentpoint() {
        let subr = CharstringBuilder::new().rlineto(10, 0).r#return().bytes();
        let code = CharstringBuilder::new()
            .hsbw(0, 500)
            .rmoveto(0, 0)
            .callsubr(0)
            .callsubr(0)
            .num(100)
            .num(4)
            .div()
            .num(0)
            .op(5)
            .num(7)
            .num(9)
            .setcurrentpoint()
            .rlineto(1, 1)
            .endchar()
            .bytes();
        let p = program(vec![("s", code)], vec![subr]);
        let g = glyph(&p, "s");
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                LineTo(10.0, 0.0),
                LineTo(20.0, 0.0),
                LineTo(45.0, 0.0),
                LineTo(8.0, 10.0),
            ]
        );
    }

    #[test]
    fn flex_becomes_two_curves() {
        let subrs = vec![
            CharstringBuilder::new()
                .num(3)
                .num(0)
                .callothersubr()
                .pop()
                .pop()
                .setcurrentpoint()
                .r#return()
                .bytes(),
            CharstringBuilder::new()
                .num(0)
                .num(1)
                .callothersubr()
                .r#return()
                .bytes(),
            CharstringBuilder::new()
                .num(0)
                .num(2)
                .callothersubr()
                .r#return()
                .bytes(),
            CharstringBuilder::new().r#return().bytes(),
        ];
        let mut b = CharstringBuilder::new()
            .hsbw(0, 400)
            .rmoveto(0, 0)
            .callsubr(1);
        for (dx, dy) in [
            (0, 50),
            (10, 10),
            (10, 0),
            (10, -5),
            (10, 0),
            (10, 5),
            (10, 10),
        ] {
            b = b.rmoveto(dx, dy).callsubr(2);
        }
        let code = b
            .num(50)
            .num(60)
            .num(70)
            .callsubr(0)
            .rlineto(0, -70)
            .closepath()
            .endchar()
            .bytes();
        let p = program(vec![("f", code)], subrs);
        let g = glyph(&p, "f");
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(10.0, 60.0, 20.0, 60.0, 30.0, 55.0),
                CurveTo(40.0, 55.0, 50.0, 60.0, 60.0, 70.0),
                LineTo(60.0, 0.0),
                Close,
            ]
        );
    }

    #[test]
    fn hint_replacement_and_unknown_other_subrs_feed_pop() {
        let subrs = vec![
            CharstringBuilder::new().r#return().bytes(),
            CharstringBuilder::new().r#return().bytes(),
            CharstringBuilder::new().r#return().bytes(),
            CharstringBuilder::new().r#return().bytes(),
            CharstringBuilder::new().hstem(0, 10).r#return().bytes(),
        ];
        let code = CharstringBuilder::new()
            .hsbw(0, 300)
            .num(4)
            .num(1)
            .num(3)
            .callothersubr()
            .pop()
            .op(10)
            .num(5)
            .num(6)
            .num(2)
            .num(13)
            .callothersubr()
            .pop()
            .pop()
            .op(21)
            .endchar()
            .bytes();
        let p = program(vec![("h", code)], subrs);
        let g = glyph(&p, "h");
        assert_eq!(g.outline.ops, vec![MoveTo(6.0, 5.0)]);
    }

    #[test]
    fn seac_composes_base_and_accent() {
        let e = CharstringBuilder::new()
            .hsbw(20, 500)
            .rmoveto(0, 0)
            .rlineto(400, 0)
            .rlineto(0, 400)
            .closepath()
            .endchar()
            .bytes();
        let acute = CharstringBuilder::new()
            .hsbw(30, 300)
            .rmoveto(0, 500)
            .rlineto(100, 100)
            .closepath()
            .endchar()
            .bytes();
        let eacute = CharstringBuilder::new()
            .hsbw(20, 500)
            .seac(30, 150, 20, 101, 194)
            .bytes();
        let p = program(
            vec![("e", e), ("acute", acute), ("eacute", eacute)],
            Vec::new(),
        );
        let g = glyph(&p, "eacute");
        assert_eq!(g.advance, (500.0, 0.0));
        assert_eq!(
            g.outline.ops,
            vec![
                MoveTo(20.0, 0.0),
                LineTo(420.0, 0.0),
                LineTo(420.0, 400.0),
                Close,
                MoveTo(170.0, 520.0),
                LineTo(270.0, 620.0),
                Close,
            ]
        );
        assert_eq!(
            p.seac_components(b"eacute").unwrap(),
            vec![b"e".to_vec(), b"acute".to_vec()]
        );
        assert_eq!(p.seac_components(b"e").unwrap(), Vec::<Vec<u8>>::new());
        let missing = CharstringBuilder::new()
            .hsbw(0, 500)
            .seac(0, 0, 0, 101, 66)
            .bytes();
        let p = program(vec![("x", missing)], Vec::new());
        assert_eq!(
            p.glyph(b"x"),
            Err(FontError::MissingComponent(b"e".to_vec()))
        );
    }

    #[test]
    fn malformed_charstrings_are_errors() {
        let cases: Vec<(Vec<u8>, FontError)> = vec![
            (
                CharstringBuilder::new().hsbw(0, 500).num(1).bytes(),
                FontError::Truncated("charstring"),
            ),
            (vec![139, 255, 0, 0], FontError::Truncated("charstring")),
            (
                CharstringBuilder::new().hsbw(0, 500).op(5).bytes(),
                FontError::Operands("rlineto"),
            ),
            (
                CharstringBuilder::new().hsbw(0, 500).op(2).bytes(),
                FontError::UnknownOperator(2, None),
            ),
            (
                CharstringBuilder::new().hsbw(0, 500).esc(40).bytes(),
                FontError::UnknownOperator(12, Some(40)),
            ),
            (
                CharstringBuilder::new().hsbw(0, 500).callsubr(7).bytes(),
                FontError::SubrIndex(7),
            ),
            (
                CharstringBuilder::new().hsbw(0, 500).pop().bytes(),
                FontError::Operands("pop"),
            ),
            (
                CharstringBuilder::new().num(1).num(0).div().bytes(),
                FontError::Malformed("division by zero"),
            ),
            (
                CharstringBuilder::new()
                    .num(0)
                    .num(0)
                    .callothersubr()
                    .bytes(),
                FontError::Malformed("flex end"),
            ),
            (vec![12], FontError::Truncated("charstring")),
        ];
        for (code, want) in cases {
            let p = program(vec![("m", code)], Vec::new());
            assert_eq!(p.glyph(b"m"), Err(want));
        }
        let recursive = CharstringBuilder::new().callsubr(0).bytes();
        let p = program(vec![("r", recursive.clone())], vec![recursive]);
        assert_eq!(p.glyph(b"r"), Err(FontError::CallDepth));
        let overflow: Vec<u8> = std::iter::repeat_n(139u8, 30).collect();
        let p = program(vec![("o", overflow)], Vec::new());
        assert_eq!(p.glyph(b"o"), Err(FontError::Malformed("charstring stack")));
    }
}
