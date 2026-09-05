// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The corpus files under `corpus/unit/fonts` that carry synthesised
//! font programs. `generate_corpus_fonts` (ignored) writes them; the
//! committed files are the project's own work, and
//! `corpus_fonts_are_current` checks they match what the generator would
//! write, so a change to the builders shows up as a test failure rather
//! than a silent drift.

use std::path::PathBuf;

use ps_fonts::testing::{
    CffFont, CidType1Font, TRAILER, TrueTypeFont, Type1Font, corpus_cff, corpus_cid_cff,
    corpus_cmap, corpus_truetype, corpus_type1, eexec_binary, eexec_hex,
};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("unit")
        .join("fonts")
}

fn syn() -> Type1Font {
    corpus_type1()
}

fn syn_tt() -> TrueTypeFont {
    corpus_truetype()
}

fn syn_cff() -> CffFont {
    corpus_cff()
}

fn header(expect: &[&str], scenario: &str) -> String {
    header_with("%!PS", expect, scenario)
}

/// A FontSet resource file's header: the DSC resource header line, the
/// declarations, and the scenario.
fn font_set_header(expect: &[&str], scenario: &str) -> String {
    header_with("%!PS-Adobe-3.0 Resource-FontSet", expect, scenario)
}

fn header_with(first: &str, expect: &[&str], scenario: &str) -> String {
    let mut out = format!(
        "{first}\n\
         % SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
         % SPDX-License-Identifier: MIT\n",
    );
    for line in expect {
        out.push_str(&format!("% expect-output: {line}\n"));
    }
    out.push_str(scenario);
    out.push_str(
        "% The font program below is synthesised by ps-fonts' corpus\n\
         % generator (tests/corpus_fonts.rs); regenerate rather than edit.\n",
    );
    out
}

const ROUND: &str = "/r { 1000 mul round 1000 div } def\n";

