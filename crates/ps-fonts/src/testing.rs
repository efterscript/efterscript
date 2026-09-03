// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Synthesised fonts for tests in several crates: a charstring encoder, a
//! Type 1 program builder that yields both an in-memory [`Program`] and a
//! complete `eexec` font program, and a TrueType builder that yields the
//! program bytes and its Type 42 wrapper. Not compiled out of release
//! builds, since the corpus generator and other crates' tests use it.

use std::collections::BTreeMap;

use crate::outline::{Outline, OutlineOp};
use crate::program::{FontError, Program};
use crate::truetype::TrueTypeProgram;
use crate::truetype::write::{self, Table};
use crate::type1::write::encrypt_section_binary;
use crate::type1::{CHARSTRING_KEY, EEXEC_KEY, Type1Dict, Type1Program, encrypt};

pub use crate::type1::charstring::encode_number;
pub use crate::type1::write::TRAILER;

/// A charstring assembled operator by operator.
#[derive(Clone, Debug, Default)]
pub struct CharstringBuilder {
    bytes: Vec<u8>,
}

impl CharstringBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn num(mut self, v: i32) -> Self {
        encode_number(v, &mut self.bytes);
        self
    }

    pub fn op(mut self, op: u8) -> Self {
        self.bytes.push(op);
        self
    }

    pub fn esc(mut self, op: u8) -> Self {
        self.bytes.push(12);
        self.bytes.push(op);
        self
    }

    pub fn hstem(self, y: i32, dy: i32) -> Self {
        self.num(y).num(dy).op(1)
    }

    pub fn vstem(self, x: i32, dx: i32) -> Self {
        self.num(x).num(dx).op(3)
    }

    pub fn vmoveto(self, dy: i32) -> Self {
        self.num(dy).op(4)
    }

    pub fn rlineto(self, dx: i32, dy: i32) -> Self {
        self.num(dx).num(dy).op(5)
    }

    pub fn hlineto(self, dx: i32) -> Self {
        self.num(dx).op(6)
    }

    pub fn vlineto(self, dy: i32) -> Self {
        self.num(dy).op(7)
    }

    pub fn rrcurveto(self, d: i32, e: i32, f: i32, g: i32, h: i32, k: i32) -> Self {
        self.num(d).num(e).num(f).num(g).num(h).num(k).op(8)
    }

    pub fn closepath(self) -> Self {
        self.op(9)
    }

    pub fn callsubr(self, index: i32) -> Self {
        self.num(index).op(10)
    }

    pub fn r#return(self) -> Self {
        self.op(11)
    }

    pub fn hsbw(self, sbx: i32, wx: i32) -> Self {
        self.num(sbx).num(wx).op(13)
    }

    pub fn endchar(self) -> Self {
        self.op(14)
    }

    pub fn rmoveto(self, dx: i32, dy: i32) -> Self {
        self.num(dx).num(dy).op(21)
    }

    pub fn hmoveto(self, dx: i32) -> Self {
        self.num(dx).op(22)
    }

    pub fn vhcurveto(self, a: i32, b: i32, c: i32, d: i32) -> Self {
        self.num(a).num(b).num(c).num(d).op(30)
    }

    pub fn hvcurveto(self, a: i32, b: i32, c: i32, d: i32) -> Self {
        self.num(a).num(b).num(c).num(d).op(31)
    }

    pub fn dotsection(self) -> Self {
        self.esc(0)
    }

    pub fn vstem3(self, a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) -> Self {
        self.num(a).num(b).num(c).num(d).num(e).num(f).esc(1)
    }

    pub fn hstem3(self, a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) -> Self {
        self.num(a).num(b).num(c).num(d).num(e).num(f).esc(2)
    }

    pub fn seac(self, asb: i32, adx: i32, ady: i32, bchar: i32, achar: i32) -> Self {
        self.num(asb).num(adx).num(ady).num(bchar).num(achar).esc(6)
    }

    pub fn sbw(self, sbx: i32, sby: i32, wx: i32, wy: i32) -> Self {
        self.num(sbx).num(sby).num(wx).num(wy).esc(7)
    }

    pub fn div(self) -> Self {
        self.esc(12)
    }

    pub fn callothersubr(self) -> Self {
        self.esc(16)
    }

    pub fn pop(self) -> Self {
        self.esc(17)
    }

    pub fn setcurrentpoint(self) -> Self {
        self.esc(33)
    }

    pub fn bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// The charstring encrypted with `len_iv` leading bytes.
    pub fn encrypted(self, len_iv: usize) -> Vec<u8> {
        encrypt(CHARSTRING_KEY, &self.bytes, len_iv)
    }
}

