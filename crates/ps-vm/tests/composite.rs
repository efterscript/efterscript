// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Composite fonts through the recording backend: CMaps embedded and
//! predefined, CIDFonts of every loading form, Type 0 fonts and
//! `composefont`, and how the show family decodes, measures, and
//! outlines through them. The printed-output scenarios are corpus files
//! under `corpus/unit/fonts`, generated from the same synthesised fonts.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Fonts, Log, Recording};
use ps_fonts::ProgramKind;
use ps_fonts::testing::{CidType1Font, corpus_cid_cff, corpus_cmap, corpus_truetype};
use ps_vm::{Config, FontSource, Glyph, Interp, Io, Outcome, Point, SliceSource};

struct Run {
    interp: Interp,
    outcome: Outcome,
    output: String,
    log: Log,
    fonts: Fonts,
}

impl Run {
    fn calls(&self) -> Vec<Call> {
        self.log.borrow().clone()
    }

    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }

    fn command(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.command),
            _ => None,
        }
    }

    fn shows(&self) -> Vec<Vec<Glyph>> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Show(glyphs) => Some(glyphs),
                _ => None,
            })
            .collect()
    }

    fn path_calls(&self) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    Call::MoveTo(_) | Call::LineTo(_) | Call::CurveTo(..) | Call::ClosePath
                )
            })
            .collect()
    }

    fn top_numbers(&self, n: usize) -> Vec<f32> {
        let stack = self.interp.ostack();
        stack[stack.len() - n..]
            .iter()
            .map(|o| o.as_number().expect("number"))
            .collect()
    }

    fn sources(&self) -> Vec<FontSource> {
        self.fonts
            .borrow()
            .iter()
            .map(|(_, info)| info.source.clone())
            .collect()
    }
}

