// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The fourteen standard fonts and their metrics, read from the embedded
//! AFM files on first use.

use std::sync::OnceLock;

use crate::afm::Afm;
use crate::encoding::{Encoding, STANDARD_ENCODING};

/// The standard fourteen, in the order `resourceforall` lists them
/// (sorted by PostScript name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StdFont {
    Courier,
    CourierBold,
    CourierBoldOblique,
    CourierOblique,
    Helvetica,
    HelveticaBold,
    HelveticaBoldOblique,
    HelveticaOblique,
    Symbol,
    TimesBold,
    TimesBoldItalic,
    TimesItalic,
    TimesRoman,
    ZapfDingbats,
}

/// A typeface family of the standard set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Courier,
    Helvetica,
    Times,
    Symbol,
    ZapfDingbats,
}

impl StdFont {
    pub const ALL: [StdFont; 14] = [
        StdFont::Courier,
        StdFont::CourierBold,
        StdFont::CourierBoldOblique,
        StdFont::CourierOblique,
        StdFont::Helvetica,
        StdFont::HelveticaBold,
        StdFont::HelveticaBoldOblique,
        StdFont::HelveticaOblique,
        StdFont::Symbol,
        StdFont::TimesBold,
        StdFont::TimesBoldItalic,
        StdFont::TimesItalic,
        StdFont::TimesRoman,
        StdFont::ZapfDingbats,
    ];