/// Every generated file as `(name, content)`.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    let syn = syn().pfa();
    let syn_tt = syn_tt().type42("SynTT", &[(97, "a"), (111, "o")]);
    let mut eexec_hex_file = header(
        &["42"],
        "% Scenario: a hexadecimal eexec section. The section decrypts to\n\
         % `userdict /x 42 put mark currentfile closefile`; closefile ends the\n\
         % layer, and the zeros and cleartomark that follow run in the clear.\n\
         % systemdict is on the dictionary stack inside the section and is\n\
         % read-only, so the definition goes through userdict explicitly.\n\
         % Expect: 42.\n",
    )
    .into_bytes();
    eexec_hex_file.extend_from_slice(b"currentfile eexec\n");
    eexec_hex_file.extend_from_slice(
        eexec_hex(b"userdict /x 42 put mark currentfile closefile\n").as_bytes(),
    );
    eexec_hex_file.extend_from_slice(TRAILER.as_bytes());
    eexec_hex_file.extend_from_slice(b"x =\n");

    let mut eexec_string_file = header(
        &["hi"],
        "% Scenario: eexec on a string. The string holds the binary encryption\n\
         % of `(hi) print`; the newline is printed afterwards so the output is\n\
         % a line.\n\
         % Expect: hi.\n",
    )
    .into_bytes();
    eexec_string_file.push(b'<');
    for byte in eexec_binary(b"(hi) print") {
        eexec_string_file.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    eexec_string_file.extend_from_slice(b"> eexec\n(\\n) print\n");

    let type1_width = format!(
        "{}{syn}{ROUND}/Syn findfont 10 scalefont setfont\n(aa) stringwidth r exch r = =\n",
        header(
            &["12.0", "0.0"],
            "% Scenario: width from the charstring. The synthesised Type 1 font\n\
             % Syn defines glyph a with advance 600 in a 1000-unit em through a\n\
             % hexadecimal eexec program; at size 10, (aa) measures 12.\n\
             % Expect: 12.0 and 0.0.\n",
        )
    );

    let seac_advance = format!(
        "{}{syn}{ROUND}/Syn findfont 10 scalefont setfont\n(\\351) stringwidth r exch r = =\n",
        header(
            &["5.0", "0.0"],
            "% Scenario: a composed glyph. eacute (code 233) is defined by seac\n\
             % from e and acute; its advance is the composite's own hsbw width,\n\
             % 500, so at size 10 it measures 5.\n\
             % Expect: 5.0 and 0.0.\n",
        )
    );

    let malformed = format!(
        "{}{syn}/Syn findfont 10 scalefont setfont\n0 0 moveto (b) show\n",
        header(
            &[],
            "% expect-error: invalidfont\n\
             % divergence: malformed-font-invalidfont\n\
             % Scenario: a malformed charstring. Glyph b ends after a number,\n\
             % with no operator to consume it; show raises invalidfont when it\n\
             % needs the glyph.\n\
             % Expect: invalidfont.\n",
        )
    );

    let type42_advance = format!(
        "{}{syn_tt}{ROUND}/SynTT findfont 20 scalefont setfont\n(a) stringwidth r exch r = =\n",
        header(
            &["10.0", "0.0"],
            "% Scenario: advance from the metrics. The synthesised TrueType font\n\
             % SynTT has 2048 units per em and glyph a advances 1024; wrapped\n\
             % as Type 42 with the identity FontMatrix, glyph space is the unit\n\
             % em, so at size 20 the advance is 10.\n\
             % Expect: 10.0 and 0.0.\n",
        )
    );

    let charpath_advance = format!(
        "{}{syn}{ROUND}/Syn findfont 10 scalefont setfont\n\
         0 0 moveto (aa) false charpath currentpoint r exch r = =\n",
        header(
            &["12.0", "0.0"],
            "% Scenario: charpath advances. The outlines of (aa) join the current\n\
             % path and the current point moves by the two advances, as show\n\
             % would move it.\n\
             % Expect: 12.0 and 0.0.\n",
        )
    );

    // --- page-producing scenarios, with .ir and .pdf goldens -----------------

    let charpath_fill = format!(
        "{}{syn}/Syn findfont 10 scalefont setfont\n\
         100 100 moveto (a) false charpath fill\nshowpage\n",
        header(
            &[],
            "% Scenario: filling a charpath. The square glyph's outline joins the\n\
             % path at (100, 100) scaled by the size; the fill paints it as one\n\
             % path operation, with the trailing single-point subpath charpath\n\
             % leaves at the end of the run, and no text operation is recorded.\n",
        )
    );

    let seac_glyphshow = format!(
        "{}{syn}/Syn findfont 10 scalefont setfont\n\
         0 0 moveto /eacute glyphshow\n\
         100 100 moveto (\\351) false charpath fill\nshowpage\n",
        header(
            &[],
            "% Scenario: a composed glyph. eacute is defined by seac from e and\n\
             % acute: glyphshow records it under its code with the composite's\n\
             % advance, 500, and charpath of the same glyph yields both\n\
             % components' outlines, the accent displaced by the seac offsets.\n",
        )
    );

    let type42_bbox = format!(
        "{}{syn_tt}{ROUND}/SynTT findfont [20 0 0 20 0 -5] makefont setfont\n\
         0 0 moveto (o) true charpath\n\
         pathbbox 4 {{ 4 -1 roll r }} repeat\n\
         4 -1 roll = 3 -1 roll = exch = =\n\
         fill showpage\n",
        header(
            &["0.0", "-3.372", "11.719", "3.138"],
            "% Scenario: outline conversion. Glyph o is one quadratic contour\n\
             % whose control box is 100..1100 by 0..1000 font units; converted to\n\
             % cubics its control points sit at two thirds of the way to the\n\
             % quadratic control, so the path's extent in y is 166.67 to 833.33\n\
             % units, within the quadratic box: at 20/2048 per unit, shifted down\n\
             % by 5 so the run's start and end points lie inside, -3.372 to 3.138.\n\
             % In x the box spans the run, from the start point to the trailing\n\
             % point charpath leaves at the advance, since pathbbox counts both;\n\
             % the .ir golden pins the outline itself.\n\
             % Expect: 0.0, -3.372, 11.719, 3.138.\n",
        )
    );

    let dump = format!(
        "{}{syn}/Syn findfont 12 scalefont setfont\n72 700 moveto (a) show\nshowpage\n",
        header(
            &[],
            "% Scenario: an embedded font in the dump, and showing an embedded\n\
             % font. Syn shown once: the page holds one text operation over an\n\
             % embedded-font resource, listed as `font 0 embedded type1 Syn` with\n\
             % its glyph count and encoding and never its program bytes.\n",
        )
    );

    let round_trip = format!(
        "{}{syn}/Syn findfont 10 scalefont setfont\n100 100 moveto (a) show\nshowpage\n",
        header(
            &[],
            "% Scenario: a Type 1 subset round-trips. Only a is shown, so the\n\
             % embedded FontFile defines exactly .notdef and a; the remelt test\n\
             % suite extracts it and runs it through the interpreter.\n",
        )
    );

    let two_pages = format!(
        "{}{syn}/Syn findfont 10 scalefont setfont\n100 100 moveto (a) show\nshowpage\n\
         /Syn findfont 20 scalefont setfont\n100 200 moveto (e) show\nshowpage\n",
        header(
            &[],
            "% Scenario: Type 1 embedded. The font is shown on two pages with\n\
             % different glyphs; the document holds one font dictionary whose\n\
             % FontFile carries Length1, Length2, and Length3 and defines exactly\n\
             % a, e, and .notdef, and both pages reference it.\n",
        )
    );

    let syn_tt_upper = corpus_truetype().type42("SynTT", &[(65, "a"), (66, "o")]);
    let truetype_cmap = format!(
        "{}{syn_tt_upper}/SynTT findfont 20 scalefont setfont\n100 100 moveto (AB) show\nshowpage\n",
        header(
            &[],
            "% Scenario: a TrueType subset has a cmap. Codes 65 and 66 are shown,\n\
             % so the embedded FontFile2 holds the two glyphs plus the notdef and\n\
             % a (3,0) cmap mapping 65 and 66, and the font dictionary lists\n\
             % widths for those codes.\n",
        )
    );

    let truetype_embedded = format!(
        "{}{syn_tt}/SynTT findfont 20 scalefont setfont\n100 100 moveto (ao) show\nshowpage\n",
        header(
            &[],
            "% Scenario: TrueType embedded. The synthesised TrueType font shown\n\
             % once becomes a /TrueType font dictionary whose descriptor embeds\n\
             % the subset program as FontFile2 with the symbolic flag, and the\n\
             % content stream shows the codes.\n",
        )
    );

    // --- FontSet files: a binary CFF program after StartData ----------------

    let set = syn_cff().font_set("SynSet");
    let mut cff_width = font_set_header(
        &["6.0", "0.0"],
        "% Scenario: width from a Type 2 charstring. The FontSet SynSet holds\n\
         % the synthesised CFF font SynCFF, whose private dictionary has\n\
         % nominal width 500 and whose glyph a encodes a width delta of 100;\n\
         % StartData reads the binary program that follows it from this file\n\
         % and defines SynCFF as a FontType 2 font. At size 10, (a) measures 6.\n\
         % Expect: 6.0 and 0.0.\n",
    )
    .into_bytes();
    cff_width.extend_from_slice(&set);
    cff_width.extend_from_slice(
        format!("{ROUND}/SynCFF findfont 10 scalefont setfont\n(a) stringwidth r exch r = =\n")
            .as_bytes(),
    );

    let mut cff_bbox = font_set_header(
        &["0.0", "-0.5", "6.0", "2.0"],
        "% Scenario: flex and hint mask. Glyph f declares a horizontal stem,\n\
         % a vertical stem through the implicit vstem before its hintmask,\n\
         % and draws an hflex whose two curves rise to 200 units, then two\n\
         % lines down to -50; its control box is 0..600 by -50..200, which at\n\
         % size 10 is 0..6 by -0.5..2. The run's start and the trailing point\n\
         % charpath leaves at the advance both lie inside that box.\n\
         % Expect: 0.0, -0.5, 6.0, 2.0.\n",
    )
    .into_bytes();
    cff_bbox.extend_from_slice(&set);
    cff_bbox.extend_from_slice(
        format!(
            "{ROUND}/SynCFF findfont 10 scalefont setfont\n\
             0 0 moveto (f) true charpath\n\
             pathbbox 4 {{ 4 -1 roll r }} repeat\n\
             4 -1 roll = 3 -1 roll = exch = =\n\
             fill showpage\n"
        )
        .as_bytes(),
    );

    let mut fontset_defines = font_set_header(
        &["true", "2"],
        "% Scenario: a FontSet defines its fonts. After StartData the FontSet\n\
         % resource SynSet exists and its one font, SynCFF, is a FontType 2\n\
         % font in FontDirectory.\n\
         % Expect: true and 2.\n",
    )
    .into_bytes();
    fontset_defines.extend_from_slice(&set);
    fontset_defines.extend_from_slice(
        b"/SynSet /FontSet resourcestatus { pop pop true } { false } ifelse =\n\
          /SynCFF findfont /FontType get =\n",
    );

    let mut dict_stack = font_set_header(
        &["true"],
        "% Scenario: the dictionary stack is restored. countdictstack before\n\
         % the procedure set's begin and after StartData has consumed the\n\
         % data agree: StartData ends the dictionary the begin pushed, and\n\
         % the file, in its canonical form, carries no end of its own.\n\
         % Expect: true.\n",
    )
    .into_bytes();
    dict_stack.extend_from_slice(b"countdictstack\n");
    dict_stack.extend_from_slice(&set);
    dict_stack.extend_from_slice(b"countdictstack eq =\n");

    let mut cff_type1c = font_set_header(
        &[],
        "% Scenario: Type1C embedded. SynCFF is shown once with only a, so\n\
         % the document holds one /Type1 font dictionary whose descriptor's\n\
         % FontFile3 stream has /Subtype /Type1C and carries a CFF subset\n\
         % defining exactly .notdef and a with no subroutines, since a\n\
         % reaches none; the remelt test suite parses it back with the\n\
         % engine and compares the outline and advance with the original's.\n",
    )
    .into_bytes();
    cff_type1c.extend_from_slice(&set);
    cff_type1c.extend_from_slice(
        b"/SynCFF findfont 10 scalefont setfont\n100 100 moveto (a) show\nshowpage\n",
    );

    let data = syn_cff().build();
    let mut short_data = font_set_header(
        &[],
        "% expect-error: invalidfont\n\
         % divergence: malformed-font-invalidfont\n\
         % Scenario: short data. StartData declares more bytes than the file\n\
         % holds after it, so the read ends early and the error is\n\
         % invalidfont, with nothing defined.\n\
         % Expect: invalidfont.\n",
    )
    .into_bytes();
    short_data.extend_from_slice(
        format!(
            "/FontSetInit /ProcSet findresource begin\n/Short {} StartData\n",
            data.len() + 100
        )
        .as_bytes(),
    );
    short_data.extend_from_slice(&data);

    // --- CMaps, CIDFonts, and composite text -----------------------------------

    let cmap_embedded = format!(
        "{}{}/Syn-H /CMap resourcestatus = = =\n",
        header(
            &["true", "0", "0"],
            "% divergence: resource-size-unknown\n\
             % Scenario: an embedded CMap. The CMap program Syn-H, written in the\n\
             % CIDInit operators, maps one-byte codes <20>-<7E> to CIDs from 1 and\n\
             % two-byte codes <8140>-<817E> to CIDs from 200, and defines itself\n\
             % as a CMap resource; resourcestatus finds it with status 0.\n\
             % Expect: true, 0, 0.\n",
        ),
        corpus_cmap()
    );

    let cid_set = corpus_cid_cff().font_set("SynCIDSet");
    let mut cidfont_fontset = font_set_header(
        &["true", "0", "0", "201"],
        "% divergence: resource-size-unknown\n\
         % Scenario: a CID-keyed CFF from a FontSet. SynCIDSet holds the\n\
         % CID-keyed CFF SynCID, whose CIDs 1 and 2 lie in different font\n\
         % dictionaries; StartData defines it as a CIDFontType 0 resource of\n\
         % the CIDFont category with the program's CIDCount, 201.\n\
         % Expect: true, 0, 0, 201.\n",
    )
    .into_bytes();
    cidfont_fontset.extend_from_slice(&cid_set);
    cidfont_fontset.extend_from_slice(
        b"/SynCID /CIDFont resourcestatus = = =\n\
          /SynCID /CIDFont findresource /CIDCount get =\n",
    );

    // The resource files carry their own DSC header line, which the
    // corpus header already supplies.
    let without_header = |text: &str| {
        text.split_once('\n')
            .map_or(text, |(_, rest)| rest)
            .to_string()
    };
    let cid_tt = without_header(&corpus_truetype().cidfont_type2("SynCIDTT", &[(3, 1)]));
    let cidfont_type2 = format!(
        "{}{cid_tt}{ROUND}/SynTTComposite /Identity-H [ /SynCIDTT /CIDFont findresource ] \
         composefont 20 scalefont setfont\n<0003> stringwidth r exch r = =\n",
        header_with(
            "%!PS-Adobe-3.0 Resource-CIDFont",
            &["10.0", "0.0"],
            "% Scenario: CIDFontType 2 by CID map. The synthesised TrueType font\n\
             % is wrapped as a CIDFontType 2 dictionary whose CIDMap maps CID 3 to\n\
             % glyph 1, the 1024-unit a in a 2048 em; shown through Identity-H at\n\
             % size 20, CID 3 advances 10.\n\
             % Expect: 10.0 and 0.0.\n",
        )
    );

    let mut cidfont_type1 = header_with(
        "%!PS-Adobe-3.0 Resource-CIDFont",
        &["true", "0", "0", "5.0", "0.0", "7.0", "0.0", "3.0", "0.0"],
        "% divergence: resource-size-unknown\n\
         % Scenario: a Type 1 charstring CIDFont. SynCIDT1 is in the CIDInit\n\
         % StartData form: two font dictionaries in FDArray, each with its own\n\
         % lenIV and subroutines, and the CIDMap and charstrings in the\n\
         % binary GlyphData that follows StartData. CID 1 (dictionary 0)\n\
         % advances 500, CID 2 (dictionary 1, drawn through its subroutine)\n\
         % 700, and CID 3 (dictionary 0) 300; at size 10 through Identity-H\n\
         % they measure 5, 7, and 3.\n\
         % Expect: true, 0, 0, then 5.0 0.0, 7.0 0.0, 3.0 0.0.\n",
    )
    .into_bytes();
    let cid_file = CidType1Font::corpus().file();
    let body = cid_file
        .iter()
        .position(|&b| b == b'\n')
        .map_or(0, |n| n + 1);
    cidfont_type1.extend_from_slice(&cid_file[body..]);
    cidfont_type1.extend_from_slice(
        format!(
            "/SynCIDT1 /CIDFont resourcestatus = = =\n\
             {ROUND}/SynT1Composite /Identity-H [ /SynCIDT1 /CIDFont findresource ] \
             composefont 10 scalefont setfont\n\
             <0001> stringwidth r exch r = =\n\
             <0002> stringwidth r exch r = =\n\
             <0003> stringwidth r exch r = =\n"
        )
        .as_bytes(),
    );

    let mut compose = font_set_header(
        &["0"],
        "% Scenario: composefont. A Type 0 font named SynComposite is composed\n\
         % over the predefined Identity-H CMap and the CID-keyed CFF SynCID;\n\
         % findfont returns it and its FontType is 0.\n\
         % Expect: 0.\n",
    )
    .into_bytes();
    compose.extend_from_slice(&cid_set);
    compose.extend_from_slice(
        b"/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont pop\n\
          /SynComposite findfont /FontType get =\n",
    );

    let mut two_byte = font_set_header(
        &["12.0", "0.0"],
        "% Scenario: two-byte show through Identity. SynCID's CIDs 1 and 2\n\
         % advance 500 and 700; through Identity-H at size 10 the two-byte\n\
         % string <00010002> shows from (0, 0) and leaves the current point\n\
         % at 12. The page carries the run as one text operation over a\n\
         % composite resource; the PDF embeds the CID-keyed CFF subset as a\n\
         % CIDFontType0C descendant of a Type 0 font with Identity-H.\n\
         % Expect: 12.0 and 0.0.\n",
    )
    .into_bytes();
    two_byte.extend_from_slice(&cid_set);
    two_byte.extend_from_slice(
        format!(
            "{ROUND}/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
             10 scalefont setfont\n0 0 moveto <00010002> show currentpoint r exch r = =\nshowpage\n"
        )
        .as_bytes(),
    );

    let mut vertical = font_set_header(
        &["0.0", "90.0"],
        "% Scenario: vertical writing. Through Identity-V, writing mode 1,\n\
         % every glyph advances downward by the default vertical advance, one\n\
         % em, and sits at its vertical origin; at size 10 the one-glyph\n\
         % string <0001> shown from (0, 100) leaves the current point at\n\
         % (0, 90). The text operation records the writing mode and the PDF\n\
         % font's encoding is Identity-V.\n\
         % Expect: 0.0 and 90.0.\n",
    )
    .into_bytes();
    vertical.extend_from_slice(&cid_set);
    vertical.extend_from_slice(
        format!(
            "{ROUND}/SynV /Identity-V [ /SynCID /CIDFont findresource ] composefont \
             10 scalefont setfont\n0 100 moveto <0001> show currentpoint r exch r = =\nshowpage\n"
        )
        .as_bytes(),
    );

    let mut partial = font_set_header(
        &["14.5", "0.0"],
        "% Scenario: a partial match decodes as notdef. Through the corpus\n\
         % CMap Syn-H, the string <41 81 20 42> holds the one-byte code A\n\
         % (CID 34, advance 500), then the lead byte 81 of the two-byte\n\
         % codespace followed by 20, which lies outside it: two bytes of\n\
         % notdef (CID 0, advance 250), then B (CID 35, advance 700). At\n\
         % size 10 the run leaves the current point at 14.5, and the page's\n\
         % text operation holds three glyphs, the notdef with its two bytes.\n\
         % Expect: 14.5 and 0.0.\n",
    )
    .into_bytes();
    partial.extend_from_slice(&cid_set);
    partial.extend_from_slice(corpus_cmap().as_bytes());
    partial.extend_from_slice(
        format!(
            "{ROUND}/SynMixed /Syn-H [ /SynCID /CIDFont findresource ] composefont \
             10 scalefont setfont\n0 0 moveto <41812042> show currentpoint r exch r = =\nshowpage\n"
        )
        .as_bytes(),
    );

    let mut mixed = font_set_header(
        &[],
        "% Scenario: mixed byte lengths. Through the corpus CMap Syn-H the\n\
         % string <41 8140 42> decodes as a one-byte, a two-byte, and a\n\
         % one-byte code (CIDs 34, 200, and 35); the run holds three glyphs\n\
         % whose codes the dump writes in hexadecimal, each padded to its\n\
         % length, with the displacements 500, 300, and 700.\n",
    )
    .into_bytes();
    mixed.extend_from_slice(&cid_set);
    mixed.extend_from_slice(corpus_cmap().as_bytes());
    mixed.extend_from_slice(
        b"/SynMixed /Syn-H [ /SynCID /CIDFont findresource ] composefont \
          10 scalefont setfont\n72 700 moveto <41814042> show\nshowpage\n",
    );

    let mut composite_dump = font_set_header(
        &[],
        "% Scenario: a composite run in the dump. The two-byte string\n\
         % <00010002> shown through Identity-H records one text operation\n\
         % whose codes print as <00010002> with the displacements 500 and\n\
         % 700, over the resource line naming Identity-H, the writing mode,\n\
         % and the CID-keyed CFF descendant.\n",
    )
    .into_bytes();
    composite_dump.extend_from_slice(&cid_set);
    composite_dump.extend_from_slice(
        b"/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
          10 scalefont setfont\n72 700 moveto <00010002> show\nshowpage\n",
    );

    let cid_tt_two =
        without_header(&corpus_truetype().cidfont_type2("SynCIDTT", &[(3, 1), (4, 2)]));
    let cidfont_type2_embedded = format!(
        "{}{cid_tt_two}/SynTTComposite /Identity-H [ /SynCIDTT /CIDFont findresource ] \
         composefont 20 scalefont setfont\n100 100 moveto <00030004> show\nshowpage\n",
        header_with(
            "%!PS-Adobe-3.0 Resource-CIDFont",
            &[],
            "% Scenario: CIDFontType2 with a CID map. The synthesised TrueType\n\
             % font wrapped as CIDFontType 2 maps CID 3 to glyph 1 and CID 4 to\n\
             % glyph 2; shown through Identity-H, the PDF's descendant is a\n\
             % CIDFontType2 whose FontFile2 is the subset and whose CIDToGIDMap\n\
             % stream maps each CID to the subset's glyph index, with ToUnicode\n\
             % from the program's own Unicode cmap.\n",
        )
    );

    let ucs2_cmap = "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\n\
         begincmap\n\
         /CIDSystemInfo 3 dict dup begin\n\
         /Registry (Adobe) def\n\
         /Ordering (Identity) def\n\
         /Supplement 0 def\n\
         end def\n\
         /CMapName /Syn-UCS2-H def\n\
         /CMapType 1 def\n\
         /WMode 0 def\n\
         1 begincodespacerange\n\
         <0000> <ffff>\n\
         endcodespacerange\n\
         1 begincidchar\n\
         <0041> 1\n\
         endcidchar\n\
         1 begincidrange\n\
         <0042> <0042> 2\n\
         endcidrange\n\
         endcmap\n\
         CMapName currentdict /CMap defineresource pop\n\
         end\n\
         end\n";
    let mut ucs2 = font_set_header(
        &[],
        "% Scenario: a Unicode-based CMap gives ToUnicode. The CMap Syn-UCS2-H\n\
         % maps the UTF-16 codes <0041> and <0042> to CIDs 1 and 2; its name\n\
         % marks it Unicode-based, so the Type 0 font's ToUnicode maps CID 1\n\
         % to U+0041 and CID 2 to U+0042 from the codes the run came from, and\n\
         % text extraction yields AB.\n",
    )
    .into_bytes();
    ucs2.extend_from_slice(&cid_set);
    ucs2.extend_from_slice(ucs2_cmap.as_bytes());
    ucs2.extend_from_slice(
        b"/SynUnicode /Syn-UCS2-H [ /SynCID /CIDFont findresource ] composefont \
          10 scalefont setfont\n100 100 moveto <00410042> show\nshowpage\n",
    );

    let mut fallback = header_with(
        "%!PS-Adobe-3.0 Resource-CIDFont",
        &[],
        "% Scenario: a Type 1 charstring CID font falls back to Type 3. The\n\
         % CIDFont SynCIDT1 has no PDF embedding form, so its one shown glyph,\n\
         % CID 2 (drawn through dictionary 1's subroutine, advance 700),\n\
         % becomes a Type 3 font with one CharProc holding the outline under\n\
         % d1, an Encoding naming it cid2 at code 1, and the content stream\n\
         % re-encoded to that one-byte code.\n",
    )
    .into_bytes();
    fallback.extend_from_slice(&cid_file[body..]);
    fallback.extend_from_slice(
        b"/SynT1Composite /Identity-H [ /SynCIDT1 /CIDFont findresource ] composefont \
          10 scalefont setfont\n100 100 moveto <0002> show\nshowpage\n",
    );

    let mut composite_charpath = font_set_header(
        &[],
        "% Scenario: charpath through a composite font. <0001> false charpath\n\
         % appends CID 1's outline, the 400-unit square from (50, 0), scaled\n\
         % by the size at (100, 100), and advances the current point by its\n\
         % width; the fill paints one path and no text operation is recorded.\n",
    )
    .into_bytes();
    composite_charpath.extend_from_slice(&cid_set);
    composite_charpath.extend_from_slice(
        b"/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
          10 scalefont setfont\n100 100 moveto <0001> false charpath fill\nshowpage\n",
    );

    vec![
        ("eexec-hex.ps", eexec_hex_file),
        ("eexec-string.ps", eexec_string_file),
        ("type1-width.ps", type1_width.into_bytes()),
        ("type1-seac-advance.ps", seac_advance.into_bytes()),
        ("type1-malformed-charstring.ps", malformed.into_bytes()),
        ("type42-advance.ps", type42_advance.into_bytes()),
        ("charpath-advance.ps", charpath_advance.into_bytes()),
        ("charpath-fill.ps", charpath_fill.into_bytes()),
        ("type1-seac-glyphshow.ps", seac_glyphshow.into_bytes()),
        ("type42-charpath-bbox.ps", type42_bbox.into_bytes()),
        ("embedded-font-dump.ps", dump.into_bytes()),
        ("type1-subset-round-trip.ps", round_trip.into_bytes()),
        ("type1-embedded-two-pages.ps", two_pages.into_bytes()),
        ("truetype-subset-cmap.ps", truetype_cmap.into_bytes()),
        ("truetype-embedded.ps", truetype_embedded.into_bytes()),
        ("cff-width.ps", cff_width),
        ("cff-charpath-bbox.ps", cff_bbox),
        ("fontset-defines-fonts.ps", fontset_defines),
        ("fontset-dictionary-stack.ps", dict_stack),
        ("fontset-short-data.ps", short_data),
        ("cff-embedded-type1c.ps", cff_type1c),
        ("cmap-embedded.ps", cmap_embedded.into_bytes()),
        ("cidfont-fontset.ps", cidfont_fontset),
        ("cidfont-type2-cidmap.ps", cidfont_type2.into_bytes()),
        ("cidfont-type1-charstrings.ps", cidfont_type1),
        ("composefont.ps", compose),
        ("composite-two-byte-width.ps", two_byte),
        ("composite-vertical-width.ps", vertical),
        ("composite-partial-match.ps", partial),
        ("composite-mixed-lengths.ps", mixed),
        ("composite-dump.ps", composite_dump),
        (
            "cidfont-type2-embedded.ps",
            cidfont_type2_embedded.into_bytes(),
        ),
        ("cmap-ucs2-tounicode.ps", ucs2),
        ("cidfont-type1-fallback.ps", fallback),
        ("composite-charpath-fill.ps", composite_charpath),
    ]
}

#[test]
#[ignore = "writes the corpus files; run once and commit the result"]
fn generate_corpus_fonts() {
    let dir = corpus_dir();
    std::fs::create_dir_all(&dir).expect("corpus directory");
    for (name, content) in files() {
        std::fs::write(dir.join(name), content).expect("writable corpus");
    }
}

#[test]
fn corpus_fonts_are_current() {
    let dir = corpus_dir();
    for (name, content) in files() {
        let path = dir.join(name);
        let committed = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run the generator", path.display()));
        assert!(
            committed == content,
            "{} differs from the generator's output; rerun \
             `cargo test -p ps-fonts --test corpus_fonts -- --ignored`",
            path.display()
        );
    }
}
