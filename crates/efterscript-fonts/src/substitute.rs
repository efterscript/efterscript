// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Name-level substitution: any font name resolves to one of the
//! thirty-five resident faces. Aliases cover the metrically compatible
//! families and the classic printer-resident families; anything else is
//! classified by hints in the name, Helvetica being the default.

use crate::resident::{Family, ResidentFace};

// Family aliases, compared case-insensitively against the family part of
// the name with spaces removed and `MT`, `PS`, and `PSMT` suffixes
// dropped.
const ALIASES: &[(&str, Family)] = &[
    ("courier", Family::Courier),
    ("couriernew", Family::Courier),
    ("helvetica", Family::Helvetica),
    ("arial", Family::Helvetica),
    ("helveticanarrow", Family::HelveticaNarrow),
    ("arialnarrow", Family::HelveticaNarrow),
    ("times", Family::Times),
    ("timesnewroman", Family::Times),
    ("timesroman", Family::Times),
    ("symbol", Family::Symbol),
    ("zapfdingbats", Family::ZapfDingbats),
    ("dingbats", Family::ZapfDingbats),
    ("palatino", Family::Palatino),
    ("bookantiqua", Family::Palatino),
    ("palladio", Family::Palatino),
    ("bookman", Family::Bookman),
    ("itcbookman", Family::Bookman),
    ("avantgarde", Family::AvantGarde),
    ("itcavantgarde", Family::AvantGarde),
    ("avantgardegothic", Family::AvantGarde),
    ("itcavantgardegothic", Family::AvantGarde),
    ("gothic", Family::AvantGarde),
    ("newcenturyschlbk", Family::NewCenturySchlbk),
    ("newcenturyschoolbook", Family::NewCenturySchlbk),
    ("centuryschoolbook", Family::NewCenturySchlbk),
    ("schoolbook", Family::NewCenturySchlbk),
    ("zapfchancery", Family::ZapfChancery),
    ("itczapfchancery", Family::ZapfChancery),
    ("chancery", Family::ZapfChancery),
    ("garamond", Family::Times),
    ("optima", Family::Helvetica),
    ("univers", Family::Helvetica),
];

/// The resident face a name stands for.
pub fn substitute(name: &[u8]) -> ResidentFace {
    let name = String::from_utf8_lossy(name);
    let name = strip_subset_tag(&name);
    if let Some(face) = ResidentFace::from_postscript_name(name.as_bytes()) {
        return face;
    }
    let lower = name.to_ascii_lowercase();
    let (family, style) = match lower.find(['-', ',']) {
        Some(at) => (&lower[..at], &lower[at + 1..]),
        None => (lower.as_str(), ""),
    };
    let family: String = family.chars().filter(|c| !c.is_whitespace()).collect();
    let family = trim_family(&family);
    let whole = lower.as_str();
    let mut class = ALIASES
        .iter()
        .find(|(alias, _)| *alias == family)
        .map(|&(_, class)| class)
        .unwrap_or_else(|| classify(whole));
    // `Helvetica-Narrow-…` and `Arial-Narrow…` split at the first dash.
    if class == Family::Helvetica && style.starts_with("narrow") {
        class = Family::HelveticaNarrow;
    }
    let bold = ["bold", "black", "heavy", "semibold", "demi"]
        .iter()
        .any(|hint| style.contains(hint) || (style.is_empty() && whole.contains(hint)));
    let italic = ["italic", "oblique"]
        .iter()
        .any(|hint| style.contains(hint) || (style.is_empty() && whole.contains(hint)));
    ResidentFace::styled(class, bold, italic)
}

/// Six upper-case letters and a plus sign, as an embedded subset carries.
fn strip_subset_tag(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() > 7 && bytes[6] == b'+' && bytes[..6].iter().all(u8::is_ascii_uppercase) {
        &name[7..]
    } else {
        name
    }
}

fn trim_family(family: &str) -> &str {
    ["psmt", "mt", "ps"]
        .iter()
        .find_map(|suffix| family.strip_suffix(suffix))
        .unwrap_or(family)
}

