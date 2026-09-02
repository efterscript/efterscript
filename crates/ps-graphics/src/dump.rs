// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The canonical text form of a page, version `ir/1`.
//!
//! One page dumps as:
//!
//! ```text
//! ir/1
//! page <width> <height>
//! origin <llx> <lly>                  only when the media box origin is not 0 0
//! resources:
//! cs <n> <space>                      one per colour space, in index order
//! img <n> <w>x<h> bpc=<b> cs=<n>|mask decode=[<d>…] <len> bytes [interpolate]
//! font <n> <BaseName> [diff=[<code> /<name>…]]
//! font <n> type3 <a> <b> <c> <d> <tx> <ty> bbox=[<llx> <lly> <urx> <ury>] enc=[<code> /<name>…]
//! font <n> embedded <type1|truetype> <FontName> glyphs=<count> enc=[<code> /<name>…]
//! glyph /<name> <wx> <wy> [<llx> <lly> <urx> <ury>] {
//!   <op>                              the glyph's procedure, indented
//! }
//! ops:
//! <op>                                one per line, in page order
//! ```
//!
//! A resident font lists only the codes whose glyph differs from the
//! base font's built-in encoding (`/.notdef` where the program removed
//! one); a Type 3 font lists the codes of the glyphs it captured, then
//! one `glyph` block per captured glyph in name order, the box present
//! for a `setcachedevice` glyph. An embedded font has no built-in
//! encoding to differ from, so it lists every code that names a glyph,
//! with the kind of program, its `FontName`, and how many glyphs it
//! defines; the program's bytes never appear.
//!
//! Colour spaces are described by family: `DeviceGray`, `DeviceRGB`,
//! `DeviceCMYK`, `Separation (<name>) alt=<space> tint=<len> bytes`,
//! `DeviceN (<name>) (<name>)… alt=<space> tint=<len> bytes`, and
//! `Indexed base=<space> hival=<n> lookup=<len> bytes`, with nested
//! spaces written inline. Names use PostScript string escapes. Sample
//! data, lookup tables, and tint procedures appear as byte counts only.
//!
//! Operations use PDF's operator names where one exists:
//!
//! ```text
//! q  Q                                save / restore around clips
//! w <n>  J <0-2>  j <0-2>  M <n>  d [<n>…] <phase>  i <n>
//! cs <n>                              colour space by resource index
//! sc <n>…                             components
//! m <x> <y>  l <x> <y>  c <x1> <y1> <x2> <y2> <x3> <y3>  h
//! f  f*  S                            paint the segments just listed
//! W n  W* n                           clip to the segments just listed
//! stroke-ctm <a> <b> <c> <d> <tx> <ty>   before a stroke's path, only
//!                                     when the CTM at the stroke is not
//!                                     the identity
//! Do img <n> <a> <b> <c> <d> <tx> <ty>   image with its unit-square matrix
//! text <n> <a> <b> <c> <d> <tx> <ty> (<bytes>) <dx> <dy>…
//!                                     a glyph run: font, the matrix from
//!                                     glyph space to default user space
//!                                     at the first glyph, the codes, and
//!                                     each glyph's displacement in glyph
//!                                     space
//! ```
//!
//! Numbers use the project's canonical real syntax ([`crate::fmt_real`]).
//! Every line ends in a newline; the dump of a page is a pure function of
//! its value.

use ps_fonts::ProgramKind;
use ps_vm::{Bounds, ImageSpec, Matrix, Seg, SpaceSpec};

use crate::ir::{FillRule, FontSpec, GlyphNames, GlyphProc, Image, IrOp, Op, Page};
use crate::real::{fmt_real, fmt_reals};

pub const VERSION: &str = "ir/1";

