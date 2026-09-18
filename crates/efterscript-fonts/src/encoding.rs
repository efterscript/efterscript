// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The two built-in text encodings of the PostScript language (PLRM3
//! Appendix E): glyph names by code, `None` where a code is unassigned.

/// A 256-entry encoding vector.
pub type Encoding = [Option<&'static str>; 256];

const fn table(pairs: &[(u8, &'static str)]) -> Encoding {
    let mut out = [None; 256];
    let mut k = 0;
    while k < pairs.len() {
        out[pairs[k].0 as usize] = Some(pairs[k].1);
        k += 1;
    }
    out
}

// The printable ASCII range, shared by both encodings: codes 32 to 126
// with the typographic quotes at 39 and 96.
const ASCII: [(u8, &str); 95] = [
    (32, "space"),
    (33, "exclam"),
    (34, "quotedbl"),
    (35, "numbersign"),
    (36, "dollar"),
    (37, "percent"),
    (38, "ampersand"),
    (39, "quoteright"),
    (40, "parenleft"),
    (41, "parenright"),
    (42, "asterisk"),
    (43, "plus"),
    (44, "comma"),
    (45, "hyphen"),
    (46, "period"),
    (47, "slash"),
    (48, "zero"),
    (49, "one"),
    (50, "two"),
    (51, "three"),
    (52, "four"),
    (53, "five"),
    (54, "six"),
    (55, "seven"),
    (56, "eight"),
    (57, "nine"),
    (58, "colon"),
    (59, "semicolon"),
    (60, "less"),
    (61, "equal"),
    (62, "greater"),
    (63, "question"),
    (64, "at"),
    (65, "A"),
    (66, "B"),
    (67, "C"),
    (68, "D"),
    (69, "E"),
    (70, "F"),
    (71, "G"),
    (72, "H"),
    (73, "I"),
    (74, "J"),
    (75, "K"),
    (76, "L"),
    (77, "M"),
    (78, "N"),
    (79, "O"),
    (80, "P"),
    (81, "Q"),
    (82, "R"),
    (83, "S"),
    (84, "T"),
    (85, "U"),
    (86, "V"),
    (87, "W"),
    (88, "X"),
    (89, "Y"),
    (90, "Z"),
    (91, "bracketleft"),
    (92, "backslash"),
    (93, "bracketright"),
    (94, "asciicircum"),
    (95, "underscore"),
    (96, "quoteleft"),
    (97, "a"),
    (98, "b"),
    (99, "c"),
    (100, "d"),
    (101, "e"),
    (102, "f"),
    (103, "g"),
    (104, "h"),
    (105, "i"),
    (106, "j"),
    (107, "k"),
    (108, "l"),
    (109, "m"),
    (110, "n"),
    (111, "o"),
    (112, "p"),
    (113, "q"),
    (114, "r"),
    (115, "s"),
    (116, "t"),
    (117, "u"),
    (118, "v"),
    (119, "w"),
    (120, "x"),
    (121, "y"),
    (122, "z"),
    (123, "braceleft"),
    (124, "bar"),
    (125, "braceright"),
    (126, "asciitilde"),
];

const STANDARD_UPPER: [(u8, &str); 54] = [
    (161, "exclamdown"),
    (162, "cent"),
    (163, "sterling"),
    (164, "fraction"),
    (165, "yen"),
    (166, "florin"),
    (167, "section"),
    (168, "currency"),
    (169, "quotesingle"),
    (170, "quotedblleft"),
    (171, "guillemotleft"),
    (172, "guilsinglleft"),
    (173, "guilsinglright"),
    (174, "fi"),
    (175, "fl"),
    (177, "endash"),
    (178, "dagger"),
    (179, "daggerdbl"),
    (180, "periodcentered"),
    (182, "paragraph"),
    (183, "bullet"),
    (184, "quotesinglbase"),
    (185, "quotedblbase"),
    (186, "quotedblright"),
    (187, "guillemotright"),
    (188, "ellipsis"),
    (189, "perthousand"),
    (191, "questiondown"),
    (193, "grave"),
    (194, "acute"),
    (195, "circumflex"),
    (196, "tilde"),
    (197, "macron"),
    (198, "breve"),
    (199, "dotaccent"),
    (200, "dieresis"),
    (202, "ring"),
    (203, "cedilla"),
    (205, "hungarumlaut"),
    (206, "ogonek"),
    (207, "caron"),
    (208, "emdash"),
    (225, "AE"),
    (227, "ordfeminine"),
    (232, "Lslash"),
    (233, "Oslash"),
    (234, "OE"),
    (235, "ordmasculine"),
    (241, "ae"),
    (245, "dotlessi"),
    (248, "lslash"),
    (249, "oslash"),
    (250, "oe"),
    (251, "germandbls"),
];

const LATIN1_UPPER: [(u8, &str); 110] = [
    (144, "dotlessi"),
    (145, "grave"),
    (146, "acute"),
    (147, "circumflex"),
    (148, "tilde"),
    (149, "macron"),
    (150, "breve"),
    (151, "dotaccent"),
    (152, "dieresis"),
    (154, "ring"),
    (155, "cedilla"),
    (157, "hungarumlaut"),
    (158, "ogonek"),
    (159, "caron"),
    (160, "space"),
    (161, "exclamdown"),
    (162, "cent"),
    (163, "sterling"),
    (164, "currency"),
    (165, "yen"),
    (166, "brokenbar"),
    (167, "section"),
    (168, "dieresis"),
    (169, "copyright"),
    (170, "ordfeminine"),
    (171, "guillemotleft"),
    (172, "logicalnot"),
    (173, "hyphen"),
    (174, "registered"),
    (175, "macron"),
    (176, "degree"),
    (177, "plusminus"),
    (178, "twosuperior"),
    (179, "threesuperior"),
    (180, "acute"),
    (181, "mu"),
    (182, "paragraph"),
    (183, "periodcentered"),
    (184, "cedilla"),
    (185, "onesuperior"),
    (186, "ordmasculine"),
    (187, "guillemotright"),
    (188, "onequarter"),
    (189, "onehalf"),
    (190, "threequarters"),
    (191, "questiondown"),
    (192, "Agrave"),
    (193, "Aacute"),
    (194, "Acircumflex"),
    (195, "Atilde"),
    (196, "Adieresis"),
    (197, "Aring"),
    (198, "AE"),
    (199, "Ccedilla"),
    (200, "Egrave"),
    (201, "Eacute"),
    (202, "Ecircumflex"),
    (203, "Edieresis"),
    (204, "Igrave"),
    (205, "Iacute"),
    (206, "Icircumflex"),
    (207, "Idieresis"),
    (208, "Eth"),
    (209, "Ntilde"),
    (210, "Ograve"),
    (211, "Oacute"),
    (212, "Ocircumflex"),
    (213, "Otilde"),
    (214, "Odieresis"),
    (215, "multiply"),
    (216, "Oslash"),
    (217, "Ugrave"),
    (218, "Uacute"),
    (219, "Ucircumflex"),
    (220, "Udieresis"),
    (221, "Yacute"),
    (222, "Thorn"),
    (223, "germandbls"),
    (224, "agrave"),
    (225, "aacute"),
    (226, "acircumflex"),
    (227, "atilde"),
    (228, "adieresis"),
    (229, "aring"),
    (230, "ae"),
    (231, "ccedilla"),
    (232, "egrave"),
    (233, "eacute"),
    (234, "ecircumflex"),
    (235, "edieresis"),
    (236, "igrave"),
    (237, "iacute"),
    (238, "icircumflex"),
    (239, "idieresis"),
    (240, "eth"),
    (241, "ntilde"),
    (242, "ograve"),
    (243, "oacute"),
    (244, "ocircumflex"),
    (245, "otilde"),
    (246, "odieresis"),
    (247, "divide"),
    (248, "oslash"),
    (249, "ugrave"),
    (250, "uacute"),
    (251, "ucircumflex"),
    (252, "udieresis"),
    (253, "yacute"),
    (254, "thorn"),
    (255, "ydieresis"),
];

const fn merged(upper: &[(u8, &'static str)]) -> Encoding {
    let mut out = table(&ASCII);
    let mut k = 0;
    while k < upper.len() {
        out[upper[k].0 as usize] = Some(upper[k].1);
        k += 1;
    }
    out
}

/// `StandardEncoding`: the built-in encoding of the twelve text fonts.
pub static STANDARD_ENCODING: Encoding = merged(&STANDARD_UPPER);

/// `ISOLatin1Encoding`.
pub static ISO_LATIN1_ENCODING: Encoding = merged(&LATIN1_UPPER);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_codes() {
        assert_eq!(STANDARD_ENCODING[65], Some("A"));
        assert_eq!(STANDARD_ENCODING[39], Some("quoteright"));
        assert_eq!(STANDARD_ENCODING[96], Some("quoteleft"));
        assert_eq!(STANDARD_ENCODING[174], Some("fi"));
        assert_eq!(STANDARD_ENCODING[251], Some("germandbls"));
        assert_eq!(STANDARD_ENCODING[233], Some("Oslash"));
        assert_eq!(STANDARD_ENCODING[0], None);
        assert_eq!(STANDARD_ENCODING[127], None);
        assert_eq!(STANDARD_ENCODING[160], None);
        assert_eq!(STANDARD_ENCODING[255], None);
        assert_eq!(STANDARD_ENCODING.iter().flatten().count(), 149);

        assert_eq!(ISO_LATIN1_ENCODING[65], Some("A"));
        assert_eq!(ISO_LATIN1_ENCODING[233], Some("eacute"));
        assert_eq!(ISO_LATIN1_ENCODING[160], Some("space"));
        assert_eq!(ISO_LATIN1_ENCODING[173], Some("hyphen"));
        assert_eq!(ISO_LATIN1_ENCODING[255], Some("ydieresis"));
        assert_eq!(ISO_LATIN1_ENCODING[128], None);
        assert_eq!(ISO_LATIN1_ENCODING[153], None);
        assert_eq!(ISO_LATIN1_ENCODING.iter().flatten().count(), 205);
    }
}