/// A charstring drawing `outline` (integral coordinates in glyph space)
/// with sidebearing `sbx` and advance `wx`: relative moves, lines, and
/// curves, closed where the outline closes.
pub fn charstring(sbx: i32, wx: i32, outline: &Outline) -> Vec<u8> {
    let mut b = CharstringBuilder::new().hsbw(sbx, wx);
    let (mut x, mut y) = (sbx, 0);
    let r = |v: f32| v.round() as i32;
    for op in &outline.ops {
        match *op {
            OutlineOp::MoveTo(px, py) => {
                b = b.rmoveto(r(px) - x, r(py) - y);
                (x, y) = (r(px), r(py));
            }
            OutlineOp::LineTo(px, py) => {
                b = b.rlineto(r(px) - x, r(py) - y);
                (x, y) = (r(px), r(py));
            }
            OutlineOp::CurveTo(x1, y1, x2, y2, px, py) => {
                b = b.rrcurveto(
                    r(x1) - x,
                    r(y1) - y,
                    r(x2) - r(x1),
                    r(y2) - r(y1),
                    r(px) - r(x2),
                    r(py) - r(y2),
                );
                (x, y) = (r(px), r(py));
            }
            OutlineOp::Close => b = b.closepath(),
        }
    }
    b.endchar().bytes()
}

/// A rectangle outline from `(x0, y0)` to `(x1, y1)`.
pub fn rectangle(x0: f32, y0: f32, x1: f32, y1: f32) -> Outline {
    let mut o = Outline::new();
    o.move_to(x0, y0);
    o.line_to(x1, y0);
    o.line_to(x1, y1);
    o.line_to(x0, y1);
    o.close();
    o
}