fn exec(program: &[u8]) -> Run {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let fonts: Fonts = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::with_fonts(log.clone(), fonts.clone())));
    let outcome = interp.run(&mut SliceSource::new(program));
    Run {
        interp,
        outcome,
        output: out.text(),
        log,
        fonts,
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn point_approx(p: Point, x: f32, y: f32) -> bool {
    approx(p.x, x) && approx(p.y, y)
}

/// The CMap behind a CMap dictionary, through its `CodeMap` id.
fn cmap_of(interp: &mut Interp, dict: ps_vm::Object) -> Rc<ps_fonts::CMap> {
    let key = interp.intern("CodeMap");
    let id = interp.memory().dict(dict).unwrap().get(key).unwrap();
    interp.cmap(id.as_font_id().unwrap()).unwrap()
}

fn glyph_approx(a: Glyph, b: Glyph) -> bool {
    a.code == b.code && a.len == b.len && a.cid == b.cid && approx(a.dx, b.dx) && approx(a.dy, b.dy)
}

fn glyph(code: u32, len: u8, cid: u16, dx: f32, dy: f32) -> Glyph {
    Glyph {
        code,
        len,
        cid,
        dx,
        dy,
    }
}

/// The corpus CID-keyed CFF as a FontSet, followed by `program`.
fn with_cid_set(program: &str) -> Vec<u8> {
    let mut out = corpus_cid_cff().font_set("SynCIDSet");
    out.extend_from_slice(program.as_bytes());
    out
}

const COMPOSE_H: &str =
    "/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont pop ";
const COMPOSE_V: &str = "/SynV /Identity-V [ /SynCID /CIDFont findresource ] composefont pop ";

// --- CMaps -----------------------------------------------------------------------------

#[test]
fn identity_cmaps_are_predefined_and_loaded_on_first_use() {
    let run = exec(
        b"/Identity-H /CMap resourcestatus \
          /Identity-H /CMap findresource dup /WMode get exch \
          dup /CMapName get exch dup /CIDSystemInfo get /Ordering get exch \
          dup /CodeMap known exch /CMapType get \
          /Identity-V /CMap findresource /WMode get \
          /Identity-H /CMap resourcestatus \
          save /Identity-H /CMap findresource pop restore \
          /Identity-H /CMap findresource /WMode get \
          (*) { == } 64 string /CMap resourceforall",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(stack[0].as_i32(), Some(2), "predefined before loading");
    assert_eq!(stack[1].as_i32(), Some(0));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(0), "Identity-H is horizontal");
    assert_eq!(mem.name_text(stack[4].as_name().unwrap()), b"Identity-H");
    assert_eq!(mem.string(stack[5]).unwrap(), b"Identity");
    assert_eq!(stack[6].as_bool(), Some(true), "the CodeMap id");
    assert_eq!(stack[7].as_i32(), Some(1));
    assert_eq!(stack[8].as_i32(), Some(1), "Identity-V is vertical");
    assert_eq!(stack[9].as_i32(), Some(2), "still predefined after loading");
    assert_eq!(
        stack[12].as_i32(),
        Some(0),
        "the cached dictionary survives restore"
    );
    assert_eq!(stack.len(), 13);
    assert_eq!(run.output, "(Identity-H)\n(Identity-V)\n");
    // The loaded dictionaries are read-only and the CMap itself is the
    // identity over two-byte codes.
    let run = exec(b"/Identity-H /CMap findresource /X 1 put");
    assert_eq!(run.error(), Some("invalidaccess"));
    let mut run = exec(b"/Identity-V /CMap findresource");
    let dict = run.interp.ostack()[0];
    let cmap = cmap_of(&mut run.interp, dict);
    assert_eq!(cmap.name, b"Identity-V");
    assert_eq!(cmap.wmode, 1);
    assert_eq!(cmap.decode(&[0x12, 0x34]).cid, Some(0x1234));
    assert_eq!(cmap.decode(&[0x12, 0x34]).len, 2);
    assert!(
        cmap.parent
            .as_ref()
            .is_some_and(|p| p.name == b"Identity-H")
    );
    assert!(!cmap.unicode_based);
}

#[test]
fn an_embedded_cmap_program_defines_a_resource() {
    let program = format!(
        "{}/Syn-H /CMap resourcestatus \
         /Syn-H /CMap findresource dup /WMode get exch /CMapName get \
         (*) {{ == }} 64 string /CMap resourceforall",
        corpus_cmap()
    );
    let run = exec(program.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack().to_vec();
    assert_eq!(stack[0].as_i32(), Some(0), "defined by the program");
    assert_eq!(stack[1].as_i32(), Some(0));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(0));
    assert_eq!(
        run.interp.memory().name_text(stack[4].as_name().unwrap()),
        b"Syn-H"
    );
    assert_eq!(run.output, "(Syn-H)\n(Identity-H)\n(Identity-V)\n");
    let mut run = exec(format!("{}/Syn-H /CMap findresource", corpus_cmap()).as_bytes());
    let dict = run.interp.ostack()[0];
    let cmap = cmap_of(&mut run.interp, dict);
    assert_eq!(cmap.codespaces.len(), 2);
    assert_eq!(cmap.decode(b"A").cid, Some(34));
    assert_eq!(cmap.decode(&[0x81, 0x41]).cid, Some(201));
    assert_eq!(cmap.decode(&[0x81, 0x90]).cid, Some(0), "the notdef range");
    assert_eq!(cmap.decode(&[0x81, 0x20]).len, 2, "a partial match");
    assert_eq!(cmap.decode(&[0x81, 0x20]).cid, None);
    assert_eq!(
        cmap.system_info.as_ref().map(|s| s.ordering.clone()),
        Some(b"Identity".to_vec())
    );
    // Misuse of the operators.
    let run = exec(b"/CIDInit /ProcSet findresource begin endcmap");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec(
        b"/CIDInit /ProcSet findresource begin 5 dict begin begincmap \
          1 begincidrange <41> 5 endcidrange",
    );
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec(b"/CIDInit /ProcSet findresource begin 5 dict begin begincmap endcidchar");
    assert_eq!(run.error(), Some("unmatchedmark"));
    let run = exec(b"/CIDInit /ProcSet findresource begin 5 dict begin begincmap /Nope usecmap");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec(b"/X 5 dict /CMap defineresource");
    assert_eq!(run.error(), Some("typecheck"), "not a CMap dictionary");
    let run = exec(
        b"/CIDInit /ProcSet resourcestatus pop pop /CIDInit /ProcSet findresource /usecmap known",
    );
    assert_eq!(run.interp.ostack()[0].as_i32(), Some(2));
    assert_eq!(run.interp.ostack()[1].as_bool(), Some(true));
}

#[test]
fn a_cmap_may_chain_to_a_predefined_parent_and_carry_bf_entries() {
    let run = exec(
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
          /CMapName /Syn-UCS2-V def \
          /Identity-H usecmap \
          /WMode 1 def \
          1 begincidchar <0041> 7 endcidchar \
          1 beginbfchar <0042> <00420043> endbfchar \
          1 beginbfrange <0050> <0052> <0060> endbfrange \
          1 beginbfrange <0070> <0071> [ <0001> /two ] endbfrange \
          2 usefont \
          1 begincidrange <0100> <01ff> 500 endcidrange \
          endcmap CMapName currentdict /CMap defineresource pop end end \
          /Syn-UCS2-V /CMap findresource",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let dict = run.interp.ostack()[0];
    let mut interp = run.interp;
    let cmap = cmap_of(&mut interp, dict);
    assert_eq!(cmap.wmode, 1);
    assert!(cmap.unicode_based);
    assert_eq!(cmap.decode(&[0x00, 0x41]).cid, Some(7));
    assert_eq!(
        cmap.decode(&[0x12, 0x34]).cid,
        Some(0x1234),
        "through the parent"
    );
    assert_eq!(cmap.decode(&[0x01, 0x05]).cid, Some(505));
    assert_eq!(cmap.decode(&[0x01, 0x05]).font, 2);
    assert_eq!(cmap.bf(2, 0x42), Some(vec![0x00, 0x42, 0x00, 0x43]));
    assert_eq!(cmap.bf(2, 0x52), Some(vec![0x00, 0x62]));
    assert_eq!(cmap.bf(2, 0x71), Some(b"two".to_vec()));
}

// --- CIDFonts -----------------------------------------------------------------------

#[test]
fn a_cid_keyed_cff_in_a_fontset_defines_a_cidfont_resource() {
    let run = exec(&with_cid_set(
        "/SynCID /CIDFont resourcestatus \
         /SynCID /CIDFont findresource dup /CIDCount get exch \
         dup /CIDFontType get exch dup /FontType get exch \
         dup /CIDSystemInfo get /Registry get exch dup /FID known exch \
         dup /FontMatrix get 0 get exch /CIDFontName get \
         /SynCID /Font resourcestatus \
         /SynCIDSet /FontSet findresource length \
         (*) { == } 64 string /CIDFont resourceforall",
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(stack[0].as_i32(), Some(0));
    assert_eq!(stack[1].as_i32(), Some(0));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(201), "CIDs up to 200");
    assert_eq!(stack[4].as_i32(), Some(0));
    assert_eq!(stack[5].as_i32(), Some(9));
    assert_eq!(mem.string(stack[6]).unwrap(), b"Adobe");
    assert_eq!(stack[7].as_bool(), Some(true));
    assert!(approx(stack[8].as_number().unwrap(), 0.001));
    assert_eq!(mem.name_text(stack[9].as_name().unwrap()), b"SynCID");
    assert_eq!(stack[10].as_bool(), Some(false), "not a Font instance");
    assert_eq!(
        stack[11].as_i32(),
        Some(0),
        "the FontSet lists name-keyed fonts"
    );
    assert_eq!(run.output, "(SynCID)\n");
    assert!(run.interp.cid_program(b"SynCID").is_some());
    // A CIDFont is not a font to show with directly, but it is a font
    // dictionary `setfont` accepts.
    let run = exec(&with_cid_set(
        "/SynCID /CIDFont findresource 10 scalefont setfont 0 0 moveto <0001> show",
    ));
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("show"));
    let run = exec(&with_cid_set("/SynCID /CIDFont findresource /X 1 put"));
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn a_cidfonttype2_dictionary_maps_cids_to_glyphs() {
    let font = corpus_truetype().cidfont_type2("SynCIDTT", &[(3, 1), (4, 2)]);
    let program = format!(
        "{font}/SynCIDTT /CIDFont resourcestatus pop pop \
         /C /Identity-H [ /SynCIDTT /CIDFont findresource ] composefont 20 scalefont setfont \
         <0003> stringwidth <0004> stringwidth <0005> stringwidth <0001> stringwidth \
         0 0 moveto <00030004> show currentpoint"
    );
    let run = exec(program.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(0), "a defined CIDFont");
    let top = run.top_numbers(10);
    assert!(
        approx(top[0], 10.0),
        "CID 3 is glyph 1, 1024 of 2048: {top:?}"
    );
    assert!(approx(top[2], 11.71875), "CID 4 is glyph 2: {top:?}");
    assert!(approx(top[4], 10.0), "CID 5 is outside the map: the notdef");
    assert!(approx(top[6], 10.0), "CID 1 maps to glyph 0: the notdef");
    assert!(approx(top[8], 21.71875) && approx(top[9], 0.0), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0][0].cid, 3);
    assert_eq!(shows[0][0].len, 2);
    assert!(approx(shows[0][0].dx, 0.5), "units of the em");
    let sources = run.sources();
    assert!(matches!(
        &sources[0],
        FontSource::Composite { descendant, .. }
            if matches!(&**descendant, FontSource::Embedded { kind: ProgramKind::TrueType, font_name, .. }
                if font_name == b"SynCIDTT")
    ));
    // The same dictionary through definefont lands in the CIDFont
    // category; a dictionary form of CIDMap and the integer form work.
    let by_definefont = font.replace(
        "/SynCIDTT exch /CIDFont defineresource pop",
        "/SynCIDTT exch definefont pop /SynCIDTT /CIDFont resourcestatus",
    );
    let run = exec(by_definefont.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.interp.ostack()[2].as_bool(), Some(true));
    let dict_map = font.replace(
        "/CIDMap <00000000000000010002> def",
        "/CIDMap << 3 1 4 2 >> def",
    );
    assert!(dict_map != font, "the string form was replaced");
    let run = exec(
        format!(
            "{dict_map}/C /Identity-H [ /SynCIDTT /CIDFont findresource ] composefont \
             20 scalefont setfont <0003> stringwidth"
        )
        .as_bytes(),
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert!(approx(run.top_numbers(2)[0], 10.0));
    let offset_map = font.replace("/CIDMap <00000000000000010002> def", "/CIDMap 0 def");
    let run = exec(
        format!(
            "{offset_map}/C /Identity-H [ /SynCIDTT /CIDFont findresource ] composefont \
             20 scalefont setfont <0002> stringwidth"
        )
        .as_bytes(),
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert!(approx(run.top_numbers(2)[0], 11.71875), "CID 2 is glyph 2");
    // Without sfnts the dictionary is not a CIDFont at all.
    let run = exec(b"/Bad << /CIDFontType 2 /FontMatrix [1 0 0 1 0 0] >> definefont");
    assert_eq!(run.error(), Some("invalidfont"));
}

#[test]
fn a_type1_charstring_cidfont_loads_through_startdata() {
    let mut program = CidType1Font::corpus().file();
    program.extend_from_slice(
        b"/SynCIDT1 /CIDFont resourcestatus pop pop \
          /SynCIDT1 /CIDFont findresource dup /CIDCount get exch dup /FontMatrix get 0 get exch \
          /FID known \
          /T /Identity-H [ /SynCIDT1 /CIDFont findresource ] composefont 10 scalefont setfont \
          <0001> stringwidth <0002> stringwidth <0003> stringwidth <0009> stringwidth \
          0 0 moveto <0002> false charpath currentpoint",
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(0), "a defined CIDFont");
    assert_eq!(stack[1].as_i32(), Some(4));
    assert!(
        approx(stack[2].as_number().unwrap(), 0.001),
        "the default matrix"
    );
    assert_eq!(stack[3].as_bool(), Some(true));
    let top = run.top_numbers(10);
    assert!(approx(top[0], 5.0), "{top:?}");
    assert!(approx(top[2], 7.0), "dictionary 1's subroutine: {top:?}");
    assert!(approx(top[4], 3.0), "{top:?}");
    assert!(
        approx(top[6], 0.0),
        "a CID without a charstring is the notdef: {top:?}"
    );
    assert!(approx(top[8], 7.0) && approx(top[9], 0.0), "{top:?}");
    // The outline of CID 2: the 600-unit square through the subroutine,
    // at size 10.
    let path = run.path_calls();
    assert_eq!(path.len(), 7, "{path:?}");
    assert!(matches!(path[1], Call::MoveTo(p) if point_approx(p, 0.0, 0.0)));
    assert!(matches!(path[2], Call::LineTo(p) if point_approx(p, 6.0, 0.0)));
    assert!(matches!(path[3], Call::LineTo(p) if point_approx(p, 6.0, 6.0)));
    assert!(matches!(path[4], Call::LineTo(p) if point_approx(p, 0.0, 6.0)));
    assert!(matches!(path[5], Call::ClosePath));
    assert!(matches!(path[6], Call::MoveTo(p) if point_approx(p, 7.0, 0.0)));
    // Short data, a bad form, and a dictionary without its layout.
    let mut short = CidType1Font::corpus().file();
    let cut = short.len() - 12;
    short.truncate(cut);
    let run = exec(&short);
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("StartData"));
    assert_eq!(run.interp.ostack().len(), 2, "operands stay on failure");
    assert_eq!(run.interp.dstack().len(), 5, "dictionaries stay on failure");
    let run = exec(b"/CIDInit /ProcSet findresource begin 5 dict begin (Octal) 4 StartData\nabcd");
    assert_eq!(run.error(), Some("rangecheck"));
    // A current dictionary that is not a CIDFont's (here the procedure
    // set itself) has no CIDFontName.
    let run = exec(b"/CIDInit /ProcSet findresource begin (Binary) 4 StartData\nabcd");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec(b"/CIDInit /ProcSet findresource begin (Binary) (4) StartData\nabcd");
    assert_eq!(run.error(), Some("typecheck"));
}

#[test]
fn start_data_ends_the_font_and_procedure_set_dictionaries() {
    let mut program = b"countdictstack\n".to_vec();
    program.extend(CidType1Font::corpus().file());
    program.extend_from_slice(b"countdictstack count");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(3);
    assert_eq!(top[0], top[1]);
    assert_eq!(top[2], 2.0, "nothing but the two counts remains");
    assert_eq!(run.interp.dstack().len(), 3);
}

#[test]
fn hexadecimal_glyph_data_is_read_too() {
    let font = CidType1Font::corpus();
    let (data, _) = font.glyph_data();
    let file = font.file();
    let binary = file
        .windows(8)
        .position(|w| w == b"(Binary)")
        .expect("the binary form");
    let mut program = String::from_utf8(file[..binary].to_vec()).unwrap();
    program.push_str(&format!("(Hex) {} StartData\n", data.len()));
    program.push_str(&ps_fonts::testing::hex_lines(&data));
    program.push_str(
        "/T /Identity-H [ /SynCIDT1 /CIDFont findresource ] composefont 10 scalefont setfont \
         <0002> stringwidth",
    );
    let run = exec(program.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert!(approx(run.top_numbers(2)[0], 7.0));
}

// --- Type 0 fonts ---------------------------------------------------------------------

#[test]
fn composefont_defines_a_type0_font() {
    let run = exec(&with_cid_set(
        "/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
         dup /FontType get exch dup /FMapType get exch dup /FID known exch \
         dup /WMode get exch dup /Encoding get 0 get exch dup /CMap get /CMapName get exch \
         /FDepVector get length \
         /SynComposite findfont /FontType get \
         /SynComposite /Font resourcestatus pop pop",
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(stack[0].as_i32(), Some(0));
    assert_eq!(stack[1].as_i32(), Some(9));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(0));
    assert_eq!(stack[4].as_i32(), Some(0));
    assert_eq!(mem.name_text(stack[5].as_name().unwrap()), b"Identity-H");
    assert_eq!(stack[6].as_i32(), Some(1));
    assert_eq!(stack[7].as_i32(), Some(0));
    assert_eq!(stack[8].as_i32(), Some(0), "a defined Font instance");
    // A CMap dictionary operand, and the vertical CMap's writing mode.
    let run = exec(&with_cid_set(
        "/Identity-V /CMap findresource /V exch [ /SynCID /CIDFont findresource ] composefont \
         /WMode get",
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.interp.ostack()[0].as_i32(), Some(1));
    let run = exec(&with_cid_set(
        "/V /NoSuchCMap [ /SynCID /CIDFont findresource ] composefont",
    ));
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.interp.ostack().len(), 3, "operands stay on failure");
    let run = exec(b"/V /Identity-H [ ] composefont");
    assert_eq!(run.error(), Some("rangecheck"));
}

#[test]
fn definefont_accepts_map_type_9_and_rejects_the_rest() {
    let run = exec(&with_cid_set(
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-V \
         /FDepVector [ /SynCID /CIDFont findresource ] >> definefont \
         10 scalefont setfont 0 100 moveto <0001> show currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(2);
    assert!(
        approx(top[0], 0.0) && approx(top[1], 90.0),
        "a CMap by name, loaded by definefont, Encoding defaulted: {top:?}"
    );
    for bad in [
        "/T0 << /FontType 0 /FMapType 2 /FontMatrix [1 0 0 1 0 0] /Encoding [0] \
         /FDepVector [ /SynCID /CIDFont findresource ] >> definefont",
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /NoSuch \
         /FDepVector [ /SynCID /CIDFont findresource ] >> definefont",
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-H \
         /FDepVector [ ] >> definefont",
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-H \
         /FDepVector [ /SynCID /CIDFont findresource ] /Encoding [ 1 ] >> definefont",
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-H \
         /FDepVector [ 5 dict ] >> definefont",
        "/T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap 5 dict \
         /FDepVector [ /SynCID /CIDFont findresource ] >> definefont",
        "/T0 /Identity-H [ /SynCID /CIDFont findresource ] composefont pop \
         /Nested << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-H \
         /FDepVector [ /T0 findfont ] >> definefont",
    ] {
        let run = exec(&with_cid_set(bad));
        assert_eq!(run.error(), Some("invalidfont"), "{bad}");
        assert_eq!(run.command(), Some("definefont"), "{bad}");
    }
}

// --- composite text ---------------------------------------------------------------------

#[test]
fn two_byte_codes_show_through_identity_h() {
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_H}/SynComposite findfont 10 scalefont setfont \
         0 0 moveto <00010002> show currentpoint <00010002> stringwidth"
    )));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(4);
    assert!(approx(top[0], 12.0) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 12.0) && approx(top[3], 0.0), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(
        shows[0],
        vec![glyph(1, 2, 1, 500.0, 0.0), glyph(2, 2, 2, 700.0, 0.0)]
    );
    let sources = run.sources();
    assert_eq!(sources.len(), 1);
    let FontSource::Composite {
        cmap_name,
        wmode,
        unicode_based,
        cmap,
        descendant,
        ..
    } = &sources[0]
    else {
        panic!("a composite source: {:?}", sources[0]);
    };
    assert_eq!(cmap_name, b"Identity-H");
    assert_eq!(*wmode, 0);
    assert!(!unicode_based);
    assert_eq!(cmap.name, b"Identity-H");
    assert!(matches!(
        &**descendant,
        FontSource::Embedded { kind: ProgramKind::Cff, font_name, font_matrix, .. }
            if font_name == b"SynCID" && approx(font_matrix.0[0], 0.001)
    ));
    // The composed matrix: the descendant's thousandths under the Type
    // 0 font's identity, scaled by the size.
    let set = run
        .calls()
        .into_iter()
        .find_map(|c| match c {
            Call::SetFont(Some(font)) => Some(font),
            _ => None,
        })
        .expect("setfont reached the backend");
    assert!(approx(set.matrix.0[0], 0.01) && approx(set.matrix.0[3], 0.01));
}

#[test]
fn vertical_writing_advances_downward_and_positions_at_the_vertical_origin() {
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_V}/SynV findfont 10 scalefont setfont \
         0 100 moveto <0001> show currentpoint <0001> stringwidth \
         0 0 moveto <0001> false charpath currentpoint"
    )));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(6);
    assert!(approx(top[0], 0.0) && approx(top[1], 90.0), "{top:?}");
    assert!(approx(top[2], 0.0) && approx(top[3], -10.0), "{top:?}");
    assert!(approx(top[4], 0.0) && approx(top[5], -10.0), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows[0], vec![glyph(1, 2, 1, 0.0, -1000.0)]);
    assert!(matches!(
        &run.sources()[0],
        FontSource::Composite { wmode: 1, cmap_name, .. } if cmap_name == b"Identity-V"
    ));
    // The square from (50, 0) to (450, 400) of a 500-wide glyph, moved
    // so its vertical origin (250, 880) sits at the current point.
    let path = run.path_calls();
    let path = &path[path.len() - 6..];
    assert!(
        matches!(path[0], Call::MoveTo(p) if point_approx(p, -2.0, -8.8)),
        "{path:?}"
    );
    assert!(matches!(path[1], Call::LineTo(p) if point_approx(p, 2.0, -8.8)));
    assert!(matches!(path[2], Call::LineTo(p) if point_approx(p, 2.0, -4.8)));
    assert!(matches!(path[3], Call::LineTo(p) if point_approx(p, -2.0, -4.8)));
    assert!(matches!(path[4], Call::ClosePath));
    assert!(matches!(path[5], Call::MoveTo(p) if point_approx(p, 0.0, -10.0)));
}

