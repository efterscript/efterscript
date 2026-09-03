// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Derived metric tables: the advances, font bounding box, and file
//! encoding of a Type 1 program in a small text form, generated at
//! intake from the program itself and embedded in place of a metrics
//! file. The format is `metrics/1` on the first line, then `bbox llx
//! lly urx ury`, `enc <code> /<name>` for each encoded code, and `w
//! /<name> <advance>` for every charstring sorted by name, all values
//! integers; lines starting with `#` are comments. The reader borrows
//! the text, so a table embedded with `include_str!` is parsed without
//! copying its names.

use std::fmt;

use crate::encoding::STANDARD_ENCODING;
use crate::program::FontError;
use crate::type1::{FileEncoding, ParsedFont};

/// The first line of a table.
pub const VERSION_LINE: &str = "metrics/1";

/// Why a text could not be read as a metric table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetricsError {
    /// The first line is not the version line.
    NotTable,
    /// The bounding box line is absent.
    MissingBBox,
    /// A line (1-based) that could not be understood, or a width line
    /// out of name order.
    Malformed(usize),
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricsError::NotTable => f.write_str("not a metric table"),
            MetricsError::MissingBBox => f.write_str("missing bbox"),
            MetricsError::Malformed(line) => write!(f, "malformed line {line}"),
        }
    }
}

impl std::error::Error for MetricsError {}

