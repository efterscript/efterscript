// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Synthesised fonts for tests in several crates: a charstring encoder, a
//! Type 1 program builder that yields both an in-memory [`Program`] and a
//! complete `eexec` font program, a TrueType builder that yields the
//! program bytes and its Type 42 wrapper, and a CFF builder with a Type 2
//! encoder that yields the program bytes and a FontSet resource file.
//! Not compiled out of release builds, since the corpus generator and
//! other crates' tests use it.

use std::collections::BTreeMap;

use crate::outline::{Outline, OutlineOp};
use crate::program::{FontError, Program};
use crate::truetype::TrueTypeProgram;
use crate::truetype::write::{self, Table};
use crate::type1::write::encrypt_section_binary;
use crate::type1::{CHARSTRING_KEY, EEXEC_KEY, Type1Dict, Type1Program, encrypt};

pub use crate::cff::charstring::encode_number as encode_type2_number;
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

// --- CFF -----------------------------------------------------------------------

/// A Type 2 charstring assembled operator by operator.
#[derive(Clone, Debug, Default)]
pub struct Type2Builder {
    bytes: Vec<u8>,
}

impl Type2Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn num(mut self, v: i32) -> Self {
        encode_type2_number(v, &mut self.bytes);
        self
    }

    /// A number in the 16.16 fixed-point form.
    pub fn fixed(mut self, v: f32) -> Self {
        crate::cff::charstring::encode_fixed(v, &mut self.bytes);
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

    fn nums(mut self, values: &[i32]) -> Self {
        for &v in values {
            self = self.num(v);
        }
        self
    }

    pub fn hstem(self, y: i32, dy: i32) -> Self {
        self.nums(&[y, dy]).op(1)
    }

    pub fn vstem(self, x: i32, dx: i32) -> Self {
        self.nums(&[x, dx]).op(3)
    }

    pub fn hstemhm(self, y: i32, dy: i32) -> Self {
        self.nums(&[y, dy]).op(18)
    }

    pub fn vstemhm(self, x: i32, dx: i32) -> Self {
        self.nums(&[x, dx]).op(23)
    }

    /// `hintmask` with its mask bytes, which the caller sizes to the
    /// stems declared so far.
    pub fn hintmask(mut self, mask: &[u8]) -> Self {
        self.bytes.push(19);
        self.bytes.extend_from_slice(mask);
        self
    }

    pub fn cntrmask(mut self, mask: &[u8]) -> Self {
        self.bytes.push(20);
        self.bytes.extend_from_slice(mask);
        self
    }

    pub fn rmoveto(self, dx: i32, dy: i32) -> Self {
        self.nums(&[dx, dy]).op(21)
    }

    pub fn hmoveto(self, dx: i32) -> Self {
        self.num(dx).op(22)
    }

    pub fn vmoveto(self, dy: i32) -> Self {
        self.num(dy).op(4)
    }

    pub fn rlineto(self, dx: i32, dy: i32) -> Self {
        self.nums(&[dx, dy]).op(5)
    }

    pub fn hlineto(self, dx: i32) -> Self {
        self.num(dx).op(6)
    }

    pub fn vlineto(self, dy: i32) -> Self {
        self.num(dy).op(7)
    }

    pub fn rrcurveto(self, a: i32, b: i32, c: i32, d: i32, e: i32, f: i32) -> Self {
        self.nums(&[a, b, c, d, e, f]).op(8)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hflex(
        self,
        dx1: i32,
        dx2: i32,
        dy2: i32,
        dx3: i32,
        dx4: i32,
        dx5: i32,
        dx6: i32,
    ) -> Self {
        self.nums(&[dx1, dx2, dy2, dx3, dx4, dx5, dx6]).esc(34)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn flex(
        self,
        dx1: i32,
        dy1: i32,
        dx2: i32,
        dy2: i32,
        dx3: i32,
        dy3: i32,
        dx4: i32,
        dy4: i32,
        dx5: i32,
        dy5: i32,
        dx6: i32,
        dy6: i32,
        fd: i32,
    ) -> Self {
        self.nums(&[
            dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5, dx6, dy6, fd,
        ])
        .esc(35)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hflex1(
        self,
        dx1: i32,
        dy1: i32,
        dx2: i32,
        dy2: i32,
        dx3: i32,
        dx4: i32,
        dx5: i32,
        dy5: i32,
        dx6: i32,
    ) -> Self {
        self.nums(&[dx1, dy1, dx2, dy2, dx3, dx4, dx5, dy5, dx6])
            .esc(36)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn flex1(
        self,
        dx1: i32,
        dy1: i32,
        dx2: i32,
        dy2: i32,
        dx3: i32,
        dy3: i32,
        dx4: i32,
        dy4: i32,
        dx5: i32,
        dy5: i32,
        d6: i32,
    ) -> Self {
        self.nums(&[dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5, d6])
            .esc(37)
    }

    /// `callsubr` with the biased operand as written.
    pub fn callsubr(self, biased: i32) -> Self {
        self.num(biased).op(10)
    }

    pub fn callgsubr(self, biased: i32) -> Self {
        self.num(biased).op(29)
    }

    pub fn r#return(self) -> Self {
        self.op(11)
    }

    pub fn endchar(self) -> Self {
        self.op(14)
    }

    pub fn bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// A Type 2 charstring drawing `outline` (integral coordinates) with
/// advance `wx`: the width relative to `nominal` when it is not `default`,
/// then relative moves, lines, and curves; subpaths close implicitly.
pub fn charstring_type2(wx: i32, default: i32, nominal: i32, outline: &Outline) -> Vec<u8> {
    let mut b = Type2Builder::new();
    if wx != default {
        b = b.num(wx - nominal);
    }
    let (mut x, mut y) = (0, 0);
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
            OutlineOp::Close => {}
        }
    }
    b.endchar().bytes()
}

/// One font dictionary of a CID-keyed font: its private data.
#[derive(Clone, Debug, Default)]
pub struct CffFd {
    pub subrs: Vec<Vec<u8>>,
    pub default_width: i32,
    pub nominal_width: i32,
}

/// The CID-keyed layout of a [`CffFont`].
#[derive(Clone, Debug)]
pub struct CidLayout {
    pub registry: String,
    pub ordering: String,
    pub supplement: i32,
    pub fds: Vec<CffFd>,
    /// `(cid, font dictionary index)` per glyph, glyph 0 first.
    pub glyphs: Vec<(u16, u8)>,
}

/// A CFF font built from plain Type 2 charstrings, name-keyed unless
/// [`CffFont::cid_keyed`] made it CID-keyed.
#[derive(Clone, Debug)]
pub struct CffFont {
    pub name: String,
    pub bbox: [i32; 4],
    /// Written to the top dictionary when set; the default otherwise.
    pub font_matrix: Option<[f64; 6]>,
    /// Plain charstrings by name, glyph 0 (`.notdef`) first.
    pub glyphs: Vec<(String, Vec<u8>)>,
    pub subrs: Vec<Vec<u8>>,
    pub gsubrs: Vec<Vec<u8>>,
    pub default_width: i32,
    pub nominal_width: i32,
    pub std_vw: Option<i32>,
    pub notice: Option<String>,
    /// A custom encoding; empty means the standard encoding.
    pub encoding: Vec<(u8, String)>,
    pub cid: Option<CidLayout>,
}

impl CffFont {
    /// A name-keyed font with a `.notdef` of the default width.
    pub fn new(name: &str) -> Self {
        CffFont {
            name: name.to_string(),
            bbox: [0, 0, 1000, 1000],
            font_matrix: None,
            glyphs: vec![(".notdef".to_string(), Type2Builder::new().endchar().bytes())],
            subrs: Vec::new(),
            gsubrs: Vec::new(),
            default_width: 0,
            nominal_width: 0,
            std_vw: None,
            notice: None,
            encoding: Vec::new(),
            cid: None,
        }
    }

    /// A CID-keyed font with the given registry, ordering, and
    /// supplement, no font dictionaries yet, and a `.notdef` at CID 0 in
    /// font dictionary 0.
    pub fn cid_keyed(name: &str, registry: &str, ordering: &str, supplement: i32) -> Self {
        let mut font = CffFont::new(name);
        font.cid = Some(CidLayout {
            registry: registry.to_string(),
            ordering: ordering.to_string(),
            supplement,
            fds: Vec::new(),
            glyphs: vec![(0, 0)],
        });
        font
    }

    pub fn bbox(mut self, bbox: [i32; 4]) -> Self {
        self.bbox = bbox;
        self
    }

    pub fn font_matrix(mut self, matrix: [f64; 6]) -> Self {
        self.font_matrix = Some(matrix);
        self
    }

    /// The default and nominal widths of the private dictionary.
    pub fn widths(mut self, default: i32, nominal: i32) -> Self {
        self.default_width = default;
        self.nominal_width = nominal;
        self
    }

    pub fn std_vw(mut self, width: i32) -> Self {
        self.std_vw = Some(width);
        self
    }

    pub fn notice(mut self, notice: &str) -> Self {
        self.notice = Some(notice.to_string());
        self
    }

    /// A glyph drawn from an outline, advance `wx`.
    pub fn glyph(self, name: &str, wx: i32, outline: &Outline) -> Self {
        let code = charstring_type2(wx, self.default_width, self.nominal_width, outline);
        self.charstring(name, code)
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

    pub fn gsubr(mut self, code: Vec<u8>) -> Self {
        self.gsubrs.push(code);
        self
    }

    pub fn encode(mut self, code: u8, name: &str) -> Self {
        self.encoding.push((code, name.to_string()));
        self
    }

    /// Adds a font dictionary to a CID-keyed font.
    pub fn fd(mut self, fd: CffFd) -> Self {
        self.cid.as_mut().expect("a CID-keyed font").fds.push(fd);
        self
    }

    /// A glyph of a CID-keyed font at `cid`, run under font dictionary
    /// `fd`.
    pub fn cid_glyph(mut self, cid: u16, fd: u8, code: Vec<u8>) -> Self {
        self.cid
            .as_mut()
            .expect("a CID-keyed font")
            .glyphs
            .push((cid, fd));
        self.glyphs.push((format!("cid{cid}"), code));
        self
    }

    /// The glyph index of `name`.
    pub fn gid(&self, name: &str) -> Option<u16> {
        self.glyphs
            .iter()
            .position(|(n, _)| n == name)
            .map(|k| k as u16)
    }

    /// The program bytes.
    pub fn build(&self) -> Vec<u8> {
        use crate::cff::{STANDARD_STRINGS, op, write};

        // String ids: standard strings by position, the font's own after
        // them in order of first use.
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut sid = |text: &str| -> u16 {
            if let Some(k) = STANDARD_STRINGS.iter().position(|s| *s == text) {
                return k as u16;
            }
            let bytes = text.as_bytes().to_vec();
            let k = match strings.iter().position(|s| *s == bytes) {
                Some(k) => k,
                None => {
                    strings.push(bytes);
                    strings.len() - 1
                }
            };
            (STANDARD_STRINGS.len() + k) as u16
        };
        let notice_sid = self.notice.as_deref().map(&mut sid);
        let ros = self
            .cid
            .as_ref()
            .map(|cid| (sid(&cid.registry), sid(&cid.ordering), cid.supplement));
        let charset_ids: Vec<u16> = match &self.cid {
            Some(cid) => cid.glyphs.iter().map(|&(c, _)| c).collect(),
            None => self.glyphs.iter().map(|(name, _)| sid(name)).collect(),
        };
        let encoding_codes: Vec<(u8, u16)> = self
            .encoding
            .iter()
            .map(|(code, name)| (*code, sid(name)))
            .collect();

        let private = |subrs: &[Vec<u8>], default: i32, nominal: i32| -> (Vec<u8>, Vec<u8>) {
            let mut w = write::DictWriter::new();
            if let Some(std_vw) = self.std_vw {
                w.entry(op::STD_VW, &[f64::from(std_vw)]);
            }
            w.entry(op::DEFAULT_WIDTH_X, &[f64::from(default)]);
            w.entry(op::NOMINAL_WIDTH_X, &[f64::from(nominal)]);
            if subrs.is_empty() {
                return (w.bytes, Vec::new());
            }
            let len = w.len() + 6;
            w.fixed(op::SUBRS, &[len as i32]);
            (w.bytes, write::index(subrs))
        };

        let top = |charset: i32,
                   encoding: i32,
                   charstrings: i32,
                   private: (i32, i32),
                   fd_array: i32,
                   fd_select: i32|
         -> Vec<u8> {
            let mut w = write::DictWriter::new();
            if let Some((r, o, s)) = ros {
                w.entry(op::ROS, &[f64::from(r), f64::from(o), f64::from(s)]);
            }
            if let Some(n) = notice_sid {
                w.entry(op::NOTICE, &[f64::from(n)]);
            }
            w.entry(op::FONT_BBOX, &self.bbox.map(f64::from));
            if let Some(m) = self.font_matrix {
                w.entry(op::FONT_MATRIX, &m);
            }
            w.fixed(op::CHARSET, &[charset]);
            if !encoding_codes.is_empty() && ros.is_none() {
                w.fixed(op::ENCODING, &[encoding]);
            }
            w.fixed(op::CHARSTRINGS, &[charstrings]);
            match &self.cid {
                Some(cid) => {
                    w.entry(
                        op::CID_COUNT,
                        &[f64::from(cid.glyphs.iter().map(|g| g.0).max().unwrap_or(0)) + 1.0],
                    );
                    w.fixed(op::FD_ARRAY, &[fd_array]);
                    w.fixed(op::FD_SELECT, &[fd_select]);
                }
                None => {
                    w.fixed(op::PRIVATE, &[private.0, private.1]);
                }
            }
            w.bytes
        };

        let charstrings: Vec<Vec<u8>> = self.glyphs.iter().map(|(_, c)| c.clone()).collect();
        let charset = write::charset_format0(&charset_ids);
        let encoding = write::encoding_supplements(&encoding_codes);
        let fd_select = self.cid.as_ref().map(|cid| {
            write::fd_select_format3(&cid.glyphs.iter().map(|g| g.1).collect::<Vec<_>>())
        });

        // First pass: sizes with placeholder offsets.
        let mut header = write::header(4).to_vec();
        header.extend(write::index(&[self.name.as_bytes().to_vec()]));
        let top_size = write::index(&[top(0, 0, 0, (0, 0), 0, 0)]).len();
        let strings_index = write::index(&strings);
        let gsubrs_index = write::index(&self.gsubrs);
        let charstrings_index = write::index(&charstrings);

        let mut at = header.len() + top_size + strings_index.len() + gsubrs_index.len();
        let charset_at = at;
        at += charset.len();
        let encoding_at = at;
        if !encoding_codes.is_empty() && self.cid.is_none() {
            at += encoding.len();
        }
        let fd_select_at = at;
        if let Some(select) = &fd_select {
            at += select.len();
        }
        let charstrings_at = at;
        at += charstrings_index.len();

        let mut tail = Vec::new();
        let mut private_entry = (0, 0);
        let mut fd_array_at = 0;
        match &self.cid {
            None => {
                let (dict, subrs) = private(&self.subrs, self.default_width, self.nominal_width);
                private_entry = (dict.len() as i32, at as i32);
                tail.extend(dict);
                tail.extend(subrs);
            }
            Some(cid) => {
                // Font dictionaries carry a fixed-size private entry, so
                // the array's size is known before its privates are laid
                // out after it.
                let fd_dict = |size: i32, offset: i32| {
                    let mut w = write::DictWriter::new();
                    w.fixed(op::PRIVATE, &[size, offset]);
                    w.bytes
                };
                let fds: Vec<Vec<u8>> = cid.fds.iter().map(|_| fd_dict(0, 0)).collect();
                fd_array_at = at;
                let array_size = write::index(&fds).len();
                let mut privates = Vec::new();
                let mut dicts = Vec::new();
                let mut offset = at + array_size;
                for fd in &cid.fds {
                    let (dict, subrs) = private(&fd.subrs, fd.default_width, fd.nominal_width);
                    dicts.push(fd_dict(dict.len() as i32, offset as i32));
                    offset += dict.len() + subrs.len();
                    privates.extend(dict);
                    privates.extend(subrs);
                }
                tail.extend(write::index(&dicts));
                tail.extend(privates);
            }
        }

        let top = top(
            charset_at as i32,
            encoding_at as i32,
            charstrings_at as i32,
            private_entry,
            fd_array_at as i32,
            fd_select_at as i32,
        );
        let mut out = header;
        out.extend(write::index(&[top]));
        out.extend(strings_index);
        out.extend(gsubrs_index);
        out.extend(charset);
        if !encoding_codes.is_empty() && self.cid.is_none() {
            out.extend(encoding);
        }
        if let Some(select) = fd_select {
            out.extend(select);
        }
        out.extend(charstrings_index);
        out.extend(tail);
        out
    }

    /// The parsed program.
    pub fn program(&self) -> Result<Program, FontError> {
        Ok(Program::Cff(crate::cff::CffProgram::parse(&self.build())?))
    }

    /// The font as a FontSet resource file: the `FontSetInit` procedure
    /// set, `StartData` with the byte count, the binary program, and
    /// `end`.
    pub fn font_set(&self, set_name: &str) -> Vec<u8> {
        let data = self.build();
        let mut out = format!(
            "/FontSetInit /ProcSet findresource begin\n/{set_name} {} StartData\n",
            data.len()
        )
        .into_bytes();
        out.extend(data);
        out.extend_from_slice(b"\nend\n");
        out
    }
}

/// The CFF font of the corpus and of the tests across crates: nominal
/// width 500, `a` a 500-unit square of advance 600 (a width delta of
/// 100), `b` and `c` reaching subroutines, `f` a glyph with two stem
/// hints, a hint mask, and an `hflex` whose control box is 0..600 by
/// -50..200; the standard encoding.
pub fn corpus_cff() -> CffFont {
    let a = charstring_type2(600, 0, 500, &rectangle(50.0, 0.0, 550.0, 500.0));
    // Local subroutine 0 draws a bar, global 0 a diagonal; local 1 is
    // unreached by any glyph, so a subset can drop it.
    let bar = Type2Builder::new()
        .rlineto(300, 0)
        .rlineto(0, 100)
        .r#return()
        .bytes();
    let unused = Type2Builder::new().rlineto(1, 1).r#return().bytes();
    let diagonal = Type2Builder::new().rlineto(200, 200).r#return().bytes();
    let b = Type2Builder::new()
        .num(-100)
        .rmoveto(0, 0)
        .callsubr(-107)
        .callgsubr(-107)
        .endchar()
        .bytes();
    let c = Type2Builder::new()
        .num(0)
        .rmoveto(50, 50)
        .callgsubr(-107)
        .rlineto(-200, 0)
        .endchar()
        .bytes();
    let f = Type2Builder::new()
        .num(100)
        .hstemhm(0, 100)
        .num(0)
        .num(100)
        .hintmask(&[0b1100_0000])
        .rmoveto(0, 0)
        .hflex(100, 100, 200, 100, 100, 100, 100)
        .rlineto(0, -50)
        .rlineto(-600, 0)
        .endchar()
        .bytes();
    CffFont::new("SynCFF")
        .bbox([0, -50, 600, 500])
        .widths(0, 500)
        .std_vw(80)
        .subr(bar)
        .subr(unused)
        .gsubr(diagonal)
        .charstring("a", a)
        .charstring("b", b)
        .charstring("c", c)
        .charstring("f", f)
}
