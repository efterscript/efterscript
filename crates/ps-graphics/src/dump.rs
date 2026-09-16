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
//! img <n> <w>x<h> bpc=<b> cs=<n>|mask decode=[<d>…] <len> bytes [interpolate] [dct]
//! font <n> <BaseName> [diff=[<code> /<name>…]]
//! font <n> type3 <a> <b> <c> <d> <tx> <ty> bbox=[<llx> <lly> <urx> <ury>] enc=[<code> /<name>…]
//! font <n> embedded <type1|truetype|cff> <FontName> glyphs=<count> enc=[<code> /<name>…]
//! font <n> composite <CMapName> wmode=<m> <cff|truetype|cidtype1> <FontName> glyphs=<count>
//! glyph /<name> <wx> <wy> [<llx> <lly> <urx> <ury>] {
//!   <op>                              the glyph's procedure, indented
//! }
//! pattern <n> matrix <a> <b> <c> <d> <tx> <ty> bbox <llx> <lly> <urx> <ury> step <xs> <ys> paint <1|2> tiling <1-3> {
//!   <op>                              the cell, in pattern space, indented
//! }
//! form <n> bbox <llx> <lly> <urx> <ury> {
//!   <op>                              the body, in form space, indented
//! }
//! ops:
//! <op>                                one per line, in page order
//! annot link <llx> <lly> <urx> <ury> <target> [border=<bx> <by> <w>] [color=<r> <g> <b>] [contents=(<text>)]
//! ```
//!
//! A link's target is `dest=/<name>`, `uri=(<text>)`, or `page=<n>
//! <view>`, where a view is `fit`, `fith <top>`, or `xyz <left> <top>
//! <zoom>` with `null` for a component the mark left out. A document
//! ([`document`]) is its pages separated by blank lines and then, only
//! when the run made document-level marks, a `doc:` section:
//!
//! ```text
//! doc:
//! out <count> (<title>) -> dest=/<name>|page=<n> <view>|none
//! dest /<name> -> page <n> <view>
//! info /<key> (<value>)               one line per entry
//! view [mode=/<PageMode>] [layout=/<PageLayout>] [open=<target>]
//! pages [cropbox <llx> <lly> <urx> <ury>] [rotate <n>]
//! page <n> [cropbox <llx> <lly> <urx> <ury>] [rotate <n>]
//! ignored /<kind>
//! param /<key> <value>              one line per entry, PostScript syntax
//! ```
//!
//! A pattern resource gives the matrix from pattern space to the space
//! of the content that names it, its box and steps in pattern space,
//! its paint type, and its tiling type; a form resource its box in form
//! space. Both list their captured operations indented, as a glyph
//! does.
//!
//! A resident font lists only the codes whose glyph differs from the
//! base font's built-in encoding (`/.notdef` where the program removed
//! one); a Type 3 font lists the codes of the glyphs it captured, then
//! one `glyph` block per captured glyph in name order, the box present
//! for a `setcachedevice` glyph. An embedded font has no built-in
//! encoding to differ from, so it lists every code that names a glyph,
//! with the kind of program, its `FontName`, and how many glyphs it
//! defines; the program's bytes never appear. A composite font names
//! its CMap and writing mode, then its descendant's kind, `FontName`,
//! and glyph count; the descendant is addressed by CID and has no
//! encoding to list.
//!
//! Colour spaces are described by family: `DeviceGray`, `DeviceRGB`,
//! `DeviceCMYK`, `Separation (<name>) alt=<space> tint=<len> bytes`,
//! `DeviceN (<name>) (<name>)… alt=<space> tint=<len> bytes`,
//! `Indexed base=<space> hival=<n> lookup=<len> bytes`, `CalGray
//! white=<x> <y> <z> black=<x> <y> <z> gamma=<g>`, `CalRGB white=…
//! black=… gamma=<g> <g> <g> matrix=<nine numbers>`, `Lab white=…
//! black=… range=<amin> <amax> <bmin> <bmax>`, and `Pattern` or
//! `Pattern base=<space>` for a pattern space, with nested spaces
//! written inline. Names use PostScript string escapes. Sample data,
//! lookup tables, and tint procedures appear as byte counts only.
//!
//! Operations use PDF's operator names where one exists:
//!
//! ```text
//! q  Q                                save / restore around clips
//! w <n>  J <0-2>  j <0-2>  M <n>  d [<n>…] <phase>  i <n>
//! cs <n>                              colour space by resource index
//! sc <n>…                             components
//! pattern <n> [<c>…]                  a pattern as the colour, with the
//!                                     components of an uncoloured one
//! form <n> <a> <b> <c> <d> <tx> <ty>  a form placed under its matrix
//! m <x> <y>  l <x> <y>  c <x1> <y1> <x2> <y2> <x3> <y3>  h
//! f  f*  S                            paint the segments just listed
//! W n  W* n                           clip to the segments just listed
//! stroke-ctm <a> <b> <c> <d> <tx> <ty>   before a stroke's path, only
//!                                     when the CTM at the stroke is not
//!                                     the identity
//! Do img <n> <a> <b> <c> <d> <tx> <ty>   image with its unit-square matrix
//! text <n> <a> <b> <c> <d> <tx> <ty> (<bytes>) <dx> <dy>… [wmode=1]
//!                                     a glyph run: font, the matrix from
//!                                     glyph space to default user space
//!                                     at the first glyph, the codes (as
//!                                     <hex> when any code is longer than
//!                                     one byte), each glyph's
//!                                     displacement in glyph space, and
//!                                     the writing mode when vertical
//! ```
//!
//! Numbers use the project's canonical real syntax ([`crate::fmt_real`]).
//! Every line ends in a newline; the dump of a page is a pure function of
//! its value.