fn ps_string(bytes: &[u8]) -> String {
    let mut out = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            b' '..=b'~' => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// A name's bytes with those that would end or confuse a token written
/// as octal escapes.
fn name_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'!'..=b'~'
                if !matches!(
                    b,
                    b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'\\'
                ) =>
            {
                out.push(b as char)
            }
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

/// A glyph name as `/name`.
fn ps_name(bytes: &[u8]) -> String {
    format!("/{}", name_text(bytes))
}

/// `code /name` pairs for the codes of `encoding` that `differs` selects.
fn code_names(encoding: &GlyphNames, differs: impl Fn(u8, Option<&[u8]>) -> bool) -> String {
    let mut parts = Vec::new();
    for (code, name) in encoding.iter().enumerate() {
        let code = code as u8;
        let name = name.as_deref();
        if differs(code, name) {
            parts.push(format!("{code} {}", ps_name(name.unwrap_or(b".notdef"))));
        }
    }
    parts.join(" ")
}

fn glyph(name: &[u8], proc_: &GlyphProc) -> String {
    let mut line = format!(
        "glyph {} {}",
        ps_name(name),
        fmt_reals(&[proc_.width.0, proc_.width.1])
    );
    if let Some(b) = proc_.bbox {
        line.push_str(&format!(" [{}]", fmt_reals(&[b.llx, b.lly, b.urx, b.ury])));
    }
    line.push_str(" {\n");
    let mut body = String::new();
    ops(&mut body, &proc_.ops);
    for inner in body.lines() {
        line.push_str("  ");
        line.push_str(inner);
        line.push('\n');
    }
    line.push_str("}\n");
    line
}

/// The `font` line of a resource and, for a Type 3 font, its glyph
/// blocks; every line ends in a newline.
fn font(index: usize, spec: &FontSpec) -> String {
    match spec {
        FontSpec::Resident { base, encoding } => {
            let builtin = base.builtin_encoding();
            let diff = code_names(encoding, |code, name| {
                name != builtin[usize::from(code)].map(str::as_bytes)
            });
            let mut line = format!("font {index} {}", base.postscript_name());
            if !diff.is_empty() {
                line.push_str(&format!(" diff=[{diff}]"));
            }
            line.push('\n');
            line
        }
        FontSpec::Type3 {
            font_matrix,
            font_bbox: b,
            encoding,
            glyphs,
        } => {
            let enc = code_names(encoding, |_, name| {
                name.is_some_and(|name| glyphs.contains_key(name))
            });
            let mut out = format!(
                "font {index} type3 {} bbox=[{}] enc=[{enc}]\n",
                matrix(*font_matrix),
                fmt_reals(&[b.llx, b.lly, b.urx, b.ury])
            );
            for (name, proc_) in glyphs {
                out.push_str(&glyph(name, proc_));
            }
            out
        }
        FontSpec::Embedded {
            kind,
            font_name,
            program,
            encoding,
            ..
        } => {
            let kind = match kind {
                ProgramKind::Type1 => "type1",
                ProgramKind::TrueType => "truetype",
            };
            let enc = code_names(encoding, |_, name| name.is_some());
            format!(
                "font {index} embedded {kind} {} glyphs={} enc=[{enc}]\n",
                name_text(font_name),
                program.glyph_count()
            )
        }
    }
}

fn space(spec: &SpaceSpec) -> String {
    match spec {
        SpaceSpec::DeviceGray | SpaceSpec::DeviceRGB | SpaceSpec::DeviceCMYK => {
            spec.family().to_string()
        }
        SpaceSpec::Separation {
            name,
            alternate,
            tint_source,
        } => format!(
            "Separation {} alt={} tint={} bytes",
            ps_string(name),
            space(alternate),
            tint_source.len()
        ),
        SpaceSpec::DeviceN {
            names,
            alternate,
            tint_source,
        } => {
            let names: Vec<String> = names.iter().map(|n| ps_string(n)).collect();
            format!(
                "DeviceN {} alt={} tint={} bytes",
                names.join(" "),
                space(alternate),
                tint_source.len()
            )
        }
        SpaceSpec::Indexed {
            base,
            hival,
            lookup,
        } => format!(
            "Indexed base={} hival={} lookup={} bytes",
            space(base),
            hival,
            lookup.len()
        ),
    }
}

fn image(index: usize, image: &Image) -> String {
    let ImageSpec {
        width,
        height,
        bits_per_component,
        decode,
        interpolate,
        ..
    } = &image.spec;
    let cs = match image.color_space {
        Some(r) => r.0.to_string(),
        None => "mask".to_string(),
    };
    let mut line = format!(
        "img {index} {width}x{height} bpc={bits_per_component} cs={cs} decode=[{}] {} bytes",
        fmt_reals(decode),
        image.data.len()
    );
    if *interpolate {
        line.push_str(" interpolate");
    }
    line
}

fn matrix(m: Matrix) -> String {
    fmt_reals(&m.0)
}

fn segments(out: &mut String, path: &[Seg]) {
    for seg in path {
        match *seg {
            Seg::Move(p) => out.push_str(&format!("m {}\n", fmt_reals(&[p.x, p.y]))),
            Seg::Line(p) => out.push_str(&format!("l {}\n", fmt_reals(&[p.x, p.y]))),
            Seg::Curve(a, b, c) => out.push_str(&format!(
                "c {}\n",
                fmt_reals(&[a.x, a.y, b.x, b.y, c.x, c.y])
            )),
            Seg::Close => out.push_str("h\n"),
        }
    }
}

fn op(out: &mut String, op: &IrOp) {
    match op {
        IrOp::Save => out.push_str("q\n"),
        IrOp::Restore => out.push_str("Q\n"),
        IrOp::LineWidth(w) => out.push_str(&format!("w {}\n", fmt_real(*w))),
        IrOp::LineCap(cap) => out.push_str(&format!("J {}\n", cap.code())),
        IrOp::LineJoin(join) => out.push_str(&format!("j {}\n", join.code())),
        IrOp::MiterLimit(m) => out.push_str(&format!("M {}\n", fmt_real(*m))),
        IrOp::Dash(lengths, phase) => out.push_str(&format!(
            "d [{}] {}\n",
            fmt_reals(lengths),
            fmt_real(*phase)
        )),
        IrOp::Flatness(f) => out.push_str(&format!("i {}\n", fmt_real(*f))),
        IrOp::SetColorSpace(r) => out.push_str(&format!("cs {}\n", r.0)),
        IrOp::SetColor(c) => out.push_str(&format!("sc {}\n", fmt_reals(c))),
        IrOp::Fill { path, rule } => {
            segments(out, path);
            out.push_str(match rule {
                FillRule::NonZero => "f\n",
                FillRule::EvenOdd => "f*\n",
            });
        }
        IrOp::Stroke { path, ctm } => {
            if *ctm != Matrix::IDENTITY {
                out.push_str(&format!("stroke-ctm {}\n", matrix(*ctm)));
            }
            segments(out, path);
            out.push_str("S\n");
        }
        IrOp::Clip { path, rule } => {
            segments(out, path);
            out.push_str(match rule {
                FillRule::NonZero => "W n\n",
                FillRule::EvenOdd => "W* n\n",
            });
        }
        IrOp::Image {
            image: r,
            matrix: m,
        } => out.push_str(&format!("Do img {} {}\n", r.0, matrix(*m))),
        IrOp::Text {
            font,
            matrix: m,
            glyphs,
        } => {
            let codes: Vec<u8> = glyphs.iter().map(|g| g.code).collect();
            let displacements: Vec<f32> = glyphs.iter().flat_map(|g| [g.dx, g.dy]).collect();
            out.push_str(&format!(
                "text {} {} {} {}\n",
                font.0,
                matrix(*m),
                ps_string(&codes),
                fmt_reals(&displacements)
            ));
        }
    }
}

fn ops(out: &mut String, ops: &[Op]) {
    for entry in ops {
        op(out, &entry.op);
    }
}

/// The dump of one page.
pub fn page(page: &Page) -> String {
    let Bounds { llx, lly, urx, ury } = page.media_box;
    let mut out = format!("{VERSION}\npage {}\n", fmt_reals(&[urx - llx, ury - lly]));
    if llx != 0.0 || lly != 0.0 {
        out.push_str(&format!("origin {}\n", fmt_reals(&[llx, lly])));
    }
    out.push_str("resources:\n");
    for (i, spec) in page.resources.color_spaces.iter().enumerate() {
        out.push_str(&format!("cs {i} {}\n", space(spec)));
    }
    for (i, img) in page.resources.images.iter().enumerate() {
        out.push_str(&image(i, img));
        out.push('\n');
    }
    for (i, spec) in page.resources.fonts.iter().enumerate() {
        out.push_str(&font(i, spec));
    }
    out.push_str("ops:\n");
    ops(&mut out, &page.ops);
    out
}

/// The dump of a sequence of pages, separated by blank lines.
pub fn pages(pages: &[Page]) -> String {
    pages.iter().map(Page::dump).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_escaped_like_strings() {
        assert_eq!(ps_string(b"Spot"), "(Spot)");
        assert_eq!(ps_string(b"a(b)\\"), "(a\\(b\\)\\\\)");
        assert_eq!(ps_string(b"\x01\xff"), "(\\001\\377)");
    }

    #[test]
    fn compound_spaces_describe_their_parts() {
        let sep = SpaceSpec::Separation {
            name: b"Spot".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceCMYK),
            tint_source: b"{dup 0 0 0}".to_vec(),
        };
        assert_eq!(
            space(&sep),
            "Separation (Spot) alt=DeviceCMYK tint=11 bytes"
        );
        let indexed = SpaceSpec::Indexed {
            base: Box::new(sep),
            hival: 1,
            lookup: vec![0, 1],
        };
        assert_eq!(
            space(&indexed),
            "Indexed base=Separation (Spot) alt=DeviceCMYK tint=11 bytes hival=1 lookup=2 bytes"
        );
        let n = SpaceSpec::DeviceN {
            names: vec![b"A".to_vec(), b"B".to_vec()],
            alternate: Box::new(SpaceSpec::DeviceGray),
            tint_source: Vec::new(),
        };
        assert_eq!(space(&n), "DeviceN (A) (B) alt=DeviceGray tint=0 bytes");
    }
}