/// The metrics of one face.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricTable<'a> {
    bbox: [f32; 4],
    encoding: [Option<&'a str>; 256],
    /// `(name, advance)` sorted by name, for binary search.
    widths: Vec<(&'a str, u16)>,
}

/// A table derived from a program, with the advances that were not
/// integral and had to be rounded.
#[derive(Clone, Debug, PartialEq)]
pub struct Derived<'a> {
    pub table: MetricTable<'a>,
    /// `(name, advance as the charstring computes it)` for every width
    /// the table rounds.
    pub rounded: Vec<(&'a str, f32)>,
}

impl<'a> MetricTable<'a> {
    /// Reads a table.
    pub fn parse(text: &'a str) -> Result<Self, MetricsError> {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).enumerate();
        match lines.next() {
            Some((_, first)) if first == VERSION_LINE => {}
            _ => return Err(MetricsError::NotTable),
        }
        let mut bbox = None;
        let mut encoding = [None; 256];
        let mut widths: Vec<(&'a str, u16)> = Vec::new();
        for (index, line) in lines {
            let malformed = || MetricsError::Malformed(index + 1);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split(' ');
            match fields.next() {
                Some("bbox") => {
                    let mut values = fields.map(|v| v.parse::<i32>().ok());
                    let b = [
                        values.next().flatten().ok_or_else(malformed)?,
                        values.next().flatten().ok_or_else(malformed)?,
                        values.next().flatten().ok_or_else(malformed)?,
                        values.next().flatten().ok_or_else(malformed)?,
                    ];
                    if values.next().is_some() || bbox.is_some() {
                        return Err(malformed());
                    }
                    bbox = Some(b.map(|v| v as f32));
                }
                Some("enc") => {
                    let code: u8 = fields
                        .next()
                        .and_then(|c| c.parse().ok())
                        .ok_or_else(malformed)?;
                    let name = fields
                        .next()
                        .and_then(|n| n.strip_prefix('/'))
                        .filter(|n| !n.is_empty())
                        .ok_or_else(malformed)?;
                    if fields.next().is_some() {
                        return Err(malformed());
                    }
                    encoding[usize::from(code)] = Some(name);
                }
                Some("w") => {
                    let name = fields
                        .next()
                        .and_then(|n| n.strip_prefix('/'))
                        .filter(|n| !n.is_empty())
                        .ok_or_else(malformed)?;
                    let advance: u16 = fields
                        .next()
                        .and_then(|w| w.parse().ok())
                        .ok_or_else(malformed)?;
                    if fields.next().is_some() || widths.last().is_some_and(|(p, _)| *p >= name) {
                        return Err(malformed());
                    }
                    widths.push((name, advance));
                }
                _ => return Err(malformed()),
            }
        }
        Ok(MetricTable {
            bbox: bbox.ok_or(MetricsError::MissingBBox)?,
            encoding,
            widths,
        })
    }

    /// The table of a parsed program: its bounding box, its encoding,
    /// and every charstring's advance as the interpreter computes it.
    /// A bounding box that is not integral, a glyph name that is not
    /// UTF-8, or an advance outside a `u16` is an error; an advance that
    /// is not integral is rounded and reported.
    pub fn derive(font: &'a ParsedFont) -> Result<Derived<'a>, FontError> {
        let program = &font.program;
        let bbox = program.dict().font_bbox;
        if bbox.iter().any(|v| *v != v.trunc()) {
            return Err(FontError::Malformed("font bounding box not integral"));
        }
        let mut encoding = [None; 256];
        for (code, slot) in encoding.iter_mut().enumerate() {
            *slot = match &font.encoding {
                FileEncoding::Standard => STANDARD_ENCODING[code],
                FileEncoding::Custom(names) => match &names[code] {
                    Some(name) => Some(
                        std::str::from_utf8(name)
                            .map_err(|_| FontError::Malformed("glyph name not UTF-8"))?,
                    ),
                    None => None,
                },
            };
        }
        let mut widths = Vec::with_capacity(program.charstrings().len());
        let mut rounded = Vec::new();
        for name in program.charstrings().keys() {
            let name = std::str::from_utf8(name)
                .map_err(|_| FontError::Malformed("glyph name not UTF-8"))?;
            let glyph = program
                .glyph(name.as_bytes())?
                .ok_or(FontError::Malformed("charstring vanished"))?;
            let advance = glyph.advance.0;
            let integral = advance.round();
            if integral != advance {
                rounded.push((name, advance));
            }
            let width = u16::try_from(integral as i64)
                .map_err(|_| FontError::Malformed("advance outside a u16"))?;
            widths.push((name, width));
        }
        Ok(Derived {
            table: MetricTable {
                bbox,
                encoding,
                widths,
            },
            rounded,
        })
    }

    /// The table as text, naming `source` (the program file) in its
    /// header comments.
    pub fn render(&self, source: &str) -> String {
        let mut out = format!(
            "{VERSION_LINE}\n\
             # SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
             # SPDX-License-Identifier: MIT\n\
             # GENERATED-BY: cargo xtask fetch-fonts from {source}\n"
        );
        let b = self.bbox.map(|v| v as i64);
        out.push_str(&format!("bbox {} {} {} {}\n", b[0], b[1], b[2], b[3]));
        for (code, name) in self.encoding.iter().enumerate() {
            if let Some(name) = name {
                out.push_str(&format!("enc {code} /{name}\n"));
            }
        }
        for (name, width) in &self.widths {
            out.push_str(&format!("w /{name} {width}\n"));
        }
        out
    }

    /// The advance width of the named glyph, if the face has it.
    pub fn width(&self, name: &str) -> Option<u16> {
        self.widths
            .binary_search_by(|(n, _)| (*n).cmp(name))
            .ok()
            .map(|k| self.widths[k].1)
    }

    /// The font bounding box `[llx lly urx ury]` in glyph units.
    pub fn bbox(&self) -> [f32; 4] {
        self.bbox
    }

    /// The glyph name at each code of the program file's encoding.
    pub fn encoding(&self) -> &[Option<&'a str>; 256] {
        &self.encoding
    }

    /// Every glyph's advance, sorted by name.
    pub fn widths(&self) -> &[(&'a str, u16)] {
        &self.widths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{CharstringBuilder, Type1Font, rectangle};
    use crate::type1::parse_file;

    const SAMPLE: &str = "metrics/1\r\n\
        # a comment\r\n\
        bbox -10 -20 30 40\r\n\
        \r\n\
        enc 65 /A\r\n\
        enc 66 /B\r\n\
        w /.notdef 0\r\n\
        w /A 600\r\n\
        w /B 610\r\n";

    #[test]
    fn a_table_is_read() {
        let table = MetricTable::parse(SAMPLE).unwrap();
        assert_eq!(table.bbox(), [-10.0, -20.0, 30.0, 40.0]);
        assert_eq!(table.encoding()[65], Some("A"));
        assert_eq!(table.encoding()[66], Some("B"));
        assert_eq!(table.encoding()[67], None);
        assert_eq!(table.width("A"), Some(600));
        assert_eq!(table.width("B"), Some(610));
        assert_eq!(table.width(".notdef"), Some(0));
        assert_eq!(table.width("C"), None);
        assert_eq!(table.width(""), None);
        assert_eq!(table.widths().len(), 3);
    }

    #[test]
    fn errors_name_their_cause() {
        let err = |text: &str| MetricTable::parse(text).err();
        assert_eq!(err("hello"), Some(MetricsError::NotTable));
        assert_eq!(
            err("metrics/2\nbbox 0 0 0 0\n"),
            Some(MetricsError::NotTable)
        );
        assert_eq!(err("metrics/1\nw /a 1\n"), Some(MetricsError::MissingBBox));
        assert_eq!(
            err("metrics/1\nbbox 0 0 0\n"),
            Some(MetricsError::Malformed(2))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0 0\n"),
            Some(MetricsError::Malformed(2))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nbbox 0 0 0 0\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0.5 0 0 0\n"),
            Some(MetricsError::Malformed(2))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nenc 256 /a\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nenc 65 a\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nenc 65 /\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nenc 65 /a x\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw /a -1\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw /a 1.5\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw a 1\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw /a 1 2\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw /b 1\nw /a 1\n"),
            Some(MetricsError::Malformed(4))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nw /a 1\nw /a 1\n"),
            Some(MetricsError::Malformed(4))
        );
        assert_eq!(
            err("metrics/1\nbbox 0 0 0 0\nkern /a /b 1\n"),
            Some(MetricsError::Malformed(3))
        );
        assert_eq!(MetricsError::Malformed(5).to_string(), "malformed line 5");
        assert_eq!(MetricsError::MissingBBox.to_string(), "missing bbox");
        assert_eq!(MetricsError::NotTable.to_string(), "not a metric table");
    }

    fn font() -> Type1Font {
        // `c` computes its advance as 500 + 1/3 through `div`.
        let c = CharstringBuilder::new()
            .num(0)
            .num(1501)
            .num(3)
            .div()
            .op(13)
            .endchar()
            .bytes();
        Type1Font::new("Syn")
            .bbox([-5, -10, 700, 800])
            .glyph("b", 400, &rectangle(0.0, 0.0, 100.0, 100.0))
            .glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0))
            .charstring("c", c)
            .encode(97, "a")
            .encode(66, "b")
    }

    #[test]
    fn a_derived_table_renders_sorted_and_reads_back_equal() {
        let font = font();
        let parsed = parse_file(&font.pfb()).unwrap();
        let derived = MetricTable::derive(&parsed).unwrap();
        assert_eq!(derived.rounded.len(), 1);
        assert_eq!(derived.rounded[0].0, "c");
        assert!((derived.rounded[0].1 - 1501.0 / 3.0).abs() < 1e-3);
        let text = derived.table.render("syn.pfb");
        assert_eq!(
            text,
            "metrics/1\n\
             # SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
             # SPDX-License-Identifier: MIT\n\
             # GENERATED-BY: cargo xtask fetch-fonts from syn.pfb\n\
             bbox -5 -10 700 800\n\
             enc 66 /b\n\
             enc 97 /a\n\
             w /.notdef 0\n\
             w /a 600\n\
             w /b 400\n\
             w /c 500\n"
        );
        let again = MetricTable::parse(&text).unwrap();
        assert_eq!(again, derived.table);
        assert_eq!(again.width("c"), Some(500));
        assert_eq!(again.encoding()[97], Some("a"));
    }

    #[test]
    fn a_standard_encoding_is_spelled_out_and_faults_are_errors() {
        let font = font();
        let mut parsed = parse_file(&font.pfb()).unwrap();
        parsed.encoding = FileEncoding::Standard;
        let derived = MetricTable::derive(&parsed).unwrap();
        assert_eq!(derived.table.encoding()[65], Some("A"));
        assert_eq!(derived.table.encoding()[0], None);
        assert!(derived.table.render("x").contains("enc 32 /space\n"));

        let bad = Type1Font::new("Bad").charstring(
            "wide",
            CharstringBuilder::new().hsbw(0, 70000).endchar().bytes(),
        );
        let parsed = parse_file(&bad.pfb()).unwrap();
        assert_eq!(
            MetricTable::derive(&parsed).err(),
            Some(FontError::Malformed("advance outside a u16"))
        );
        let bad = Type1Font::new("Bad").charstring("m", CharstringBuilder::new().num(1).bytes());
        let parsed = parse_file(&bad.pfb()).unwrap();
        assert_eq!(
            MetricTable::derive(&parsed).err(),
            Some(FontError::Truncated("charstring"))
        );
    }
}
