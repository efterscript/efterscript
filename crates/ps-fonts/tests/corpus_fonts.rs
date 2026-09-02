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
    TRAILER, TrueTypeFont, Type1Font, corpus_truetype, corpus_type1, eexec_binary, eexec_hex,
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

fn header(expect: &[&str], scenario: &str) -> String {
    let mut out = String::from(
        "%!PS\n\
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
