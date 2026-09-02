// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The resident set: the fourteen standard fonts ([`StdFont`], whose
//! standard-14 status the PDF writer relies on) and the thirty-five
//! resident faces ([`ResidentFace`]) — the fourteen plus the twenty-one
//! LaserWriter faces — with their metrics, read from the embedded AFM
//! files on first use, and the outline asset each is drawn from.

use std::sync::OnceLock;

use crate::afm::Afm;
use crate::encoding::{Encoding, STANDARD_ENCODING};
use crate::outlines::OutlineAsset;

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

/// A typeface family of the resident set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Courier,
    Helvetica,
    Times,
    Symbol,
    ZapfDingbats,
    AvantGarde,
    Bookman,
    HelveticaNarrow,
    NewCenturySchlbk,
    Palatino,
    ZapfChancery,
}

impl Family {
    /// Whether the family's faces have serifs, for the PDF descriptor.
    pub fn is_serif(self) -> bool {
        matches!(
            self,
            Family::Times
                | Family::Courier
                | Family::Bookman
                | Family::NewCenturySchlbk
                | Family::Palatino
                | Family::ZapfChancery
        )
    }
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
        self.face().postscript_name()
    }

    /// The font with exactly this PostScript name.
    pub fn from_postscript_name(name: &[u8]) -> Option<StdFont> {
        ResidentFace::from_postscript_name(name)?.std_font()
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

    /// The resident face this font is.
    pub fn face(self) -> ResidentFace {
        match self {
            StdFont::Courier => ResidentFace::Courier,
            StdFont::CourierBold => ResidentFace::CourierBold,
            StdFont::CourierBoldOblique => ResidentFace::CourierBoldOblique,
            StdFont::CourierOblique => ResidentFace::CourierOblique,
            StdFont::Helvetica => ResidentFace::Helvetica,
            StdFont::HelveticaBold => ResidentFace::HelveticaBold,
            StdFont::HelveticaBoldOblique => ResidentFace::HelveticaBoldOblique,
            StdFont::HelveticaOblique => ResidentFace::HelveticaOblique,
            StdFont::Symbol => ResidentFace::Symbol,
            StdFont::TimesBold => ResidentFace::TimesBold,
            StdFont::TimesBoldItalic => ResidentFace::TimesBoldItalic,
            StdFont::TimesItalic => ResidentFace::TimesItalic,
            StdFont::TimesRoman => ResidentFace::TimesRoman,
            StdFont::ZapfDingbats => ResidentFace::ZapfDingbats,
        }
    }

    pub fn family(self) -> Family {
        self.face().family()
    }

    pub fn is_bold(self) -> bool {
        self.face().is_bold()
    }

    pub fn is_italic(self) -> bool {
        self.face().is_italic()
    }

    /// The member of `family` with the given weight and slope; Symbol and
    /// ZapfDingbats have one style. A family outside the fourteen gives
    /// the standard font nearest to it (Times for the serif families,
    /// Helvetica for the sans ones, Times-Italic for ZapfChancery).
    pub fn styled(family: Family, bold: bool, italic: bool) -> StdFont {
        let family = match family {
            Family::AvantGarde | Family::HelveticaNarrow => Family::Helvetica,
            Family::Bookman | Family::NewCenturySchlbk | Family::Palatino => Family::Times,
            Family::ZapfChancery => return StdFont::TimesItalic,
            other => other,
        };
        ResidentFace::styled(family, bold, italic)
            .std_font()
            .expect("the five families are standard")
    }

    /// Whether the font's built-in encoding is its own rather than
    /// `StandardEncoding`.
    pub fn is_symbolic(self) -> bool {
        matches!(self, StdFont::Symbol | StdFont::ZapfDingbats)
    }

    /// The font's metrics, parsed on first access.
    pub fn metrics(self) -> &'static Afm<'static> {
        self.face().metrics()
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
        self.face().builtin_encoding()
    }
}

