// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A reader for Adobe Font Metrics files: the header's global values and
//! the per-character metrics. Kerning and composite sections are left
//! unread. The reader borrows the text, so a file embedded with
//! `include_str!` is parsed without copying its names.

use std::collections::HashMap;
use std::fmt;

/// Why a file could not be read as font metrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfmError {
    /// The file does not begin with `StartFontMetrics`.
    NotAfm,
    /// A required header entry is absent.
    Missing(&'static str),
    /// A line (1-based) that could not be understood.
    Malformed(usize),
}

impl fmt::Display for AfmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AfmError::NotAfm => f.write_str("not an AFM file"),
            AfmError::Missing(key) => write!(f, "missing {key}"),
            AfmError::Malformed(line) => write!(f, "malformed line {line}"),
        }
    }
}

impl std::error::Error for AfmError {}

/// One character's metrics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharMetric<'a> {
    /// The code in the font's built-in encoding, if it has one.
    pub code: Option<u8>,
    pub name: &'a str,
    /// Advance width in glyph units (thousandths of the em for these fonts).
    pub width: u16,
    /// Glyph bounding box `[llx lly urx ury]`.
    pub bbox: [f32; 4],
}

/// The parsed metrics of one font.
#[derive(Clone, Debug)]
pub struct Afm<'a> {
    pub font_name: &'a str,
    pub full_name: Option<&'a str>,
    pub family_name: Option<&'a str>,
    pub weight: Option<&'a str>,
    pub encoding_scheme: Option<&'a str>,
    pub font_bbox: [f32; 4],
    pub italic_angle: f32,
    pub is_fixed_pitch: bool,
    pub cap_height: Option<f32>,
    pub x_height: Option<f32>,
    pub ascender: Option<f32>,
    pub descender: Option<f32>,
    /// The dominant vertical stem width (`StdVW`), when the file states it.
    pub std_vw: Option<f32>,
    chars: Vec<CharMetric<'a>>,
    by_name: HashMap<&'a str, usize>,
    encoding: [Option<&'a str>; 256],
}

impl<'a> Afm<'a> {
    /// Reads the header and character metrics of `text`.
    pub fn parse(text: &'a str) -> Result<Self, AfmError> {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).enumerate();
        match lines.next() {
            Some((_, first)) if first.starts_with("StartFontMetrics") => {}
            _ => return Err(AfmError::NotAfm),
        }
        let mut font_name = None;
        let mut full_name = None;
        let mut family_name = None;
        let mut weight = None;
        let mut encoding_scheme = None;
        let mut font_bbox = None;
        let mut italic_angle = 0.0;
        let mut is_fixed_pitch = false;
        let mut cap_height = None;
        let mut x_height = None;
        let mut ascender = None;
        let mut descender = None;
        let mut std_vw = None;
        let mut in_chars = false;
        let mut chars = Vec::new();
        for (index, line) in lines {
            let number = index + 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if in_chars {
                if line == "EndCharMetrics" {
                    break;
                }
                chars.push(parse_char(line).ok_or(AfmError::Malformed(number))?);
                continue;
            }
            let (key, value) = match line.split_once(char::is_whitespace) {
                Some((key, value)) => (key, value.trim()),
                None => (line, ""),
            };
            let real = || {
                value
                    .parse::<f32>()
                    .map_err(|_| AfmError::Malformed(number))
            };
            match key {
                "FontName" => font_name = Some(value),
                "FullName" => full_name = Some(value),
                "FamilyName" => family_name = Some(value),
                "Weight" => weight = Some(value),
                "EncodingScheme" => encoding_scheme = Some(value),
                "FontBBox" => {
                    font_bbox = Some(parse_box(value).ok_or(AfmError::Malformed(number))?)
                }
                "ItalicAngle" => italic_angle = real()?,
                "IsFixedPitch" => is_fixed_pitch = value == "true",
                "CapHeight" => cap_height = Some(real()?),
                "XHeight" => x_height = Some(real()?),
                "Ascender" => ascender = Some(real()?),
                "Descender" => descender = Some(real()?),
                "StdVW" => std_vw = Some(real()?),
                "StartCharMetrics" => in_chars = true,
                "EndFontMetrics" => break,
                _ => {}
            }
        }
        let mut by_name = HashMap::with_capacity(chars.len());
        let mut encoding = [None; 256];
        for (index, metric) in chars.iter().enumerate() {
            by_name.entry(metric.name).or_insert(index);
            if let Some(code) = metric.code {
                encoding[usize::from(code)] = Some(metric.name);
            }
        }
        Ok(Afm {
            font_name: font_name.ok_or(AfmError::Missing("FontName"))?,
            full_name,
            family_name,
            weight,
            encoding_scheme,
            font_bbox: font_bbox.ok_or(AfmError::Missing("FontBBox"))?,
            italic_angle,
            is_fixed_pitch,
            cap_height,
            x_height,
            ascender,
            descender,
            std_vw,
            chars,
            by_name,
            encoding,
        })
    }