/// Hexadecimal text in lines of 64 digits, each ending in a newline.
pub fn hex_lines(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + bytes.len() / 32 + 1);
    for (k, byte) in bytes.iter().enumerate() {
        if k > 0 && k % 32 == 0 {
            out.push('\n');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out.push('\n');
    out
}

/// Encrypts an `eexec` section in binary form; see
/// [`encrypt_section_binary`].
pub fn eexec_binary(plain: &[u8]) -> Vec<u8> {
    encrypt_section_binary(plain)
}

/// Encrypts an `eexec` section in hexadecimal form.
pub fn eexec_hex(plain: &[u8]) -> String {
    hex_lines(&encrypt(EEXEC_KEY, plain, 4))
}

/// A Type 1 font built from plain charstrings.
#[derive(Clone, Debug)]
pub struct Type1Font {
    pub name: String,
    pub bbox: [i32; 4],
    /// Plain charstrings by name, in definition order.
    pub glyphs: Vec<(String, Vec<u8>)>,
    /// Plain subroutines in index order.
    pub subrs: Vec<Vec<u8>>,
    pub encoding: Vec<(u8, String)>,
}

impl Type1Font {
    /// A font with a `.notdef` of zero width and no encoding entries.
    pub fn new(name: &str) -> Self {
        Type1Font {
            name: name.to_string(),
            bbox: [0, 0, 1000, 1000],
            glyphs: vec![(
                ".notdef".to_string(),
                CharstringBuilder::new().hsbw(0, 0).endchar().bytes(),
            )],
            subrs: Vec::new(),
            encoding: Vec::new(),
        }
    }

    pub fn bbox(mut self, bbox: [i32; 4]) -> Self {
        self.bbox = bbox;
        self
    }

    /// A glyph drawn from an outline, advance `wx`.
    pub fn glyph(self, name: &str, wx: i32, outline: &Outline) -> Self {
        self.charstring(name, charstring(0, wx, outline))
    }

    /// A glyph from a plain charstring.
    pub fn charstring(mut self, name: &str, code: Vec<u8>) -> Self {
        self.glyphs.push((name.to_string(), code));
        self
    }

    pub fn subr(mut self, code: Vec<u8>) -> Self {
        self.subrs.push(code);
        self
    }

    /// The four standard subroutines flex and hint replacement call.
    pub fn standard_subrs(self) -> Self {
        self.subr(
            CharstringBuilder::new()
                .num(3)
                .num(0)
                .callothersubr()
                .pop()
                .pop()
                .setcurrentpoint()
                .r#return()
                .bytes(),
        )
        .subr(
            CharstringBuilder::new()
                .num(0)
                .num(1)
                .callothersubr()
                .r#return()
                .bytes(),
        )
        .subr(
            CharstringBuilder::new()
                .num(0)
                .num(2)
                .callothersubr()
                .r#return()
                .bytes(),
        )
        .subr(CharstringBuilder::new().r#return().bytes())
    }

    pub fn encode(mut self, code: u8, name: &str) -> Self {
        self.encoding.push((code, name.to_string()));
        self
    }

    /// The in-memory program, carrying the dictionary entries the font
    /// file defines (see [`Type1Font::cleartext`] and
    /// [`Type1Font::private_text`]).
    pub fn program(&self) -> Program {
        let charstrings: BTreeMap<Vec<u8>, Vec<u8>> = self
            .glyphs
            .iter()
            .map(|(name, code)| (name.as_bytes().to_vec(), code.clone()))
            .collect();
        let entries = |list: &[(&str, &str)]| -> Vec<(Vec<u8>, Vec<u8>)> {
            list.iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect()
        };
        let dict = Type1Dict {
            font_bbox: self.bbox.map(|v| v as f32),
            paint_type: 0,
            font_info: entries(&[("ItalicAngle", "0"), ("isFixedPitch", "false")]),
            private: entries(&[
                ("password", "5839"),
                ("MinFeature", "{16 16}"),
                ("BlueValues", "[]"),
                ("OtherSubrs", "[{} {} {} {}]"),
            ]),
        };
        Program::Type1(
            Type1Program::from_decrypted(4, self.subrs.clone(), charstrings).with_dict(dict),
        )
    }

    /// The cleartext portion, up to and including `currentfile eexec`
    /// and its newline.
    pub fn cleartext(&self) -> String {
        let mut out = format!(
            "%!PS-AdobeFont-1.0: {name}\n\
             11 dict begin\n\
             /FontInfo 2 dict dup begin\n\
             /ItalicAngle 0 def\n\
             /isFixedPitch false def\n\
             end readonly def\n\
             /FontName /{name} def\n\
             /PaintType 0 def\n\
             /FontType 1 def\n\
             /FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
             /Encoding 256 array\n\
             0 1 255 {{1 index exch /.notdef put}} for\n",
            name = self.name
        );
        for (code, name) in &self.encoding {
            out.push_str(&format!("dup {code} /{name} put\n"));
        }
        out.push_str(&format!(
            "readonly def\n\
             /FontBBox {{{} {} {} {}}} readonly def\n\
             currentdict end\n\
             currentfile eexec\n",
            self.bbox[0], self.bbox[1], self.bbox[2], self.bbox[3]
        ));
        out
    }

    /// The plain text of the encrypted portion: the private dictionary,
    /// the subroutines and charstrings (each encrypted with `lenIV` 4),
    /// `definefont`, and the closing of the file.
    pub fn private_text(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(
            b"dup /Private 8 dict dup begin\n\
              /RD {string currentfile exch readstring pop} executeonly def\n\
              /ND {noaccess def} executeonly def\n\
              /NP {noaccess put} executeonly def\n\
              /lenIV 4 def\n\
              /password 5839 def\n\
              /MinFeature {16 16} def\n\
              /BlueValues [] def\n\
              /OtherSubrs [ {} {} {} {} ] def\n",
        );
        if !self.subrs.is_empty() {
            out.extend_from_slice(format!("/Subrs {} array\n", self.subrs.len()).as_bytes());
            for (k, code) in self.subrs.iter().enumerate() {
                let cipher = encrypt(CHARSTRING_KEY, code, 4);
                out.extend_from_slice(format!("dup {k} {} RD ", cipher.len()).as_bytes());
                out.extend_from_slice(&cipher);
                out.extend_from_slice(b" NP\n");
            }
            out.extend_from_slice(b"ND\n");
        }
        // The private dictionary is stored (and sealed) while it stays on
        // the dictionary stack, so `RD` and `ND` remain in reach for the
        // charstrings; the two `end`s follow the two `put`s.
        out.extend_from_slice(b"noaccess put\n");
        out.extend_from_slice(
            format!("dup /CharStrings {} dict dup begin\n", self.glyphs.len()).as_bytes(),
        );
        for (name, code) in &self.glyphs {
            let cipher = encrypt(CHARSTRING_KEY, code, 4);
            out.extend_from_slice(format!("/{name} {} RD ", cipher.len()).as_bytes());
            out.extend_from_slice(&cipher);
            out.extend_from_slice(b" ND\n");
        }
        out.extend_from_slice(
            b"end\n\
              readonly put\n\
              end\n\
              dup /FontName get exch definefont pop\n\
              mark currentfile closefile\n",
        );
        out
    }

    /// The complete program with a hexadecimal `eexec` section.
    pub fn pfa(&self) -> String {
        let mut out = self.cleartext();
        out.push_str(&eexec_hex(&self.private_text()));
        out.push_str(TRAILER);
        out
    }

    /// The complete program with a binary `eexec` section.
    pub fn pfa_binary(&self) -> Vec<u8> {
        let mut out = self.cleartext().into_bytes();
        out.extend_from_slice(&eexec_binary(&self.private_text()));
        out.push(b'\n');
        out.extend_from_slice(TRAILER.as_bytes());
        out
    }

    /// The program in PFB form: an ASCII segment with the cleartext, a
    /// binary segment with the encrypted section, an ASCII segment with
    /// the trailer, and the end marker.
    pub fn pfb(&self) -> Vec<u8> {
        let segment = |kind: u8, data: &[u8], out: &mut Vec<u8>| {
            out.extend_from_slice(&[0x80, kind]);
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
        };
        let mut out = Vec::new();
        segment(1, self.cleartext().as_bytes(), &mut out);
        segment(2, &eexec_binary(&self.private_text()), &mut out);
        segment(1, format!("\n{TRAILER}").as_bytes(), &mut out);
        out.extend_from_slice(&[0x80, 3]);
        out
    }
}

/// The Type 1 font of the corpus and of the tests across crates: `a` is
/// a 500-unit square of advance 600, `e` and `acute` compose `eacute`
/// through `seac`, and `b` ends mid-operator.
pub fn corpus_type1() -> Type1Font {
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
    let malformed = CharstringBuilder::new().hsbw(0, 400).num(1).bytes();
    Type1Font::new("Syn")
        .bbox([0, 0, 750, 750])
        .glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0))
        .charstring("b", malformed)
        .charstring("e", e)
        .charstring("acute", acute)
        .charstring("eacute", eacute)
        .encode(97, "a")
        .encode(98, "b")
        .encode(101, "e")
        .encode(233, "eacute")
}