use ps_fonts::ProgramKind;
use ps_vm::{Bounds, Encoded, Glyph, ImageSpec, MarkValue, Matrix, Seg, SpaceSpec};

use crate::ir::{
    Annot, DocMark, FillRule, FontSpec, FormSpec, GlyphNames, GlyphProc, Image, IrOp, LinkTarget,
    Op, Page, PageAttrs, PatternSpec, Target, View,
};
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

/// A run's code bytes in hexadecimal, the form a run with a code longer
/// than one byte takes.
fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::from("<");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out.push('>');
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

/// `header {`, the operations indented by two, and `}`.
fn block(header: &str, content: &[Op]) -> String {
    let mut out = format!("{header} {{\n");
    let mut body = String::new();
    ops(&mut body, content);
    for inner in body.lines() {
        out.push_str("  ");
        out.push_str(inner);
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn glyph(name: &[u8], proc_: &GlyphProc) -> String {
    let mut header = format!(
        "glyph {} {}",
        ps_name(name),
        fmt_reals(&[proc_.width.0, proc_.width.1])
    );
    if let Some(b) = proc_.bbox {
        header.push_str(&format!(" [{}]", fmt_reals(&[b.llx, b.lly, b.urx, b.ury])));
    }
    block(&header, &proc_.ops)
}

fn bounds(b: Bounds) -> String {
    fmt_reals(&[b.llx, b.lly, b.urx, b.ury])
}

fn pattern(index: usize, spec: &PatternSpec) -> String {
    let header = format!(
        "pattern {index} matrix {} bbox {} step {} paint {} tiling {}",
        matrix(spec.matrix),
        bounds(spec.bbox),
        fmt_reals(&[spec.xstep, spec.ystep]),
        spec.paint_type,
        spec.tiling_type
    );
    block(&header, &spec.ops)
}

fn form(index: usize, spec: &FormSpec) -> String {
    block(
        &format!("form {index} bbox {}", bounds(spec.bbox)),
        &spec.ops,
    )
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
            let enc = code_names(encoding, |_, name| name.is_some());
            format!(
                "font {index} embedded {} {} glyphs={} enc=[{enc}]\n",
                kind_text(*kind),
                name_text(font_name),
                program.glyph_count()
            )
        }
        FontSpec::Composite {
            cmap_name,
            wmode,
            descendant,
            ..
        } => {
            let (kind, font_name, glyphs) = match &**descendant {
                FontSpec::Embedded {
                    kind,
                    font_name,
                    program,
                    ..
                } => (
                    kind_text(*kind),
                    name_text(font_name),
                    program.glyph_count(),
                ),
                _ => ("unknown", String::new(), 0),
            };
            format!(
                "font {index} composite {} wmode={wmode} {kind} {font_name} glyphs={glyphs}\n",
                name_text(cmap_name)
            )
        }
    }
}