/// The thirty-five resident faces, in the order `resourceforall` lists
/// them (sorted by PostScript name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResidentFace {
    AvantGardeBook,
    AvantGardeBookOblique,
    AvantGardeDemi,
    AvantGardeDemiOblique,
    BookmanDemi,
    BookmanDemiItalic,
    BookmanLight,
    BookmanLightItalic,
    Courier,
    CourierBold,
    CourierBoldOblique,
    CourierOblique,
    Helvetica,
    HelveticaBold,
    HelveticaBoldOblique,
    HelveticaNarrow,
    HelveticaNarrowBold,
    HelveticaNarrowBoldOblique,
    HelveticaNarrowOblique,
    HelveticaOblique,
    NewCenturySchlbkBold,
    NewCenturySchlbkBoldItalic,
    NewCenturySchlbkItalic,
    NewCenturySchlbkRoman,
    PalatinoBold,
    PalatinoBoldItalic,
    PalatinoItalic,
    PalatinoRoman,
    Symbol,
    TimesBold,
    TimesBoldItalic,
    TimesItalic,
    TimesRoman,
    ZapfChanceryMediumItalic,
    ZapfDingbats,
}

/// What a face is: its name, family, style, standard-14 identity, and
/// the asset its outlines come from.
struct Record {
    name: &'static str,
    family: Family,
    bold: bool,
    italic: bool,
    std: Option<StdFont>,
    asset: Option<OutlineAsset>,
}

macro_rules! record {
    ($name:literal, $family:ident, $bold:literal, $italic:literal, $std:expr, $asset:expr) => {
        Record {
            name: $name,
            family: Family::$family,
            bold: $bold,
            italic: $italic,
            std: $std,
            asset: $asset,
        }
    };
}

use OutlineAsset::{Liberation, TexGyre};