/// The TrueType font of the corpus and of the tests across crates: 2048
/// units per em, `a` a square of advance 1024, `o` one quadratic contour
/// of advance 1200.
pub fn corpus_truetype() -> TrueTypeFont {
    TrueTypeFont::new(2048)
        .glyph(
            "a",
            1024,
            vec![vec![
                (0, 0, true),
                (1000, 0, true),
                (1000, 1000, true),
                (0, 1000, true),
            ]],
        )
        .glyph(
            "o",
            1200,
            vec![vec![
                (100, 500, true),
                (600, 1000, false),
                (1100, 500, true),
                (600, 0, false),
            ]],
        )
        .map(97, 1)
        .map(111, 2)
}

/// One glyph of a synthesised TrueType font: quadratic contours of
/// `(x, y, on_curve)` points in font units.
#[derive(Clone, Debug)]
pub struct TtGlyph {
    pub name: String,
    pub advance: u16,
    pub contours: Vec<Vec<(i16, i16, bool)>>,
}

/// A TrueType font built from quadratic contours.
#[derive(Clone, Debug)]
pub struct TrueTypeFont {
    pub units_per_em: u16,
    /// Glyph 0 first.
    pub glyphs: Vec<TtGlyph>,
    /// `(code, glyph index)` entries of the `(3,0)` cmap subtable.
    pub cmap: Vec<(u16, u16)>,
    /// Written as a `name` table when present.
    pub family: Option<String>,
}

impl TrueTypeFont {
    /// A font with an empty `.notdef` of half an em.
    pub fn new(units_per_em: u16) -> Self {
        TrueTypeFont {
            units_per_em,
            glyphs: vec![TtGlyph {
                name: ".notdef".to_string(),
                advance: units_per_em / 2,
                contours: Vec::new(),
            }],
            cmap: Vec::new(),
            family: None,
        }
    }