fn kind_text(kind: ProgramKind) -> &'static str {
    match kind {
        ProgramKind::Type1 => "type1",
        ProgramKind::TrueType => "truetype",
        ProgramKind::Cff => "cff",
        ProgramKind::Type1Cid => "cidtype1",
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
        SpaceSpec::CalGray {
            white,
            black,
            gamma,
        } => format!(
            "CalGray white={} black={} gamma={}",
            fmt_reals(white),
            fmt_reals(black),
            fmt_real(*gamma)
        ),
        SpaceSpec::CalRGB {
            white,
            black,
            gamma,
            matrix,
        } => format!(
            "CalRGB white={} black={} gamma={} matrix={}",
            fmt_reals(white),
            fmt_reals(black),
            fmt_reals(gamma),
            fmt_reals(matrix)
        ),
        SpaceSpec::Lab {
            white,
            black,
            range,
        } => format!(
            "Lab white={} black={} range={}",
            fmt_reals(white),
            fmt_reals(black),
            fmt_reals(range)
        ),
        SpaceSpec::Pattern { base } => match base {
            Some(base) => format!("Pattern base={}", space(base)),
            None => "Pattern".to_string(),
        },
    }
}

fn image(index: usize, image: &Image) -> String {
    let ImageSpec {
        width,
        height,
        bits_per_component,
        decode,
        interpolate,
        encoded,
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
    if let Some(Encoded::Dct) = encoded {
        line.push_str(" dct");
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
        IrOp::SetPattern {
            pattern,
            components,
        } => {
            out.push_str(&format!("pattern {}", pattern.0));
            if !components.is_empty() {
                out.push_str(&format!(" {}", fmt_reals(components)));
            }
            out.push('\n');
        }
        IrOp::Form { form, matrix: m } => {
            out.push_str(&format!("form {} {}\n", form.0, matrix(*m)));
        }
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
            wmode,
        } => {
            let codes: Vec<u8> = glyphs.iter().flat_map(Glyph::code_bytes).collect();
            let codes = if glyphs.iter().all(|g| g.len == 1) {
                ps_string(&codes)
            } else {
                hex_string(&codes)
            };
            let displacements: Vec<f32> = glyphs.iter().flat_map(|g| [g.dx, g.dy]).collect();
            let vertical = if *wmode == 1 { " wmode=1" } else { "" };
            out.push_str(&format!(
                "text {} {} {} {}{vertical}\n",
                font.0,
                matrix(*m),
                codes,
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

fn optional(value: Option<f32>) -> String {
    value.map_or_else(|| "null".to_string(), fmt_real)
}

fn view(view: &View) -> String {
    match view {
        View::Fit => "fit".to_string(),
        View::FitH(top) => format!("fith {}", optional(*top)),
        View::Xyz { left, top, zoom } => {
            format!(
                "xyz {} {} {}",
                optional(*left),
                optional(*top),
                optional(*zoom)
            )
        }
    }
}

fn target(target: &Target) -> String {
    match target {
        Target::Named(name) => format!("dest={}", ps_name(name)),
        Target::Page { index, view: v } => format!("page={index} {}", view(v)),
    }
}

fn annot(out: &mut String, annot: &Annot) {
    let Bounds { llx, lly, urx, ury } = annot.rect;
    let where_to = match &annot.target {
        LinkTarget::Named(name) => format!("dest={}", ps_name(name)),
        LinkTarget::Uri(uri) => format!("uri={}", ps_string(uri)),
        LinkTarget::Page { index, view: v } => format!("page={index} {}", view(v)),
    };
    out.push_str(&format!(
        "annot link {} {where_to}",
        fmt_reals(&[llx, lly, urx, ury])
    ));
    if let Some(border) = annot.border {
        out.push_str(&format!(" border={}", fmt_reals(&border)));
    }
    if let Some(color) = &annot.color {
        out.push_str(&format!(" color={}", fmt_reals(color)));
    }
    if let Some(contents) = &annot.contents {
        out.push_str(&format!(" contents={}", ps_string(contents)));
    }
    out.push('\n');
}

fn attrs(out: &mut String, attrs: &PageAttrs) {
    if let Some(Bounds { llx, lly, urx, ury }) = attrs.crop_box {
        out.push_str(&format!(" cropbox {}", fmt_reals(&[llx, lly, urx, ury])));
    }
    if let Some(rotate) = attrs.rotate {
        out.push_str(&format!(" rotate {rotate}"));
    }
}

/// One line per mark, several for an information mark.
fn mark(out: &mut String, mark: &DocMark) {
    match mark {
        DocMark::Outline {
            title,
            count,
            target: t,
        } => {
            let where_to = t.as_ref().map_or_else(|| "none".to_string(), target);
            out.push_str(&format!("out {count} {} -> {where_to}\n", ps_string(title)));
        }
        DocMark::Dest {
            name,
            page,
            view: v,
        } => out.push_str(&format!(
            "dest {} -> page {page} {}\n",
            ps_name(name),
            view(v)
        )),
        DocMark::Info(entries) => {
            for (key, value) in entries {
                out.push_str(&format!("info {} {}\n", ps_name(key), ps_string(value)));
            }
        }
        DocMark::View {
            page_mode,
            page_layout,
            open,
        } => {
            out.push_str("view");
            if let Some(mode) = page_mode {
                out.push_str(&format!(" mode={}", ps_name(mode)));
            }
            if let Some(layout) = page_layout {
                out.push_str(&format!(" layout={}", ps_name(layout)));
            }
            if let Some(open) = open {
                out.push_str(&format!(" open={}", target(open)));
            }
            out.push('\n');
        }
        DocMark::PagesDefault(a) => {
            out.push_str("pages");
            attrs(out, a);
            out.push('\n');
        }
        DocMark::PageAttr { page, attrs: a } => {
            out.push_str(&format!("page {page}"));
            attrs(out, a);
            out.push('\n');
        }
        DocMark::Ignored { kind } => out.push_str(&format!("ignored {}\n", ps_name(kind))),
        DocMark::Params(entries) => {
            for (key, value) in entries {
                out.push_str(&format!("param {} {}\n", ps_name(key), mark_value(value)));
            }
        }
    }
}

/// A parameter value in PostScript syntax: names as `/name`, strings in
/// parentheses, arrays and dictionaries with their delimiters.
pub fn mark_value(value: &MarkValue) -> String {
    match value {
        MarkValue::Name(name) => ps_name(name),
        MarkValue::String(bytes) => ps_string(bytes),
        MarkValue::Int(v) => v.to_string(),
        MarkValue::Real(v) => fmt_real(*v),
        MarkValue::Bool(v) => v.to_string(),
        MarkValue::Null => "null".to_string(),
        MarkValue::Array(items) => {
            let inner: Vec<String> = items.iter().map(mark_value).collect();
            format!("[ {} ]", inner.join(" "))
        }
        MarkValue::Dict(entries) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{} {}", ps_name(k), mark_value(v)))
                .collect();
            format!("<< {} >>", inner.join(" "))
        }
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
    for (i, spec) in page.resources.patterns.iter().enumerate() {
        out.push_str(&pattern(i, spec));
    }
    for (i, spec) in page.resources.forms.iter().enumerate() {
        out.push_str(&form(i, spec));
    }
    out.push_str("ops:\n");
    ops(&mut out, &page.ops);
    for a in &page.annots {
        annot(&mut out, a);
    }
    out
}

/// The dump of a sequence of pages, separated by blank lines.
pub fn pages(pages: &[Page]) -> String {
    pages.iter().map(Page::dump).collect::<Vec<_>>().join("\n")
}

/// The `doc:` section for `marks`; empty when there are none.
pub fn doc<'a>(marks: impl IntoIterator<Item = &'a DocMark>) -> String {
    let mut out = String::new();
    for m in marks {
        mark(&mut out, m);
    }
    if out.is_empty() {
        out
    } else {
        format!("doc:\n{out}")
    }
}

/// The dump of a run: the pages as [`pages`], then the `doc:` section
/// after a blank line when there are marks. A run with marks but no
/// pages dumps as the version line and the section.
pub fn document<'a>(pages: &[Page], marks: impl IntoIterator<Item = &'a DocMark>) -> String {
    let mut out = self::pages(pages);
    let section = doc(marks);
    if section.is_empty() {
        return out;
    }
    if out.is_empty() {
        out.push_str(VERSION);
    }
    out.push('\n');
    out.push_str(&section);
    out
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
    fn an_encoded_image_is_flagged() {
        use crate::ir::SpaceRef;
        let spec = ImageSpec {
            width: 16,
            height: 16,
            bits_per_component: 8,
            color_space: Some(SpaceSpec::DeviceGray),
            decode: vec![0.0, 1.0],
            matrix: Matrix::IDENTITY,
            interpolate: true,
            is_mask: false,
            encoded: Some(Encoded::Dct),
        };
        let img = Image {
            spec,
            color_space: Some(SpaceRef(0)),
            data: vec![0xFF, 0xD8, 0xFF, 0xD9],
        };
        assert_eq!(
            image(3, &img),
            "img 3 16x16 bpc=8 cs=0 decode=[0 1] 4 bytes interpolate dct"
        );
        let plain = Image {
            spec: ImageSpec {
                interpolate: false,
                encoded: None,
                ..img.spec.clone()
            },
            ..img
        };
        assert_eq!(
            image(0, &plain),
            "img 0 16x16 bpc=8 cs=0 decode=[0 1] 4 bytes"
        );
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

    #[test]
    fn calibrated_spaces_print_their_parameters() {
        let white = [0.9505, 1.0, 1.089];
        let gray = SpaceSpec::CalGray {
            white,
            black: [0.0; 3],
            gamma: 2.2,
        };
        assert_eq!(
            space(&gray),
            "CalGray white=0.9505 1 1.089 black=0 0 0 gamma=2.2"
        );
        let rgb = SpaceSpec::CalRGB {
            white,
            black: [0.01, 0.01, 0.01],
            gamma: [1.8; 3],
            matrix: [0.4, 0.2, 0.02, 0.35, 0.7, 0.1, 0.2, 0.1, 0.95],
        };
        assert_eq!(
            space(&rgb),
            "CalRGB white=0.9505 1 1.089 black=0.01 0.01 0.01 gamma=1.8 1.8 1.8 matrix=0.4 0.2 0.02 0.35 0.7 0.1 0.2 0.1 0.95"
        );
        let lab = SpaceSpec::Lab {
            white,
            black: [0.0; 3],
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        assert_eq!(
            space(&lab),
            "Lab white=0.9505 1 1.089 black=0 0 0 range=-100 100 -100 100"
        );
        let indexed = SpaceSpec::Indexed {
            base: Box::new(gray),
            hival: 1,
            lookup: vec![0, 255],
        };
        assert_eq!(
            space(&indexed),
            "Indexed base=CalGray white=0.9505 1 1.089 black=0 0 0 gamma=2.2 hival=1 lookup=2 bytes"
        );
    }

    #[test]
    fn views_and_targets_print_absent_components_as_null() {
        assert_eq!(view(&View::Fit), "fit");
        assert_eq!(view(&View::FitH(None)), "fith null");
        assert_eq!(view(&View::FitH(Some(5.0))), "fith 5");
        assert_eq!(
            view(&View::Xyz {
                left: Some(0.0),
                top: Some(792.0),
                zoom: None
            }),
            "xyz 0 792 null"
        );
        assert_eq!(target(&Target::Named(b"a b".to_vec())), "dest=/a\\040b");
        assert_eq!(
            target(&Target::Page {
                index: 2,
                view: View::Fit
            }),
            "page=2 fit"
        );
    }

    #[test]
    fn a_document_without_marks_dumps_as_its_pages() {
        let page = Page::new(Bounds::new(0.0, 0.0, 10.0, 10.0));
        let plain = pages(std::slice::from_ref(&page));
        assert_eq!(document(std::slice::from_ref(&page), []), plain);
        assert_eq!(doc([]), "");
        let marks = [DocMark::Ignored {
            kind: b"X".to_vec(),
        }];
        assert_eq!(
            document(std::slice::from_ref(&page), &marks),
            format!("{plain}\ndoc:\nignored /X\n")
        );
        assert_eq!(document(&[], &marks), "ir/1\ndoc:\nignored /X\n");
        let params = [DocMark::Params(vec![
            (b"CompressPages".to_vec(), MarkValue::Bool(false)),
            (b"CompatibilityLevel".to_vec(), MarkValue::Real(1.4)),
            (b"Type".to_vec(), MarkValue::Name(b"Average".to_vec())),
            (
                b"Never".to_vec(),
                MarkValue::Array(vec![MarkValue::String(b"a".to_vec()), MarkValue::Null]),
            ),
            (
                b"D".to_vec(),
                MarkValue::Dict(vec![(b"k".to_vec(), MarkValue::Int(2))]),
            ),
        ])];
        assert_eq!(
            doc(&params),
            "doc:\nparam /CompressPages false\nparam /CompatibilityLevel 1.4\nparam /Type /Average\nparam /Never [ (a) null ]\nparam /D << /k 2 >>\n"
        );
    }
}