const RECORDS: [Record; 35] = [
    record!(
        "AvantGarde-Book",
        AvantGarde,
        false,
        false,
        None,
        Some(TexGyre("qagr"))
    ),
    record!(
        "AvantGarde-BookOblique",
        AvantGarde,
        false,
        true,
        None,
        Some(TexGyre("qagri"))
    ),
    record!(
        "AvantGarde-Demi",
        AvantGarde,
        true,
        false,
        None,
        Some(TexGyre("qagb"))
    ),
    record!(
        "AvantGarde-DemiOblique",
        AvantGarde,
        true,
        true,
        None,
        Some(TexGyre("qagbi"))
    ),
    record!(
        "Bookman-Demi",
        Bookman,
        true,
        false,
        None,
        Some(TexGyre("qbkb"))
    ),
    record!(
        "Bookman-DemiItalic",
        Bookman,
        true,
        true,
        None,
        Some(TexGyre("qbkbi"))
    ),
    record!(
        "Bookman-Light",
        Bookman,
        false,
        false,
        None,
        Some(TexGyre("qbkr"))
    ),
    record!(
        "Bookman-LightItalic",
        Bookman,
        false,
        true,
        None,
        Some(TexGyre("qbkri"))
    ),
    record!(
        "Courier",
        Courier,
        false,
        false,
        Some(StdFont::Courier),
        Some(Liberation("LiberationMono-Regular"))
    ),
    record!(
        "Courier-Bold",
        Courier,
        true,
        false,
        Some(StdFont::CourierBold),
        Some(Liberation("LiberationMono-Bold"))
    ),
    record!(
        "Courier-BoldOblique",
        Courier,
        true,
        true,
        Some(StdFont::CourierBoldOblique),
        Some(Liberation("LiberationMono-BoldItalic"))
    ),
    record!(
        "Courier-Oblique",
        Courier,
        false,
        true,
        Some(StdFont::CourierOblique),
        Some(Liberation("LiberationMono-Italic"))
    ),
    record!(
        "Helvetica",
        Helvetica,
        false,
        false,
        Some(StdFont::Helvetica),
        Some(Liberation("LiberationSans-Regular"))
    ),
    record!(
        "Helvetica-Bold",
        Helvetica,
        true,
        false,
        Some(StdFont::HelveticaBold),
        Some(Liberation("LiberationSans-Bold"))
    ),
    record!(
        "Helvetica-BoldOblique",
        Helvetica,
        true,
        true,
        Some(StdFont::HelveticaBoldOblique),
        Some(Liberation("LiberationSans-BoldItalic"))
    ),
    record!(
        "Helvetica-Narrow",
        HelveticaNarrow,
        false,
        false,
        None,
        Some(TexGyre("qhvcr"))
    ),
    record!(
        "Helvetica-Narrow-Bold",
        HelveticaNarrow,
        true,
        false,
        None,
        Some(TexGyre("qhvcb"))
    ),
    record!(
        "Helvetica-Narrow-BoldOblique",
        HelveticaNarrow,
        true,
        true,
        None,
        Some(TexGyre("qhvcbi"))
    ),
    record!(
        "Helvetica-Narrow-Oblique",
        HelveticaNarrow,
        false,
        true,
        None,
        Some(TexGyre("qhvcri"))
    ),
    record!(
        "Helvetica-Oblique",
        Helvetica,
        false,
        true,
        Some(StdFont::HelveticaOblique),
        Some(Liberation("LiberationSans-Italic"))
    ),
    record!(
        "NewCenturySchlbk-Bold",
        NewCenturySchlbk,
        true,
        false,
        None,
        Some(TexGyre("qcsb"))
    ),
    record!(
        "NewCenturySchlbk-BoldItalic",
        NewCenturySchlbk,
        true,
        true,
        None,
        Some(TexGyre("qcsbi"))
    ),
    record!(
        "NewCenturySchlbk-Italic",
        NewCenturySchlbk,
        false,
        true,
        None,
        Some(TexGyre("qcsri"))
    ),
    record!(
        "NewCenturySchlbk-Roman",
        NewCenturySchlbk,
        false,
        false,
        None,
        Some(TexGyre("qcsr"))
    ),
    record!(
        "Palatino-Bold",
        Palatino,
        true,
        false,
        None,
        Some(TexGyre("qplb"))
    ),
    record!(
        "Palatino-BoldItalic",
        Palatino,
        true,
        true,
        None,
        Some(TexGyre("qplbi"))
    ),
    record!(
        "Palatino-Italic",
        Palatino,
        false,
        true,
        None,
        Some(TexGyre("qplri"))
    ),
    record!(
        "Palatino-Roman",
        Palatino,
        false,
        false,
        None,
        Some(TexGyre("qplr"))
    ),
    record!("Symbol", Symbol, false, false, Some(StdFont::Symbol), None),
    record!(
        "Times-Bold",
        Times,
        true,
        false,
        Some(StdFont::TimesBold),
        Some(Liberation("LiberationSerif-Bold"))
    ),
    record!(
        "Times-BoldItalic",
        Times,
        true,
        true,
        Some(StdFont::TimesBoldItalic),
        Some(Liberation("LiberationSerif-BoldItalic"))
    ),
    record!(
        "Times-Italic",
        Times,
        false,
        true,
        Some(StdFont::TimesItalic),
        Some(Liberation("LiberationSerif-Italic"))
    ),
    record!(
        "Times-Roman",
        Times,
        false,
        false,
        Some(StdFont::TimesRoman),
        Some(Liberation("LiberationSerif-Regular"))
    ),
    record!(
        "ZapfChancery-MediumItalic",
        ZapfChancery,
        false,
        true,
        None,
        Some(TexGyre("qzcmi"))
    ),
    record!(
        "ZapfDingbats",
        ZapfDingbats,
        false,
        false,
        Some(StdFont::ZapfDingbats),
        None
    ),
];

impl ResidentFace {
    pub const COUNT: usize = 35;

