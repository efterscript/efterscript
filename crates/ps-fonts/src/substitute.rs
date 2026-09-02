// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Name-level substitution: any font name resolves to one of the standard
//! fourteen. Aliases cover the metrically compatible families and the
//! classic LaserWriter families by class; anything else is classified by
//! hints in the name, Helvetica being the default.

use crate::resident::{Family, StdFont};

// Family aliases, compared case-insensitively against the family part of
// the name with `MT`, `PS`, and `PSMT` suffixes removed.
const ALIASES: &[(&str, Family)] = &[
    ("courier", Family::Courier),
    ("couriernew", Family::Courier),
    ("helvetica", Family::Helvetica),
    ("arial", Family::Helvetica),
    ("arialnarrow", Family::Helvetica),
    ("times", Family::Times),
    ("timesnewroman", Family::Times),
    ("timesroman", Family::Times),
    ("symbol", Family::Symbol),
    ("zapfdingbats", Family::ZapfDingbats),
    ("dingbats", Family::ZapfDingbats),
    ("palatino", Family::Times),
    ("bookman", Family::Times),
    ("newcenturyschlbk", Family::Times),
    ("newcenturyschoolbook", Family::Times),
    ("garamond", Family::Times),
    ("avantgarde", Family::Helvetica),
    ("optima", Family::Helvetica),
    ("univers", Family::Helvetica),
];

/// The standard font a name stands for.
pub fn substitute(name: &[u8]) -> StdFont {
    let name = String::from_utf8_lossy(name);
    let name = strip_subset_tag(&name);
    if let Some(font) = StdFont::from_postscript_name(name.as_bytes()) {
        return font;
    }
    let lower = name.to_ascii_lowercase();
    let (family, style) = match lower.find(['-', ',']) {
        Some(at) => (&lower[..at], &lower[at + 1..]),
        None => (lower.as_str(), ""),
    };
    let family = trim_family(family);
    let whole = lower.as_str();
    if family == "zapfchancery" {
        return StdFont::TimesItalic;
    }
    let class = ALIASES
        .iter()
        .find(|(alias, _)| *alias == family)
        .map(|&(_, class)| class)
        .unwrap_or_else(|| classify(whole));
    let bold = ["bold", "black", "heavy", "semibold", "demi"]
        .iter()
        .any(|hint| style.contains(hint) || (style.is_empty() && whole.contains(hint)));
    let italic = ["italic", "oblique"]
        .iter()
        .any(|hint| style.contains(hint) || (style.is_empty() && whole.contains(hint)));
    StdFont::styled(class, bold, italic)
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

    fn sub(name: &str) -> StdFont {
        substitute(name.as_bytes())
    }

    #[test]
    fn exact_names_are_themselves() {
        for font in StdFont::ALL {
            assert_eq!(sub(font.postscript_name()), font);
        }
    }

    #[test]
    fn aliases_and_suffixes() {
        assert_eq!(sub("Arial-BoldMT"), StdFont::HelveticaBold);
        assert_eq!(sub("ArialMT"), StdFont::Helvetica);
        assert_eq!(sub("Arial,BoldItalic"), StdFont::HelveticaBoldOblique);
        assert_eq!(sub("TimesNewRomanPSMT"), StdFont::TimesRoman);
        assert_eq!(
            sub("TimesNewRomanPS-BoldItalicMT"),
            StdFont::TimesBoldItalic
        );
        assert_eq!(sub("CourierNewPS-BoldMT"), StdFont::CourierBold);
        assert_eq!(sub("CourierNew"), StdFont::Courier);
        assert_eq!(sub("Palatino-Roman"), StdFont::TimesRoman);
        assert_eq!(sub("Bookman-Demi"), StdFont::TimesBold);
        assert_eq!(sub("NewCenturySchlbk-BoldItalic"), StdFont::TimesBoldItalic);
        assert_eq!(sub("AvantGarde-Book"), StdFont::Helvetica);
        assert_eq!(sub("Optima-Bold"), StdFont::HelveticaBold);
        assert_eq!(sub("Univers-Oblique"), StdFont::HelveticaOblique);
        assert_eq!(sub("ZapfChancery-MediumItalic"), StdFont::TimesItalic);
        assert_eq!(sub("Helvetica-Narrow-Bold"), StdFont::HelveticaBold);
        assert_eq!(sub("Times-Roman"), StdFont::TimesRoman);
        assert_eq!(sub("Symbol"), StdFont::Symbol);
        assert_eq!(sub("Dingbats"), StdFont::ZapfDingbats);
    }

    #[test]
    fn heuristics_and_subset_tags() {
        assert_eq!(sub("Garamond-Italic"), StdFont::TimesItalic);
        assert_eq!(sub("LucidaConsole"), StdFont::Courier);
        assert_eq!(sub("ABCDEF+LucidaConsole-Bold"), StdFont::CourierBold);
        assert_eq!(sub("Verdana"), StdFont::Helvetica);
        assert_eq!(sub("Georgia-BoldItalic"), StdFont::TimesBoldItalic);
        assert_eq!(sub("DejaVuSansMono-Oblique"), StdFont::CourierOblique);
        assert_eq!(sub("DejaVuSerif"), StdFont::TimesRoman);
        assert_eq!(sub("OpenSans-Semibold"), StdFont::HelveticaBold);
        assert_eq!(sub("MinionPro-Regular"), StdFont::Helvetica);
        assert_eq!(sub("SymbolMT"), StdFont::Symbol);
        assert_eq!(sub("Wingdings-Regular"), StdFont::Helvetica);
        assert_eq!(sub("MyDingbatFont"), StdFont::ZapfDingbats);
        assert_eq!(sub("abcdef+Foo"), StdFont::Helvetica);
        assert_eq!(sub(""), StdFont::Helvetica);
        assert_eq!(sub("Century-Book"), StdFont::TimesRoman);
        assert_eq!(sub("HeavyMetal"), StdFont::HelveticaBold);
        assert_eq!(sub("Arial-Black"), StdFont::HelveticaBold);
    }
}