    /// Every character, in file order.
    pub fn chars(&self) -> &[CharMetric<'a>] {
        &self.chars
    }

    pub fn glyph(&self, name: &str) -> Option<&CharMetric<'a>> {
        self.by_name.get(name).map(|&i| &self.chars[i])
    }

    /// The advance width of the named glyph, if the font has it.
    pub fn width(&self, name: &str) -> Option<u16> {
        self.glyph(name).map(|g| g.width)
    }

    /// The glyph name at each code of the font's built-in encoding.
    pub fn encoding(&self) -> &[Option<&'a str>; 256] {
        &self.encoding
    }
}

fn parse_box(text: &str) -> Option<[f32; 4]> {
    let mut values = text.split_whitespace().map(|v| v.parse::<f32>().ok());
    let bbox = [
        values.next()??,
        values.next()??,
        values.next()??,
        values.next()??,
    ];
    values.next().is_none().then_some(bbox)
}

// `C code ; WX width ; N name ; B llx lly urx ury ;` in any order, with
// other keys (`L` ligatures) ignored. Codes outside a byte are unencoded.
fn parse_char(line: &str) -> Option<CharMetric<'_>> {
    let mut code = None;
    let mut width = None;
    let mut name = None;
    let mut bbox = [0.0; 4];
    for field in line.split(';') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let (key, value) = field.split_once(char::is_whitespace).unwrap_or((field, ""));
        let value = value.trim();
        match key {
            "C" => {
                let c: i32 = value.parse().ok()?;
                code = Some(u8::try_from(c).ok());
            }
            "CH" => {
                let hex = value.strip_prefix('<')?.strip_suffix('>')?;
                let c = u32::from_str_radix(hex, 16).ok()?;
                code = Some(u8::try_from(c).ok());
            }
            "WX" | "W0X" => {
                let w: f32 = value.parse().ok()?;
                width = Some(u16::try_from(w.round() as i64).ok()?);
            }
            "N" => name = Some(value),
            "B" => bbox = parse_box(value)?,
            _ => {}
        }
    }
    Some(CharMetric {
        code: code?,
        name: name?,
        width: width?,
        bbox,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "StartFontMetrics 4.1\r\n\
        Comment made up\r\n\
        FontName Sample\r\n\
        FontBBox -10 -20 30 40\r\n\
        EncodingScheme AdobeStandardEncoding\r\n\
        IsFixedPitch true\r\n\
        CapHeight 700\r\n\
        StartCharMetrics 3\r\n\
        C 65 ; WX 600 ; N A ; B 0 0 600 700 ;\r\n\
        C -1 ; WX 500 ; N Aacute ; B 0 0 500 900 ; L A acute ;\r\n\
        C 66 ; WX 610 ; N B ; B 1 2 3 4 ;\r\n\
        EndCharMetrics\r\n\
        StartKernData\r\n\
        KPX A B -30\r\n\
        EndKernData\r\n\
        EndFontMetrics\r\n";

    #[test]
    fn header_and_metrics_are_read() {
        let afm = Afm::parse(SAMPLE).unwrap();
        assert_eq!(afm.font_name, "Sample");
        assert_eq!(afm.font_bbox, [-10.0, -20.0, 30.0, 40.0]);
        assert_eq!(afm.encoding_scheme, Some("AdobeStandardEncoding"));
        assert!(afm.is_fixed_pitch);
        assert_eq!(afm.cap_height, Some(700.0));
        assert_eq!(afm.x_height, None);
        assert_eq!(afm.chars().len(), 3);
        assert_eq!(afm.width("A"), Some(600));
        assert_eq!(afm.width("Aacute"), Some(500));
        assert_eq!(afm.width("C"), None);
        assert_eq!(afm.glyph("B").unwrap().bbox, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(afm.glyph("Aacute").unwrap().code, None);
        assert_eq!(afm.encoding()[65], Some("A"));
        assert_eq!(afm.encoding()[66], Some("B"));
        assert_eq!(afm.encoding()[67], None);
    }

    #[test]
    fn errors_name_their_cause() {
        assert_eq!(Afm::parse("hello").err(), Some(AfmError::NotAfm));
        assert_eq!(
            Afm::parse("StartFontMetrics 4.1\nFontBBox 0 0 0 0\n").err(),
            Some(AfmError::Missing("FontName"))
        );
        assert_eq!(
            Afm::parse("StartFontMetrics 4.1\nFontName X\n").err(),
            Some(AfmError::Missing("FontBBox"))
        );
        assert_eq!(
            Afm::parse("StartFontMetrics 4.1\nFontName X\nFontBBox 0 0 0\n").err(),
            Some(AfmError::Malformed(3))
        );
        assert_eq!(
            Afm::parse(
                "StartFontMetrics 4.1\nFontName X\nFontBBox 0 0 0 0\nStartCharMetrics 1\nC 65 ; N A ;\n"
            )
            .err(),
            Some(AfmError::Malformed(5))
        );
        assert_eq!(AfmError::Malformed(5).to_string(), "malformed line 5");
    }
}