    pub const ALL: [ResidentFace; 35] = [
        ResidentFace::AvantGardeBook,
        ResidentFace::AvantGardeBookOblique,
        ResidentFace::AvantGardeDemi,
        ResidentFace::AvantGardeDemiOblique,
        ResidentFace::BookmanDemi,
        ResidentFace::BookmanDemiItalic,
        ResidentFace::BookmanLight,
        ResidentFace::BookmanLightItalic,
        ResidentFace::Courier,
        ResidentFace::CourierBold,
        ResidentFace::CourierBoldOblique,
        ResidentFace::CourierOblique,
        ResidentFace::Helvetica,
        ResidentFace::HelveticaBold,
        ResidentFace::HelveticaBoldOblique,
        ResidentFace::HelveticaNarrow,
        ResidentFace::HelveticaNarrowBold,
        ResidentFace::HelveticaNarrowBoldOblique,
        ResidentFace::HelveticaNarrowOblique,
        ResidentFace::HelveticaOblique,
        ResidentFace::NewCenturySchlbkBold,
        ResidentFace::NewCenturySchlbkBoldItalic,
        ResidentFace::NewCenturySchlbkItalic,
        ResidentFace::NewCenturySchlbkRoman,
        ResidentFace::PalatinoBold,
        ResidentFace::PalatinoBoldItalic,
        ResidentFace::PalatinoItalic,
        ResidentFace::PalatinoRoman,
        ResidentFace::Symbol,
        ResidentFace::TimesBold,
        ResidentFace::TimesBoldItalic,
        ResidentFace::TimesItalic,
        ResidentFace::TimesRoman,
        ResidentFace::ZapfChanceryMediumItalic,
        ResidentFace::ZapfDingbats,
    ];