#[test]
fn charpath_outlines_by_cid_and_advances() {
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_H}/SynComposite findfont 10 scalefont setfont \
         0 0 moveto <0001> false charpath currentpoint"
    )));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(2);
    assert!(approx(top[0], 5.0) && approx(top[1], 0.0), "{top:?}");
    let path = run.path_calls();
    assert_eq!(path.len(), 7, "{path:?}");
    assert!(matches!(path[1], Call::MoveTo(p) if point_approx(p, 0.5, 0.0)));
    assert!(matches!(path[2], Call::LineTo(p) if point_approx(p, 4.5, 0.0)));
    assert!(matches!(path[3], Call::LineTo(p) if point_approx(p, 4.5, 4.0)));
    assert!(matches!(path[4], Call::LineTo(p) if point_approx(p, 0.5, 4.0)));
    assert!(matches!(path[5], Call::ClosePath));
    assert!(matches!(path[6], Call::MoveTo(p) if point_approx(p, 5.0, 0.0)));
    assert!(run.shows().is_empty());
}

#[test]
fn mixed_byte_lengths_and_partial_matches_decode_through_the_cmap() {
    let program = format!(
        "{}/M /Syn-H [ /SynCID /CIDFont findresource ] composefont 10 scalefont setfont \
         0 0 moveto <41814042> show currentpoint \
         0 0 moveto <41812042> show currentpoint \
         <41812042> stringwidth",
        corpus_cmap()
    );
    let run = exec(&with_cid_set(&program));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(6);
    assert!(approx(top[0], 15.0) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 14.5), "the notdef advance between: {top:?}");
    assert!(approx(top[4], 14.5), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows.len(), 2);
    assert_eq!(
        shows[0],
        vec![
            glyph(0x41, 1, 34, 500.0, 0.0),
            glyph(0x8140, 2, 200, 300.0, 0.0),
            glyph(0x42, 1, 35, 700.0, 0.0),
        ]
    );
    assert_eq!(
        shows[1],
        vec![
            glyph(0x41, 1, 34, 500.0, 0.0),
            glyph(0x8120, 2, 0, 250.0, 0.0),
            glyph(0x42, 1, 35, 700.0, 0.0),
        ],
        "one notdef glyph of two bytes"
    );
    assert!(matches!(
        &run.sources()[0],
        FontSource::Composite { cmap_name, .. } if cmap_name == b"Syn-H"
    ));
}