fn classify(name: &str) -> Family {
    if name.contains("symbol") {
        Family::Symbol
    } else if name.contains("dingbat") {
        Family::ZapfDingbats
    } else if ["mono", "courier", "typewriter", "console"]
        .iter()
        .any(|h| name.contains(h))
    {
        Family::Courier
    } else if name.contains("sans") {
        Family::Helvetica
    } else if ["serif", "roman", "times", "book", "georgia", "century"]
        .iter()
        .any(|h| name.contains(h))
    {
        Family::Times
    } else {
        Family::Helvetica
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(name: &str) -> ResidentFace {
        substitute(name.as_bytes())
    }

    #[test]
    fn exact_names_are_themselves() {
        for face in ResidentFace::ALL {
            assert_eq!(sub(face.postscript_name()), face);
        }
    }

    #[test]
    fn aliases_and_suffixes() {
        assert_eq!(sub("Arial-BoldMT"), ResidentFace::HelveticaBold);
        assert_eq!(sub("ArialMT"), ResidentFace::Helvetica);
        assert_eq!(sub("Arial,BoldItalic"), ResidentFace::HelveticaBoldOblique);
        assert_eq!(sub("TimesNewRomanPSMT"), ResidentFace::TimesRoman);
        assert_eq!(
            sub("TimesNewRomanPS-BoldItalicMT"),
            ResidentFace::TimesBoldItalic
        );
        assert_eq!(sub("Times New Roman"), ResidentFace::TimesRoman);
        assert_eq!(sub("CourierNewPS-BoldMT"), ResidentFace::CourierBold);
        assert_eq!(sub("CourierNew"), ResidentFace::Courier);
        assert_eq!(sub("Optima-Bold"), ResidentFace::HelveticaBold);
        assert_eq!(sub("Univers-Oblique"), ResidentFace::HelveticaOblique);
        assert_eq!(sub("Garamond"), ResidentFace::TimesRoman);
        assert_eq!(sub("Times-Roman"), ResidentFace::TimesRoman);
        assert_eq!(sub("Symbol"), ResidentFace::Symbol);
        assert_eq!(sub("Dingbats"), ResidentFace::ZapfDingbats);
    }

    #[test]
    fn classic_resident_families_resolve_to_their_own_faces() {
        assert_eq!(sub("Palatino-Roman"), ResidentFace::PalatinoRoman);
        assert_eq!(sub("Palatino"), ResidentFace::PalatinoRoman);
        assert_eq!(sub("Palatino-BoldItalic"), ResidentFace::PalatinoBoldItalic);
        assert_eq!(sub("BookAntiqua"), ResidentFace::PalatinoRoman);
        assert_eq!(sub("Book Antiqua,Bold"), ResidentFace::PalatinoBold);
        assert_eq!(sub("Palladio-Italic"), ResidentFace::PalatinoItalic);
        assert_eq!(sub("Bookman-Demi"), ResidentFace::BookmanDemi);
        assert_eq!(sub("Bookman-Light"), ResidentFace::BookmanLight);
        assert_eq!(
            sub("ITCBookman-LightItalic"),
            ResidentFace::BookmanLightItalic
        );
        assert_eq!(sub("Bookman-Bold"), ResidentFace::BookmanDemi);
        assert_eq!(sub("Bookman"), ResidentFace::BookmanLight);
        assert_eq!(sub("AvantGarde-Book"), ResidentFace::AvantGardeBook);
        assert_eq!(sub("AvantGarde-Demi"), ResidentFace::AvantGardeDemi);
        assert_eq!(
            sub("AvantGarde-BookOblique"),
            ResidentFace::AvantGardeBookOblique
        );
        assert_eq!(
            sub("ITC Avant Garde Gothic,BoldItalic"),
            ResidentFace::AvantGardeDemiOblique
        );
        assert_eq!(sub("Gothic-Bold"), ResidentFace::AvantGardeDemi);
        assert_eq!(
            sub("NewCenturySchlbk-BoldItalic"),
            ResidentFace::NewCenturySchlbkBoldItalic
        );
        assert_eq!(
            sub("NewCenturySchoolbook-Roman"),
            ResidentFace::NewCenturySchlbkRoman
        );
        assert_eq!(
            sub("Century Schoolbook,Italic"),
            ResidentFace::NewCenturySchlbkItalic
        );
        assert_eq!(sub("Schoolbook"), ResidentFace::NewCenturySchlbkRoman);
        assert_eq!(
            sub("ZapfChancery-MediumItalic"),
            ResidentFace::ZapfChanceryMediumItalic
        );
        assert_eq!(sub("Chancery"), ResidentFace::ZapfChanceryMediumItalic);
        assert_eq!(
            sub("ITCZapfChancery-Bold"),
            ResidentFace::ZapfChanceryMediumItalic
        );
        assert_eq!(
            sub("Helvetica-Narrow-Bold"),
            ResidentFace::HelveticaNarrowBold
        );
        assert_eq!(
            sub("Helvetica-Narrow-BoldItalic"),
            ResidentFace::HelveticaNarrowBoldOblique
        );
        assert_eq!(sub("ArialNarrow"), ResidentFace::HelveticaNarrow);
        assert_eq!(
            sub("Arial Narrow,Italic"),
            ResidentFace::HelveticaNarrowOblique
        );
        assert_eq!(sub("Arial-Narrow"), ResidentFace::HelveticaNarrow);
        assert_eq!(
            sub("HelveticaNarrow-Oblique"),
            ResidentFace::HelveticaNarrowOblique
        );
    }

    #[test]
    fn heuristics_and_subset_tags() {
        assert_eq!(sub("Garamond-Italic"), ResidentFace::TimesItalic);
        assert_eq!(sub("LucidaConsole"), ResidentFace::Courier);
        assert_eq!(sub("ABCDEF+LucidaConsole-Bold"), ResidentFace::CourierBold);
        assert_eq!(sub("Verdana"), ResidentFace::Helvetica);
        assert_eq!(sub("Georgia-BoldItalic"), ResidentFace::TimesBoldItalic);
        assert_eq!(sub("DejaVuSansMono-Oblique"), ResidentFace::CourierOblique);
        assert_eq!(sub("DejaVuSerif"), ResidentFace::TimesRoman);
        assert_eq!(sub("OpenSans-Semibold"), ResidentFace::HelveticaBold);
        assert_eq!(sub("MinionPro-Regular"), ResidentFace::Helvetica);
        assert_eq!(sub("SymbolMT"), ResidentFace::Symbol);
        assert_eq!(sub("Wingdings-Regular"), ResidentFace::Helvetica);
        assert_eq!(sub("MyDingbatFont"), ResidentFace::ZapfDingbats);
        assert_eq!(sub("abcdef+Foo"), ResidentFace::Helvetica);
        assert_eq!(sub(""), ResidentFace::Helvetica);
        assert_eq!(sub("Century-Book"), ResidentFace::TimesRoman);
        assert_eq!(sub("HeavyMetal"), ResidentFace::HelveticaBold);
        assert_eq!(sub("Arial-Black"), ResidentFace::HelveticaBold);
    }
}