    pub fn postscript_name(self) -> &'static str {
        match self {
            StdFont::Courier => "Courier",
            StdFont::CourierBold => "Courier-Bold",
            StdFont::CourierBoldOblique => "Courier-BoldOblique",
            StdFont::CourierOblique => "Courier-Oblique",
            StdFont::Helvetica => "Helvetica",
            StdFont::HelveticaBold => "Helvetica-Bold",
            StdFont::HelveticaBoldOblique => "Helvetica-BoldOblique",
            StdFont::HelveticaOblique => "Helvetica-Oblique",
            StdFont::Symbol => "Symbol",
            StdFont::TimesBold => "Times-Bold",
            StdFont::TimesBoldItalic => "Times-BoldItalic",
            StdFont::TimesItalic => "Times-Italic",
            StdFont::TimesRoman => "Times-Roman",
            StdFont::ZapfDingbats => "ZapfDingbats",
        }
    }

    /// The font with exactly this PostScript name.
    pub fn from_postscript_name(name: &[u8]) -> Option<StdFont> {
        StdFont::ALL
            .into_iter()
            .find(|f| f.postscript_name().as_bytes() == name)
    }

    /// Position in [`StdFont::ALL`], stable for the life of the crate.
    pub fn index(self) -> usize {
        StdFont::ALL
            .iter()
            .position(|&f| f == self)
            .expect("every font is listed")
    }

    pub fn from_index(index: usize) -> Option<StdFont> {
        StdFont::ALL.get(index).copied()
    }

    pub fn family(self) -> Family {
        match self {
            StdFont::Courier
            | StdFont::CourierBold
            | StdFont::CourierBoldOblique
            | StdFont::CourierOblique => Family::Courier,
            StdFont::Helvetica
            | StdFont::HelveticaBold
            | StdFont::HelveticaBoldOblique
            | StdFont::HelveticaOblique => Family::Helvetica,
            StdFont::TimesBold
            | StdFont::TimesBoldItalic
            | StdFont::TimesItalic
            | StdFont::TimesRoman => Family::Times,
            StdFont::Symbol => Family::Symbol,
            StdFont::ZapfDingbats => Family::ZapfDingbats,
        }
    }

    pub fn is_bold(self) -> bool {
        matches!(
            self,
            StdFont::CourierBold
                | StdFont::CourierBoldOblique
                | StdFont::HelveticaBold
                | StdFont::HelveticaBoldOblique
                | StdFont::TimesBold
                | StdFont::TimesBoldItalic
        )
    }

    pub fn is_italic(self) -> bool {
        matches!(
            self,
            StdFont::CourierBoldOblique
                | StdFont::CourierOblique
                | StdFont::HelveticaBoldOblique
                | StdFont::HelveticaOblique
                | StdFont::TimesBoldItalic
                | StdFont::TimesItalic
        )
    }

    /// The member of `family` with the given weight and slope; Symbol and
    /// ZapfDingbats have one style.
    pub fn styled(family: Family, bold: bool, italic: bool) -> StdFont {
        match (family, bold, italic) {
            (Family::Courier, false, false) => StdFont::Courier,
            (Family::Courier, true, false) => StdFont::CourierBold,
            (Family::Courier, false, true) => StdFont::CourierOblique,
            (Family::Courier, true, true) => StdFont::CourierBoldOblique,
            (Family::Helvetica, false, false) => StdFont::Helvetica,
            (Family::Helvetica, true, false) => StdFont::HelveticaBold,
            (Family::Helvetica, false, true) => StdFont::HelveticaOblique,
            (Family::Helvetica, true, true) => StdFont::HelveticaBoldOblique,
            (Family::Times, false, false) => StdFont::TimesRoman,
            (Family::Times, true, false) => StdFont::TimesBold,
            (Family::Times, false, true) => StdFont::TimesItalic,
            (Family::Times, true, true) => StdFont::TimesBoldItalic,
            (Family::Symbol, _, _) => StdFont::Symbol,
            (Family::ZapfDingbats, _, _) => StdFont::ZapfDingbats,
        }
    }

    /// Whether the font's built-in encoding is its own rather than
    /// `StandardEncoding`.
    pub fn is_symbolic(self) -> bool {
        matches!(self, StdFont::Symbol | StdFont::ZapfDingbats)
    }

    fn source(self) -> &'static str {
        match self {
            StdFont::Courier => include_str!("../data/core14/Courier.afm"),
            StdFont::CourierBold => include_str!("../data/core14/Courier-Bold.afm"),
            StdFont::CourierBoldOblique => include_str!("../data/core14/Courier-BoldOblique.afm"),
            StdFont::CourierOblique => include_str!("../data/core14/Courier-Oblique.afm"),
            StdFont::Helvetica => include_str!("../data/core14/Helvetica.afm"),
            StdFont::HelveticaBold => include_str!("../data/core14/Helvetica-Bold.afm"),
            StdFont::HelveticaBoldOblique => {
                include_str!("../data/core14/Helvetica-BoldOblique.afm")
            }
            StdFont::HelveticaOblique => include_str!("../data/core14/Helvetica-Oblique.afm"),
            StdFont::Symbol => include_str!("../data/core14/Symbol.afm"),
            StdFont::TimesBold => include_str!("../data/core14/Times-Bold.afm"),
            StdFont::TimesBoldItalic => include_str!("../data/core14/Times-BoldItalic.afm"),
            StdFont::TimesItalic => include_str!("../data/core14/Times-Italic.afm"),
            StdFont::TimesRoman => include_str!("../data/core14/Times-Roman.afm"),
            StdFont::ZapfDingbats => include_str!("../data/core14/ZapfDingbats.afm"),
        }
    }

    /// The font's metrics, parsed on first access.
    pub fn metrics(self) -> &'static Afm<'static> {
        static METRICS: [OnceLock<Afm<'static>>; 14] = [const { OnceLock::new() }; 14];
        METRICS[self.index()].get_or_init(|| {
            Afm::parse(self.source()).expect("the embedded AFM files are well formed")
        })
    }

    /// The advance width of the named glyph in thousandths of the em, if
    /// the font has the glyph.
    pub fn width(self, glyph: &str) -> Option<u16> {
        self.metrics().width(glyph)
    }

    /// The font bounding box `[llx lly urx ury]` in glyph units.
    pub fn bbox(self) -> [f32; 4] {
        self.metrics().font_bbox
    }

    /// The encoding the font has before a program re-encodes it:
    /// `StandardEncoding` for the text fonts, the font's own for Symbol and
    /// ZapfDingbats.
    pub fn builtin_encoding(self) -> &'static Encoding {
        if self.is_symbolic() {
            self.metrics().encoding()
        } else {
            &STANDARD_ENCODING
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_in_sorted_order() {
        for (k, font) in StdFont::ALL.into_iter().enumerate() {
            assert_eq!(font.index(), k);
            assert_eq!(StdFont::from_index(k), Some(font));
            assert_eq!(
                StdFont::from_postscript_name(font.postscript_name().as_bytes()),
                Some(font)
            );
            assert_eq!(
                StdFont::styled(font.family(), font.is_bold(), font.is_italic()),
                font
            );
        }
        let mut names: Vec<_> = StdFont::ALL.iter().map(|f| f.postscript_name()).collect();
        let sorted = names.clone();
        names.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(StdFont::from_postscript_name(b"Arial"), None);
        assert_eq!(StdFont::from_index(14), None);
        assert_eq!(StdFont::styled(Family::Symbol, true, true), StdFont::Symbol);
    }

    #[test]
    fn all_fourteen_parse_with_their_own_names() {
        for font in StdFont::ALL {
            let afm = font.metrics();
            assert_eq!(afm.font_name, font.postscript_name());
            assert!(afm.chars().len() >= 188, "{}", font.postscript_name());
            assert_eq!(
                afm.encoding_scheme,
                Some(if font.is_symbolic() {
                    "FontSpecific"
                } else {
                    "AdobeStandardEncoding"
                })
            );
        }
    }

    #[test]
    fn widths_and_encodings() {
        assert_eq!(StdFont::Helvetica.width("H"), Some(722));
        assert_eq!(StdFont::Helvetica.width("W"), Some(944));
        assert_eq!(StdFont::Courier.width("a"), Some(600));
        assert_eq!(StdFont::Helvetica.width("nosuchglyph"), None);
        assert_eq!(StdFont::Symbol.builtin_encoding()[97], Some("alpha"));
        assert_eq!(StdFont::ZapfDingbats.builtin_encoding()[97], Some("a60"));
        assert_eq!(StdFont::Helvetica.builtin_encoding()[65], Some("A"));
        assert_eq!(StdFont::TimesRoman.bbox(), [-168.0, -218.0, 1000.0, 898.0]);
        assert!(StdFont::Courier.metrics().is_fixed_pitch);
    }

    #[test]
    fn standard_encoding_agrees_with_every_text_font() {
        for font in StdFont::ALL.into_iter().filter(|f| !f.is_symbolic()) {
            let afm = font.metrics();
            for (code, name) in afm.encoding().iter().enumerate() {
                assert_eq!(
                    *name,
                    STANDARD_ENCODING[code],
                    "{} code {code}",
                    font.postscript_name()
                );
            }
        }
    }
}