#[test]
fn positioning_variants_consume_one_entry_per_decoded_code() {
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_H}/SynComposite findfont 10 scalefont setfont \
         0 0 moveto <00010002> [10 20] xshow currentpoint \
         0 0 moveto <00010002> [1 2 3 4] xyshow currentpoint \
         0 0 moveto <00010002> [7 7] yshow currentpoint \
         0 0 moveto 1 0 2 <00010002> widthshow currentpoint \
         0 0 moveto 1 1 <00010002> ashow currentpoint \
         {{ = = }} <00010002> kshow"
    )));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(10);
    assert!(
        approx(top[0], 30.0) && approx(top[1], 0.0),
        "xshow: {top:?}"
    );
    assert!(
        approx(top[2], 4.0) && approx(top[3], 6.0),
        "xyshow: {top:?}"
    );
    assert!(
        approx(top[4], 0.0) && approx(top[5], 14.0),
        "yshow: {top:?}"
    );
    assert!(
        approx(top[6], 13.0) && approx(top[7], 0.0),
        "widthshow on code 2: {top:?}"
    );
    assert!(
        approx(top[8], 14.0) && approx(top[9], 2.0),
        "ashow: {top:?}"
    );
    assert_eq!(run.output, "2\n1\n", "kshow hands the decoded codes");
    let shows = run.shows();
    assert!(
        glyph_approx(shows[0][0], glyph(1, 2, 1, 1000.0, 0.0)),
        "{:?}",
        shows[0]
    );
    assert!(
        glyph_approx(shows[0][1], glyph(2, 2, 2, 2000.0, 0.0)),
        "{:?}",
        shows[0]
    );
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_H}/SynComposite findfont 10 scalefont setfont 0 0 moveto <00010002> [10] xshow"
    )));
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec(&with_cid_set(&format!(
        "{COMPOSE_H}/SynComposite findfont 10 scalefont setfont 0 0 moveto /a glyphshow"
    )));
    assert_eq!(run.error(), Some("invalidfont"));
}