    pub fn glyph(mut self, name: &str, advance: u16, contours: Vec<Vec<(i16, i16, bool)>>) -> Self {
        self.glyphs.push(TtGlyph {
            name: name.to_string(),
            advance,
            contours,
        });
        self
    }

    pub fn map(mut self, code: u16, gid: u16) -> Self {
        self.cmap.push((code, gid));
        self
    }

    pub fn family(mut self, family: &str) -> Self {
        self.family = Some(family.to_string());
        self
    }

    /// The glyph index of `name`.
    pub fn gid(&self, name: &str) -> Option<u16> {
        self.glyphs
            .iter()
            .position(|g| g.name == name)
            .map(|k| k as u16)
    }

    /// The font's bounding box in font units.
    pub fn bbox(&self) -> [i16; 4] {
        let mut bbox: Option<[i16; 4]> = None;
        for glyph in &self.glyphs {
            if glyph.contours.iter().all(Vec::is_empty) {
                continue;
            }
            let b = write::contours_bbox(&glyph.contours);
            bbox = Some(match bbox {
                None => b,
                Some([x0, y0, x1, y1]) => [x0.min(b[0]), y0.min(b[1]), x1.max(b[2]), y1.max(b[3])],
            });
        }
        bbox.unwrap_or([0; 4])
    }

    /// The tables of the program.
    pub fn tables(&self) -> Vec<Table> {
        let records: Vec<Vec<u8>> = self
            .glyphs
            .iter()
            .map(|g| write::simple_glyph(&g.contours))
            .collect();
        let (loca, glyf, long_loca) = write::loca_and_glyf(&records);
        let bbox = self.bbox();
        let metrics: Vec<(u16, i16)> = self
            .glyphs
            .iter()
            .map(|g| {
                let lsb = if g.contours.iter().all(Vec::is_empty) {
                    0
                } else {
                    write::contours_bbox(&g.contours)[0]
                };
                (g.advance, lsb)
            })
            .collect();
        let names: Vec<&[u8]> = self.glyphs.iter().map(|g| g.name.as_bytes()).collect();
        let map: BTreeMap<u32, u16> = self
            .cmap
            .iter()
            .map(|&(code, gid)| (u32::from(code), gid))
            .collect();
        let max_points = self
            .glyphs
            .iter()
            .map(|g| g.contours.iter().map(Vec::len).sum::<usize>())
            .max()
            .unwrap_or(0) as u16;
        let max_contours = self
            .glyphs
            .iter()
            .map(|g| g.contours.len())
            .max()
            .unwrap_or(0) as u16;
        let mut tables = vec![
            Table::new(
                b"head",
                write::head(&write::Head {
                    units_per_em: self.units_per_em,
                    bbox,
                    long_loca,
                }),
            ),
            Table::new(
                b"hhea",
                write::hhea(&write::Hhea {
                    ascender: bbox[3],
                    descender: bbox[1],
                    line_gap: 0,
                    advance_width_max: metrics.iter().map(|m| m.0).max().unwrap_or(0),
                    min_left_sidebearing: metrics.iter().map(|m| m.1).min().unwrap_or(0),
                    min_right_sidebearing: 0,
                    x_max_extent: bbox[2],
                    num_hmetrics: metrics.len() as u16,
                }),
            ),
            Table::new(
                b"maxp",
                write::maxp(&write::Maxp {
                    num_glyphs: self.glyphs.len() as u16,
                    max_points,
                    max_contours,
                    ..Default::default()
                }),
            ),
            Table::new(b"hmtx", write::hmtx(&metrics)),
            Table::new(b"loca", loca),
            Table::new(b"glyf", glyf),
            Table::new(b"post", write::post_format2(&names, 0.0, false)),
            Table::new(b"cmap", write::cmap(&[(3, 0, &map)])),
        ];
        if let Some(family) = &self.family {
            tables.push(Table::new(
                b"name",
                write::name(family, "Regular", &family.replace(' ', "")),
            ));
        }
        tables
    }

    /// The program bytes.
    pub fn build(&self) -> Vec<u8> {
        write::assemble(self.tables())
    }

    /// The program in pieces at table boundaries: the `sfnts` strings.
    pub fn parts(&self) -> Vec<Vec<u8>> {
        write::assemble_parts(self.tables())
    }