    fn record(self) -> &'static Record {
        &RECORDS[self as usize]
    }

    pub fn postscript_name(self) -> &'static str {
        self.record().name
    }

    /// The face with exactly this PostScript name.
    pub fn from_postscript_name(name: &[u8]) -> Option<ResidentFace> {
        ResidentFace::ALL
            .into_iter()
            .find(|f| f.postscript_name().as_bytes() == name)
    }

    /// Position in [`ResidentFace::ALL`]: the value of a resident font
    /// dictionary's marker entry.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<ResidentFace> {
        ResidentFace::ALL.get(index).copied()
    }

    /// The standard font this face is, for the fourteen.
    pub fn std_font(self) -> Option<StdFont> {
        self.record().std
    }

    pub fn family(self) -> Family {
        self.record().family
    }

    pub fn is_bold(self) -> bool {
        self.record().bold
    }

    pub fn is_italic(self) -> bool {
        self.record().italic
    }

    /// The member of `family` with the given weight and slope: Bookman's
    /// Light and Demi, AvantGarde's Book and Demi; Symbol, ZapfDingbats,
    /// and ZapfChancery have one face each.
    pub fn styled(family: Family, bold: bool, italic: bool) -> ResidentFace {
        let single = matches!(
            family,
            Family::Symbol | Family::ZapfDingbats | Family::ZapfChancery
        );
        ResidentFace::ALL
            .into_iter()
            .find(|f| {
                f.family() == family && (single || (f.is_bold() == bold && f.is_italic() == italic))
            })
            .expect("every family has every style it offers")
    }

    /// Whether the face's built-in encoding is its own rather than
    /// `StandardEncoding`.
    pub fn is_symbolic(self) -> bool {
        matches!(self, ResidentFace::Symbol | ResidentFace::ZapfDingbats)
    }

    /// The asset the face's outlines come from; none for Symbol and
    /// ZapfDingbats. Whether the asset is embedded in this build is
    /// [`crate::has_resident_outlines`].
    pub fn outline_asset(self) -> Option<OutlineAsset> {
        self.record().asset
    }

    fn source(self) -> &'static str {
        macro_rules! core14 {
            ($file:literal) => {
                include_str!(concat!("../data/core14/", $file, ".afm"))
            };
        }
        macro_rules! tex_gyre {
            ($file:literal) => {
                include_str!(concat!("../data/outlines/tex-gyre/", $file, ".afm"))
            };
        }
        match self {
            ResidentFace::AvantGardeBook => tex_gyre!("qagr"),
            ResidentFace::AvantGardeBookOblique => tex_gyre!("qagri"),
            ResidentFace::AvantGardeDemi => tex_gyre!("qagb"),
            ResidentFace::AvantGardeDemiOblique => tex_gyre!("qagbi"),
            ResidentFace::BookmanDemi => tex_gyre!("qbkb"),
            ResidentFace::BookmanDemiItalic => tex_gyre!("qbkbi"),
            ResidentFace::BookmanLight => tex_gyre!("qbkr"),
            ResidentFace::BookmanLightItalic => tex_gyre!("qbkri"),
            ResidentFace::Courier => core14!("Courier"),
            ResidentFace::CourierBold => core14!("Courier-Bold"),
            ResidentFace::CourierBoldOblique => core14!("Courier-BoldOblique"),
            ResidentFace::CourierOblique => core14!("Courier-Oblique"),
            ResidentFace::Helvetica => core14!("Helvetica"),
            ResidentFace::HelveticaBold => core14!("Helvetica-Bold"),
            ResidentFace::HelveticaBoldOblique => core14!("Helvetica-BoldOblique"),
            ResidentFace::HelveticaNarrow => tex_gyre!("qhvcr"),
            ResidentFace::HelveticaNarrowBold => tex_gyre!("qhvcb"),
            ResidentFace::HelveticaNarrowBoldOblique => tex_gyre!("qhvcbi"),
            ResidentFace::HelveticaNarrowOblique => tex_gyre!("qhvcri"),
            ResidentFace::HelveticaOblique => core14!("Helvetica-Oblique"),
            ResidentFace::NewCenturySchlbkBold => tex_gyre!("qcsb"),
            ResidentFace::NewCenturySchlbkBoldItalic => tex_gyre!("qcsbi"),
            ResidentFace::NewCenturySchlbkItalic => tex_gyre!("qcsri"),
            ResidentFace::NewCenturySchlbkRoman => tex_gyre!("qcsr"),
            ResidentFace::PalatinoBold => tex_gyre!("qplb"),
            ResidentFace::PalatinoBoldItalic => tex_gyre!("qplbi"),
            ResidentFace::PalatinoItalic => tex_gyre!("qplri"),
            ResidentFace::PalatinoRoman => tex_gyre!("qplr"),
            ResidentFace::Symbol => core14!("Symbol"),
            ResidentFace::TimesBold => core14!("Times-Bold"),
            ResidentFace::TimesBoldItalic => core14!("Times-BoldItalic"),
            ResidentFace::TimesItalic => core14!("Times-Italic"),
            ResidentFace::TimesRoman => core14!("Times-Roman"),
            ResidentFace::ZapfChanceryMediumItalic => tex_gyre!("qzcmi"),
            ResidentFace::ZapfDingbats => core14!("ZapfDingbats"),
        }
    }

    /// The face's metrics — the Core 14 AFM for the fourteen, the TeX
    /// Gyre AFM for the extras — parsed on first access.
    pub fn metrics(self) -> &'static Afm<'static> {
        static METRICS: [OnceLock<Afm<'static>>; ResidentFace::COUNT] =
            [const { OnceLock::new() }; ResidentFace::COUNT];
        METRICS[self.index()].get_or_init(|| {
            Afm::parse(self.source()).expect("the embedded AFM files are well formed")
        })
    }

    /// The advance width of the named glyph in thousandths of the em, if
    /// the face has the glyph.
    pub fn width(self, glyph: &str) -> Option<u16> {
        self.metrics().width(glyph)
    }

    /// The font bounding box `[llx lly urx ury]` in glyph units.
    pub fn bbox(self) -> [f32; 4] {
        self.metrics().font_bbox
    }

    /// The encoding the face has before a program re-encodes it:
    /// `StandardEncoding` for the text faces, the font's own for Symbol
    /// and ZapfDingbats.
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
            assert_eq!(font.face().std_font(), Some(font));
        }
        let mut names: Vec<_> = StdFont::ALL.iter().map(|f| f.postscript_name()).collect();
        let sorted = names.clone();
        names.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(StdFont::from_postscript_name(b"Arial"), None);
        assert_eq!(StdFont::from_postscript_name(b"Palatino-Roman"), None);
        assert_eq!(StdFont::from_index(14), None);
        assert_eq!(StdFont::styled(Family::Symbol, true, true), StdFont::Symbol);
        assert_eq!(
            StdFont::styled(Family::Palatino, true, false),
            StdFont::TimesBold
        );
        assert_eq!(
            StdFont::styled(Family::AvantGarde, false, true),
            StdFont::HelveticaOblique
        );
        assert_eq!(
            StdFont::styled(Family::ZapfChancery, true, false),
            StdFont::TimesItalic
        );
    }

    #[test]
    fn the_thirty_five_faces_are_sorted_and_styled() {
        assert_eq!(ResidentFace::ALL.len(), ResidentFace::COUNT);
        for (k, face) in ResidentFace::ALL.into_iter().enumerate() {
            assert_eq!(face.index(), k);
            assert_eq!(ResidentFace::from_index(k), Some(face));
            assert_eq!(
                ResidentFace::from_postscript_name(face.postscript_name().as_bytes()),
                Some(face)
            );
            assert_eq!(
                ResidentFace::styled(face.family(), face.is_bold(), face.is_italic()),
                face
            );
            assert_eq!(
                face.std_font().is_some(),
                StdFont::ALL.iter().any(|s| s.face() == face),
                "{}",
                face.postscript_name()
            );
            assert_eq!(face.outline_asset().is_none(), face.is_symbolic());
        }
        let mut names: Vec<_> = ResidentFace::ALL
            .iter()
            .map(|f| f.postscript_name())
            .collect();
        let sorted = names.clone();
        names.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(
            ResidentFace::ALL
                .iter()
                .filter(|f| f.std_font().is_some())
                .count(),
            14
        );
        assert_eq!(ResidentFace::from_index(35), None);
        assert_eq!(
            ResidentFace::styled(Family::Bookman, true, true),
            ResidentFace::BookmanDemiItalic
        );
        assert_eq!(
            ResidentFace::styled(Family::ZapfChancery, true, false),
            ResidentFace::ZapfChanceryMediumItalic
        );
        assert_eq!(
            ResidentFace::styled(Family::HelveticaNarrow, false, true),
            ResidentFace::HelveticaNarrowOblique
        );
        assert!(Family::Palatino.is_serif());
        assert!(!Family::AvantGarde.is_serif());
    }

    #[test]
    fn all_thirty_five_parse_with_their_own_names() {
        for face in ResidentFace::ALL {
            let afm = face.metrics();
            match face.std_font() {
                Some(font) => {
                    assert_eq!(afm.font_name, face.postscript_name());
                    assert_eq!(font.metrics().font_name, afm.font_name);
                    assert!(afm.chars().len() >= 188, "{}", face.postscript_name());
                    assert_eq!(
                        afm.encoding_scheme,
                        Some(if face.is_symbolic() {
                            "FontSpecific"
                        } else {
                            "AdobeStandardEncoding"
                        })
                    );
                }
                None => {
                    assert!(afm.font_name.starts_with("TeXGyre"), "{}", afm.font_name);
                    assert!(afm.chars().len() >= 800, "{}", face.postscript_name());
                    for name in STANDARD_ENCODING.iter().flatten() {
                        assert!(
                            afm.width(name).is_some(),
                            "{} lacks /{name}",
                            face.postscript_name()
                        );
                    }
                }
            }
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
        assert_eq!(ResidentFace::PalatinoRoman.width("a"), Some(500));
        assert_eq!(ResidentFace::TimesRoman.width("a"), Some(444));
        assert_eq!(
            ResidentFace::PalatinoRoman.builtin_encoding()[97],
            Some("a")
        );
        assert_eq!(
            ResidentFace::ZapfChanceryMediumItalic.bbox(),
            ResidentFace::ZapfChanceryMediumItalic.metrics().font_bbox
        );
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