#[test]
fn a_simple_descendant_takes_the_cid_as_its_code() {
    let run = exec(
        b"/H /Identity-H [ /Helvetica findfont ] composefont 12 scalefont setfont \
          0 0 moveto <00480069> show currentpoint <0048> stringwidth",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(4);
    assert!(approx(top[0], 11.328) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 8.664), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows[0][0], glyph(0x48, 2, 0x48, 722.0, 0.0));
    assert!(matches!(
        &run.sources()[0],
        FontSource::Composite { descendant, .. }
            if matches!(**descendant, FontSource::Resident(ps_fonts::ResidentFace::Helvetica))
    ));
    let run = exec(
        b"/Sq << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
          /BuildGlyph { pop pop 1000 0 setcharwidth } >> definefont pop \
          /T /Identity-H [ /Sq findfont ] composefont 12 scalefont setfont \
          0 0 moveto <0048> show",
    );
    assert_eq!(run.error(), Some("invalidfont"), "a Type 3 descendant");
}

#[test]
fn a_cmap_selecting_another_descendant_is_refused() {
    let program = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
         /CMapName /Two-H def \
         1 begincodespacerange <0000> <ffff> endcodespacerange \
         1 begincidrange <0000> <00ff> 0 endcidrange \
         1 usefont 1 begincidrange <0100> <01ff> 0 endcidrange \
         endcmap CMapName currentdict /CMap defineresource pop end end \
         /D /Two-H [ /SynCID /CIDFont findresource /SynCID /CIDFont findresource ] composefont \
         10 scalefont setfont <0001> stringwidth <0101> stringwidth",
    );
    let run = exec(&with_cid_set(&program));
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("stringwidth"));
    let top = run.top_numbers(2);
    assert!(
        approx(top[0], 5.0),
        "the first descendant measured: {top:?}"
    );
    assert_eq!(run.interp.ostack().len(), 2, "the second failed mid-run");
}