    /// The parsed program with the glyph names as its `CharStrings`.
    pub fn program(&self) -> Result<Program, FontError> {
        let names = self
            .glyphs
            .iter()
            .enumerate()
            .map(|(k, g)| (g.name.as_bytes().to_vec(), k as u16))
            .collect();
        Ok(Program::TrueType(
            TrueTypeProgram::parse(self.build())?.with_names(names),
        ))
    }

    /// The Type 42 font program defining the font as `font_name` with
    /// the given encoding entries; every `sfnts` string carries the
    /// conventional trailing zero byte.
    pub fn type42(&self, font_name: &str, encoding: &[(u8, &str)]) -> String {
        let bbox = self.bbox();
        let em = f32::from(self.units_per_em);
        let unit = |v: i16| {
            let s = format!("{:.4}", f32::from(v) / em);
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        let mut out = format!(
            "%!PS-TrueTypeFont\n\
             11 dict begin\n\
             /FontName /{font_name} def\n\
             /FontType 42 def\n\
             /PaintType 0 def\n\
             /FontMatrix [1 0 0 1 0 0] def\n\
             /FontBBox [{} {} {} {}] def\n\
             /Encoding 256 array\n\
             0 1 255 {{1 index exch /.notdef put}} for\n",
            unit(bbox[0]),
            unit(bbox[1]),
            unit(bbox[2]),
            unit(bbox[3]),
        );
        for (code, name) in encoding {
            out.push_str(&format!("dup {code} /{name} put\n"));
        }
        out.push_str("readonly def\n");
        out.push_str(&format!(
            "/CharStrings {} dict dup begin\n",
            self.glyphs.len()
        ));
        for (k, glyph) in self.glyphs.iter().enumerate() {
            out.push_str(&format!("/{} {k} def\n", glyph.name));
        }
        out.push_str("end readonly def\n/sfnts [\n");
        for part in self.parts() {
            let mut part = part;
            part.push(0);
            out.push('<');
            out.push_str(hex_lines(&part).trim_end());
            out.push_str(">\n");
        }
        out.push_str(&format!(
            "] def\ncurrentdict end\n/{font_name} exch definefont pop\n"
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::OutlineOp::{Close, LineTo, MoveTo};
    use crate::type1::{decrypt, decrypt_section, is_hex_section};

    #[test]
    fn number_encoding_round_trips_at_the_boundaries() {
        for v in [
            -1132,
            -1131,
            -108,
            -107,
            0,
            107,
            108,
            1131,
            1132,
            i32::MIN,
            i32::MAX,
        ] {
            let mut bytes = Vec::new();
            encode_number(v, &mut bytes);
            let mut b = CharstringBuilder::new().num(v).bytes();
            assert_eq!(bytes, b);
            b.push(14);
            let program = Type1Program::from_decrypted(
                4,
                Vec::new(),
                [(
                    b"n".to_vec(),
                    CharstringBuilder::new().num(v).num(v).op(13).op(14).bytes(),
                )]
                .into_iter()
                .collect(),
            );
            assert_eq!(program.glyph(b"n").unwrap().unwrap().advance.0, v as f32);
        }
    }

    #[test]
    fn the_type1_builder_yields_a_program_and_a_font_file() {
        let font = Type1Font::new("Syn")
            .glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0))
            .encode(97, "a");
        let program = font.program();
        let a = program.glyph(b"a").unwrap().unwrap();
        assert_eq!(a.advance, (600.0, 0.0));
        assert_eq!(
            a.outline.ops,
            vec![
                MoveTo(50.0, 0.0),
                LineTo(550.0, 0.0),
                LineTo(550.0, 500.0),
                LineTo(50.0, 500.0),
                Close
            ]
        );
        assert_eq!(program.glyph_count(), 2);
        assert_eq!(program.glyph_names(), vec![&b".notdef"[..], b"a"]);
        let Program::Type1(type1) = &program else {
            unreachable!()
        };
        assert_eq!(type1.dict().font_bbox, [0.0, 0.0, 1000.0, 1000.0]);
        assert_eq!(type1.dict().private_number("password"), Some(5839.0));
        assert!(program.has_glyph(b"a"));
        assert_eq!(program.units_per_em(), None);

        let pfa = font.pfa();
        assert!(pfa.starts_with("%!PS-AdobeFont-1.0: Syn\n"));
        assert!(pfa.contains("dup 97 /a put\n"));
        assert!(pfa.ends_with(TRAILER));
        let section = &pfa[pfa.find("eexec\n").unwrap() + 6..pfa.find(TRAILER).unwrap()];
        assert!(section.lines().all(|l| l.len() <= 64));
        let plain = decrypt_section(section.as_bytes());
        assert_eq!(plain, font.private_text());
        let text = String::from_utf8_lossy(&plain);
        assert!(text.starts_with("dup /Private 8 dict dup begin\n"));
        assert!(text.contains("noaccess put\ndup /CharStrings 2 dict dup begin\n"));
        assert!(text.ends_with("mark currentfile closefile\n"));

        let binary = font.pfa_binary();
        let start = font.cleartext().len();
        let end = binary.len() - TRAILER.len() - 1;
        assert!(!is_hex_section(&binary[start..start + 4]));
        assert_eq!(decrypt_section(&binary[start..end]), font.private_text());

        let with_subrs = font.clone().standard_subrs();
        let text = String::from_utf8_lossy(&with_subrs.private_text()).into_owned();
        assert!(text.contains("/Subrs 4 array\ndup 0 "));
        let cipher = CharstringBuilder::new().endchar().encrypted(4);
        assert_eq!(decrypt(CHARSTRING_KEY, &cipher, 4), vec![14]);
    }

    #[test]
    fn the_truetype_builder_round_trips_through_the_parser() {
        let font = TrueTypeFont::new(2048)
            .glyph(
                "a",
                1024,
                vec![vec![
                    (0, 0, true),
                    (1000, 0, true),
                    (1000, 1000, true),
                    (0, 1000, true),
                ]],
            )
            .glyph(
                "o",
                1200,
                vec![vec![
                    (100, 500, true),
                    (600, 1000, false),
                    (1100, 500, true),
                    (600, 0, false),
                ]],
            )
            .map(97, 1)
            .map(111, 2)
            .family("Syn TT");
        let program = font.program().unwrap();
        assert_eq!(program.units_per_em(), Some(2048));
        assert_eq!(program.glyph_count(), 3);
        let a = program.glyph(b"a").unwrap().unwrap();
        assert_eq!(a.advance, (1024.0, 0.0));
        assert_eq!(a.outline.ops.len(), 6);
        assert_eq!(a.outline.control_box(), Some([0.0, 0.0, 1000.0, 1000.0]));
        let o = program.glyph(b"o").unwrap().unwrap();
        assert_eq!(o.advance, (1200.0, 0.0));
        let [x0, y0, x1, y1] = o.outline.control_box().unwrap();
        assert_eq!((x0, x1), (100.0, 1100.0));
        assert!((y0 - 500.0 / 3.0).abs() < 0.01 && (y1 - 2500.0 / 3.0).abs() < 0.01);
        let notdef = program.glyph(b".notdef").unwrap().unwrap();
        assert!(notdef.outline.is_empty());
        assert_eq!(notdef.advance, (1024.0, 0.0));
        assert!(program.glyph(b"zz").unwrap().is_none());
        let Program::TrueType(tt) = &program else {
            unreachable!()
        };
        assert_eq!(tt.bbox(), [0, 0, 1100, 1000]);
        assert_eq!(tt.post_name(2), Some(&b"o"[..]));
        assert_eq!(tt.cmap(3, 0).unwrap().unwrap().get(&111), Some(&2));
        assert_eq!(tt.table_tags().len(), 9);
        assert!(tt.table(b"name").is_some());
        assert_eq!(tt.advance(1).unwrap(), 1024);
        assert_eq!(tt.left_sidebearing(2).unwrap(), 100);
        assert_eq!(tt.advance(9), Err(FontError::GlyphIndex(9)));
        assert_eq!(tt.components(2).unwrap(), Vec::<u16>::new());
        assert_eq!(font.gid("o"), Some(2));

        let text = font.type42("SynTT", &[(97, "a"), (111, "o")]);
        assert!(text.contains("/FontType 42 def\n"));
        assert!(text.contains("/FontBBox [0 0 0.5371 0.4883] def\n"));
        assert!(text.contains("/o 2 def\n"));
        assert!(text.contains("dup 111 /o put\n"));
        assert_eq!(text.matches('<').count(), font.parts().len());
        let parts = font.parts();
        assert_eq!(parts.concat(), font.build());
    }
}
