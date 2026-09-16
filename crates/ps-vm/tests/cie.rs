// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! CIE-based colour spaces at the boundary (PLRM3 §4.8.3): the four
//! dictionaries and their errors, the clamp and the initial colour, what
//! `currentcolor` and `currentcolorspace` answer, the structural collapse
//! into the calibrated spaces, the device getters, and the rendering
//! operators and categories (§7.1, §3.9).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use ps_vm::{Config, ImageSpec, Interp, Io, Outcome, SliceSource, SpaceSpec};

struct Run {
    outcome: Outcome,
    output: String,
    log: Log,
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

    /// The colour spaces the backend was told, in order.
    fn spaces(&self) -> Vec<SpaceSpec> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::ColorSpace(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// The colours the backend was told, in order.
    fn colours(&self) -> Vec<Vec<f32>> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Color(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// The images the backend was handed, in order.
    fn images(&self) -> Vec<(ImageSpec, Vec<u8>)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Image(spec, data) => Some((spec, data)),
                _ => None,
            })
            .collect()
    }
}

fn exec(program: &str) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        outcome,
        output: out.text(),
        log,
    }
}

fn exec_without_backend(program: &str) -> (Outcome, String) {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    (outcome, out.text())
}

/// A D65-like white point, and the identity `TransformPQR` procedures a
/// rendering dictionary needs.
const DEFS: &str = "/W [0.9505 1 1.089] def \
    /T [{exch pop exch pop exch pop exch pop} dup dup] def ";

fn with_defs(program: &str) -> Run {
    exec(&format!("{DEFS}{program}"))
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}

const WHITE: [f32; 3] = [0.9505, 1.0, 1.089];
const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// The a*/b* range of every converted space.
const LAB_RANGE: [f32; 4] = [-128.0, 127.0, -128.0, 127.0];

/// Within a hundredth: the conversion's double precision rounded to the
/// boundary's single.
fn near(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

/// XYZ to L*a*b* relative to `WHITE`, written from the formulas of the
/// L*a*b* transformation independently of the interpreter, with the
/// clamps the boundary applies.
fn lab_of(xyz: [f64; 3]) -> Vec<f32> {
    let f = |v: f64| {
        if v > 216.0 / 24389.0 {
            v.cbrt()
        } else {
            v * 841.0 / 108.0 + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / 0.9505), f(xyz[1]), f(xyz[2] / 1.089));
    vec![
        (116.0 * fy - 16.0).clamp(0.0, 100.0) as f32,
        (500.0 * (fx - fy)).clamp(-128.0, 127.0) as f32,
        (200.0 * (fy - fz)).clamp(-128.0, 127.0) as f32,
    ]
}

// --- selection, initial colour, getters --------------------------------------------

#[test]
fn a_cie_space_starts_black_and_the_device_getters_answer_their_initial_values() {
    let run = with_defs(
        "[/CIEBasedABC << /WhitePoint W >>] setcolorspace \
         currentcolor == == == currentrgbcolor == == == currentgray == \
         currentcmykcolor == == == == currenthsbcolor == == == \
         [/CIEBasedA << /WhitePoint W >>] setcolorspace 0.5 setcolor \
         currentgray == currentrgbcolor == == ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "0.0\n0.0\n0.0\n0.0\n0.0\n0.0\n0.0\n1.0\n0.0\n0.0\n0.0\n0.0\n0.0\n0.0\n\
         0.0\n0.0\n0.0\n0.0\n"
    );
    let spaces = run.spaces();
    let SpaceSpec::CalRGB {
        white,
        black,
        gamma,
        matrix,
    } = &spaces[0]
    else {
        panic!("the XYZ-like space collapses to CalRGB: {spaces:?}");
    };
    assert!(close(white, &WHITE));
    assert_eq!(*black, [0.0; 3]);
    assert_eq!(*gamma, [1.0; 3]);
    assert_eq!(*matrix, IDENTITY);
    assert_eq!(run.colours()[0], vec![0.0; 3]);
}

#[test]
fn components_are_clamped_to_their_ranges_and_kept_for_currentcolor() {
    let run = with_defs(
        "[/CIEBasedA << /WhitePoint W /MatrixA W /RangeA [0 0.5] >>] setcolorspace \
         2 setcolor currentcolor == -1 setcolor currentcolor == \
         [/CIEBasedA << /WhitePoint W /RangeA [0.5 1] >>] setcolorspace currentcolor == \
         [/CIEBasedA << /WhitePoint W /RangeA [-2 -1] >>] setcolorspace currentcolor == \
         [/CIEBasedDEFG << /WhitePoint W /RangeDEFG [0 2 0 2 0 2 0 2] \
           /Table [2 2 2 2 [[<000000000000000000000000> <000000000000000000000000>] \
                            [<000000000000000000000000> <000000000000000000000000>]]] >>] \
         setcolorspace 3 1 1 1 setcolor currentcolor == == == == \
         [/CIEBasedABC << /WhitePoint W /RangeABC [0 100 -128 127 -128 127] >>] setcolorspace \
         50 200 -200 setcolor currentcolor == == == count ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "0.5\n0.0\n0.5\n-1.0\n1.0\n1.0\n1.0\n2.0\n-128.0\n127.0\n50.0\n0\n"
    );
    // Each `setcolorspace` sets the initial colour; the first space
    // collapses to CalGray and its components pass through, the others
    // are carried as Lab and converted: 0.5 through the default unit
    // MatrixA is XYZ (0.5, 0.5, 0.5); −1 and the table of zeros give
    // black; (50, 127, −128) through the identity clamps to LMN (1, 1, 0).
    let spaces = run.spaces();
    assert!(matches!(spaces[0], SpaceSpec::CalGray { .. }));
    assert!(
        spaces[1..]
            .iter()
            .all(|s| matches!(s, SpaceSpec::Lab { .. }))
    );
    let colours = run.colours();
    assert_eq!(colours[..3], [vec![0.0], vec![0.5], vec![0.0]]);
    assert!(near(&colours[3], &lab_of([0.5, 0.5, 0.5])), "{colours:?}");
    assert_eq!(colours[4..8], vec![vec![0.0, 0.0, 0.0]; 4]);
    assert!(near(&colours[8], &[100.0, 8.533, 127.0]), "{colours:?}");
    assert_eq!(colours.len(), 9);
}

#[test]
fn setcolor_operand_errors_in_a_cie_space() {
    for (program, error) in [
        (
            "[/CIEBasedABC << /WhitePoint W >>] setcolorspace 1 2 setcolor",
            "stackunderflow",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W >>] setcolorspace 1 2 (x) setcolor",
            "typecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W >>] setcolorspace setcolor",
            "stackunderflow",
        ),
    ] {
        let run = with_defs(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
    let run =
        with_defs("[/CIEBasedABC << /WhitePoint W >>] setcolorspace 1 2 3 4 setcolor count =");
    assert_eq!(run.output, "1\n");
}

#[test]
fn currentcolorspace_returns_the_array_as_given() {
    let run = with_defs(
        "/cs [/CIEBasedABC << /WhitePoint W /DecodeABC [{2.2 exp} {2.2 exp} {2.2 exp}] >>] def \
         cs setcolorspace currentcolorspace cs eq = \
         currentcolorspace 1 get /DecodeABC get 0 get == \
         gsave /DeviceRGB setcolorspace currentcolorspace == grestore currentcolorspace cs eq = \
         /ix [/Indexed cs 1 <000000ffffff>] def ix setcolorspace 1 setcolor \
         currentcolorspace ix eq = currentcolor == \
         /pt [/Pattern [/CIEBasedA << /WhitePoint W /MatrixA W >>]] def pt setcolorspace \
         currentcolorspace pt eq = currentcolor ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "true\n{2.2 exp}\n[/DeviceRGB]\ntrue\ntrue\n1\ntrue\nnull\n"
    );
    let spaces = run.spaces();
    assert!(matches!(spaces[2], SpaceSpec::Indexed { .. }));
    let SpaceSpec::Pattern { base: Some(base) } = &spaces[3] else {
        panic!("{spaces:?}");
    };
    assert!(matches!(**base, SpaceSpec::CalGray { gamma, .. } if gamma == 1.0));
}

#[test]
fn the_cie_colour_is_saved_and_restored_with_the_state() {
    let run = with_defs(
        "/cs [/CIEBasedA << /WhitePoint W >>] def cs setcolorspace 0.5 setcolor \
         gsave 0.25 setcolor currentcolor == grestore currentcolor == \
         save 0.75 setcolor /DeviceRGB setcolorspace restore currentcolor == \
         currentcolorspace cs eq = \
         initgraphics currentcolorspace ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0.25\n0.5\n0.5\ntrue\n[/DeviceGray]\n");
}

// --- the dictionaries' errors ------------------------------------------------------

#[test]
fn dictionary_errors() {
    const OK_TABLE: &str = "/Table [2 2 2 [<000000000000000000000000> <000000000000000000000000>]]";
    for (program, error) in [
        ("[/CIEBasedABC << >>] setcolorspace", "undefined"),
        (
            "[/CIEBasedA << /RangeA [0 1] >>] setcolorspace",
            "undefined",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W >>] setcolorspace",
            "undefined",
        ),
        (
            "[/CIEBasedABC << /WhitePoint [0.9505 1.0001 1.089] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint [0 1 1.089] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint [0.9505 1 -1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint [1 1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint 5 >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint [1 (a) 1] >>] setcolorspace",
            "typecheck",
        ),
        ("[/CIEBasedABC 5] setcolorspace", "typecheck"),
        ("[/CIEBasedABC] setcolorspace", "rangecheck"),
        (
            "[/CIEBasedABC << /WhitePoint W >> 1] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /RangeABC [1 0 0 1 0 1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /RangeABC [0 1 0 1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /RangeABC 5 >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /DecodeABC [{} {}] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /DecodeABC [1 2 3] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /DecodeABC {} >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /DecodeABC [{} {} [1]] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /MatrixABC [1 0 0 0 1 0 0 0] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /MatrixABC [1 0 0 0 1 0 0 0 (x)] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /MatrixABC 7 >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /BlackPoint [-1 0 0] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /BlackPoint [0 0] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /RangeLMN [0 1 0 1 1 0] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /DecodeLMN [{} {}] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedABC << /WhitePoint W /MatrixLMN [1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W /DecodeA [{}] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W /DecodeA 5 >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W /MatrixA [1 1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W /RangeA [0 1 0 1] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedA << /WhitePoint W /RangeA [1 0] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table 5 >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [1 2 2 [<000000000000> <000000000000>]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2 2 [<0000000000000000000000> <000000000000000000000000>]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2 2 [<000000000000000000000000>]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2 2 <000000000000000000000000>] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2 [<000000000000000000000000> <000000000000000000000000>]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2.0 2 [<000000000000000000000000> <000000000000000000000000>]] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedDEF << /WhitePoint W /Table [2 2 2 [<000000000000000000000000> 5]] >>] setcolorspace",
            "typecheck",
        ),
        (
            "[/CIEBasedDEFG << /WhitePoint W /Table [2 2 2 2 [[<000000000000000000000000> <000000000000000000000000>]]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEFG << /WhitePoint W /Table [2 2 2 [[<000000000000000000000000> <000000000000000000000000>] [<000000000000000000000000> <000000000000000000000000>]]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEFG << /WhitePoint W /Table [2 2 2 2 [[<000000000000000000000000> <0000000000000000000000>] [<000000000000000000000000> <000000000000000000000000>]]] >>] setcolorspace",
            "rangecheck",
        ),
        (
            "[/CIEBasedDEFG << /WhitePoint W /Table [2 2 2 2 [<000000000000000000000000> <000000000000000000000000>]] >>] setcolorspace",
            "typecheck",
        ),
    ] {
        let run = with_defs(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
    for program in [
        &format!("[/CIEBasedDEF << /WhitePoint W {OK_TABLE} >>] setcolorspace"),
        &format!(
            "[/CIEBasedDEF << /WhitePoint W /RangeDEF [0 1 0 1 0 1] /DecodeDEF [{{}} {{}} {{}}] /RangeHIJ [0 1 0 1 0 1] {OK_TABLE} >>] setcolorspace"
        ),
        "[/CIEBasedDEFG << /WhitePoint W /Table [2 2 2 2 [[<000000000000000000000000> <000000000000000000000000>] [<000000000000000000000000> <000000000000000000000000>]]] >>] setcolorspace",
        "[/CIEBasedABC << /WhitePoint W /BlackPoint [0.01 0.01 0.01] /RangeABC [0 1 0 1 0 1] /DecodeABC [{} {} {}] /MatrixABC [1 0 0 0 1 0 0 0 1] /RangeLMN [0 1 0 1 0 1] /DecodeLMN [{} {} {}] /MatrixLMN [1 0 0 0 1 0 0 0 1] >>] setcolorspace",
        "[/CIEBasedA << /WhitePoint W /RangeA [0 1] /DecodeA {} /MatrixA [1 1 1] >>] setcolorspace",
    ] {
        let run = with_defs(program);
        assert_eq!(run.outcome, Outcome::Ok, "{program}");
    }
}

// --- the collapse test -------------------------------------------------------------------

fn only_space(program: &str) -> SpaceSpec {
    let run = with_defs(program);
    assert_eq!(run.outcome, Outcome::Ok, "{program}");
    let spaces = run.spaces();
    assert_eq!(spaces.len(), 1, "{program}: {spaces:?}");
    spaces.into_iter().next().unwrap()
}

fn gray_gamma(program: &str) -> Option<f32> {
    match only_space(program) {
        SpaceSpec::CalGray { gamma, .. } => Some(gamma),
        SpaceSpec::Lab { .. } => None,
        other => panic!("{program}: {other:?}"),
    }
}

#[test]
fn a_gray_space_collapses_on_the_shape_of_its_decode_procedure() {
    let stage = "/MatrixA W";
    for (decode, gamma) in [
        ("/DecodeA {2.2 exp}", Some(2.2)),
        ("/DecodeA {2.2 exp} bind", Some(2.2)),
        ("/DecodeA {}", Some(1.0)),
        ("", Some(1.0)),
        ("/DecodeA {1.8 exp} bind", Some(1.8)),
        ("/DecodeA {2.2 exp pop 1}", None),
        ("/DecodeA {-2 exp} /RangeA [0.5 1]", None),
        ("/DecodeA {0 exp}", None),
        ("/DecodeA {dup exp}", None),
        ("/DecodeA {2.2 /exp pop pop}", None),
        ("/DecodeA {2.2 mul}", None),
    ] {
        let program = format!("[/CIEBasedA << /WhitePoint W {stage} {decode} >>] setcolorspace");
        let got = gray_gamma(&program);
        match (got, gamma) {
            (Some(g), Some(want)) => assert!((g - want).abs() < 1e-6, "{program}: {g}"),
            (None, None) => {}
            _ => panic!("{program}: {got:?}, wanted {gamma:?}"),
        }
    }
    // The matrix must be the white point, the LMN stage the identity,
    // and the ranges must not widen the component beyond the unit
    // interval; an explicit LMN range must contain the stage's output.
    for (entries, collapses) in [
        ("/MatrixA W /RangeLMN [0 0.9505 0 1 0 1.089]", true),
        ("/MatrixA W /RangeA [0 0.5]", true),
        ("/MatrixA W /RangeA [0 2]", false),
        ("/MatrixA W /RangeLMN [0 1 0 1 0 1.05]", false),
        ("/MatrixA [1 1 1]", false),
        ("", false),
        ("/MatrixA W /DecodeLMN [{} {} {}]", true),
        ("/MatrixA W /DecodeLMN [{} {2.2 exp} {}]", false),
        ("/MatrixA W /MatrixLMN [1 0 0 0 1 0 0 0 1]", true),
        ("/MatrixA W /MatrixLMN [1 0 0 0 1 0 0 0 0.9]", false),
    ] {
        let program = format!("[/CIEBasedA << /WhitePoint W {entries} >>] setcolorspace");
        assert_eq!(gray_gamma(&program).is_some(), collapses, "{program}");
    }
}

fn rgb_shape(program: &str) -> Option<([f32; 3], [f32; 9])> {
    match only_space(program) {
        SpaceSpec::CalRGB { gamma, matrix, .. } => Some((gamma, matrix)),
        SpaceSpec::Lab { .. } => None,
        other => panic!("{program}: {other:?}"),
    }
}

#[test]
fn an_rgb_space_collapses_when_one_stage_carries_the_gammas_and_the_matrix() {
    let primaries = "[0.4 0.2 0.02 0.35 0.7 0.1 0.2 0.1 0.95]";
    let matrix = [0.4, 0.2, 0.02, 0.35, 0.7, 0.1, 0.2, 0.1, 0.95];
    // The LMN stage carries them.
    let program = format!(
        "[/CIEBasedABC << /WhitePoint [0.95 1 1.07] /DecodeLMN [{{1.8 exp}} {{1.8 exp}} {{1.8 exp}}] \
         /MatrixLMN {primaries} >>] setcolorspace"
    );
    let (gamma, got) = rgb_shape(&program).expect("collapses");
    assert!(close(&gamma, &[1.8; 3]));
    assert!(close(&got, &matrix));
    // The ABC stage carries them; a default LMN range does not stop it.
    let program = format!(
        "[/CIEBasedABC << /WhitePoint [0.95 1 1.07] /DecodeABC [{{2.2 exp}} bind {{2.2 exp}} {{1.5 exp}}] \
         /MatrixABC {primaries} >>] setcolorspace"
    );
    let (gamma, got) = rgb_shape(&program).expect("collapses");
    assert!(close(&gamma, &[2.2, 2.2, 1.5]));
    assert!(close(&got, &matrix));
    // The XYZ space: identity stages with the ranges the components take.
    let program = "[/CIEBasedABC << /WhitePoint W /RangeABC [0 0.9505 0 1 0 1] \
                    /RangeLMN [0 0.9505 0 1 0 1] >>] setcolorspace";
    let (gamma, got) = rgb_shape(program).expect("collapses");
    assert_eq!(gamma, [1.0; 3]);
    assert_eq!(got, IDENTITY);
    // An ABC matrix followed by an LMN matrix fold into one.
    let program = "[/CIEBasedABC << /WhitePoint W /MatrixABC [2 0 0 0 1 0 0 0 1] \
                    /MatrixLMN [1 1 0 0 1 0 0 0 1] /RangeLMN [0 2 0 1 0 1] >>] setcolorspace";
    let (_, got) = rgb_shape(program).expect("collapses");
    assert!(close(&got, &[2.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    for entries in [
        // Both stages carry gammas.
        "/DecodeABC [{2.2 exp} {2.2 exp} {2.2 exp}] /DecodeLMN [{1.8 exp} {1.8 exp} {1.8 exp}]",
        // The ABC matrix is not the identity while the LMN stage decodes.
        "/MatrixABC [0.5 0 0 0 1 0 0 0 1] /DecodeLMN [{1.8 exp} {1.8 exp} {1.8 exp}]",
        // A procedure that is not a gamma.
        "/DecodeABC [{2.2 exp} {dup mul} {2.2 exp}]",
        // A range beyond the unit interval.
        "/RangeABC [0 2 0 1 0 1]",
        // An explicit LMN range narrower than the ABC stage's output.
        "/MatrixABC [0.4 0.2 0.02 0.35 0.7 0.1 0.2 0.1 0.95] /RangeLMN [0 0.9 0 1 0 1.07]",
        "/RangeABC [0 1 0 1 0 1] /RangeLMN [0 0.5 0 1 0 1]",
    ] {
        let program = format!("[/CIEBasedABC << /WhitePoint W {entries} >>] setcolorspace");
        assert!(rgb_shape(&program).is_none(), "{program}");
    }
    // The table families never collapse.
    let program = "[/CIEBasedDEF << /WhitePoint W /BlackPoint [0.01 0.02 0.03] \
                    /Table [2 2 2 [<000000000000000000000000> <000000000000000000000000>]] >>] setcolorspace";
    let SpaceSpec::Lab {
        white,
        black,
        range,
    } = only_space(program)
    else {
        panic!("a table space is carried as Lab");
    };
    assert!(close(&white, &WHITE));
    assert!(close(&black, &[0.01, 0.02, 0.03]));
    assert_eq!(range, LAB_RANGE);
}

// --- the conversion to L*a*b* ----------------------------------------------------------

/// A `CIEBasedABC` dictionary shaped as the L*a*b* space itself, written
/// from the two-stage transformation: the first stage forms the three
/// arguments of `g`, the second applies `g` and the white point.
const LAB_SHAPED: &str = "/G { dup 0.20690 ge { dup dup mul mul } { 0.13793 sub 0.12842 mul } ifelse } def \
    /LabSpace [/CIEBasedABC << /WhitePoint W \
      /RangeABC [0 100 -128 127 -128 127] \
      /DecodeABC [ { 116 div 0.137931 add } { 0.002 mul } { 0.005 mul } ] \
      /MatrixABC [1 1 1  1 0 0  0 0 -1] \
      /RangeLMN [-1 2 -1 2 -1 2] \
      /DecodeLMN [ { G 0.9505 mul } { G } { G 1.089 mul } ] >>] def ";

#[test]
fn a_lab_shaped_space_round_trips_its_components() {
    let run = with_defs(&format!(
        "{LAB_SHAPED} LabSpace setcolorspace 50 20 -30 setcolor currentcolor == == == \
         0 0 0 setcolor 100 0 0 setcolor 3 -128 127 setcolor count =="
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "-30.0\n20.0\n50.0\n0\n");
    let SpaceSpec::Lab {
        white,
        black,
        range,
    } = &run.spaces()[0]
    else {
        panic!("carried as Lab: {:?}", run.spaces());
    };
    assert!(close(white, &WHITE));
    assert_eq!(*black, [0.0; 3]);
    assert_eq!(*range, LAB_RANGE);
    let colours = run.colours();
    // The initial colour, then the four set: each comes back within the
    // precision of the procedures' single-precision arithmetic (both
    // branches of `g` are exercised: L* 3 lies below the knee).
    let tolerance = 0.05;
    let expected = [
        [0.0, 0.0, 0.0],
        [50.0, 20.0, -30.0],
        [0.0, 0.0, 0.0],
        [100.0, 0.0, 0.0],
        [3.0, -128.0, 127.0],
    ];
    assert_eq!(colours.len(), expected.len(), "{colours:?}");
    for (got, want) in colours.iter().zip(&expected) {
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() < tolerance),
            "{got:?} against {want:?}"
        );
    }
}

/// A 2×2×2 table whose entry at (h, i, j) is the bytes (10 + 30k, 20 +
/// 30k, 30 + 30k) with k the corner's number h·4 + i·2 + j.
const DEF_TABLE: &str = "/Table [2 2 2 [<0a141e28323c46505a646e78> <828c96a0aab4bec8d2dce6f0>]]";

#[test]
fn a_table_space_interpolates_its_entries() {
    let run = with_defs(&format!(
        "[/CIEBasedDEF << /WhitePoint W {DEF_TABLE} >>] setcolorspace \
         1 0 1 setcolor 0.5 0 0 setcolor 0.25 1 0.75 setcolor"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let colours = run.colours();
    // Corner (1, 0, 1) is entry 5; the midpoint of the h edge at i = j =
    // 0 averages entries 0 and 4; the last mixes all eight.
    let entry = |k: f64| [10.0 + 30.0 * k, 20.0 + 30.0 * k, 30.0 + 30.0 * k];
    let xyz = |abc: [f64; 3]| [abc[0] / 255.0, abc[1] / 255.0, abc[2] / 255.0];
    assert!(near(&colours[1], &lab_of(xyz(entry(5.0)))), "{colours:?}");
    assert!(near(&colours[2], &lab_of(xyz(entry(2.0)))), "{colours:?}");
    // h = 0.25, i = 1, j = 0.75: weights over k = h·4 + 2 + j with
    // the two axes' fractions.
    let mixed = 0.75 * 0.25 * 2.0 + 0.75 * 0.75 * 3.0 + 0.25 * 0.25 * 6.0 + 0.25 * 0.75 * 7.0;
    assert!(near(&colours[3], &lab_of(xyz(entry(mixed)))), "{colours:?}");
    assert_eq!(colours.len(), 4);
}

#[test]
fn a_gray_space_with_a_non_white_matrix_converts() {
    // `MatrixA` scales the decoded value into XYZ; 0.5 through a gamma
    // of 2 is 0.25.
    let run = with_defs(
        "[/CIEBasedA << /WhitePoint W /DecodeA {2 exp} /MatrixA [0.5 0.5 0.5] >>] setcolorspace \
         0.5 setcolor currentcolor ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0.5\n");
    assert!(matches!(run.spaces()[0], SpaceSpec::Lab { .. }));
    assert!(near(&run.colours()[1], &lab_of([0.125, 0.125, 0.125])));
}

#[test]
fn a_decode_procedure_must_leave_a_number() {
    for program in [
        "[/CIEBasedA << /WhitePoint W /DecodeA {pop (x)} >>] setcolorspace",
        "[/CIEBasedABC << /WhitePoint W /RangeABC [1 2 0 1 0 1] /DecodeABC [{pop /n} {} {}] >>] \
         setcolorspace",
        "[/CIEBasedA << /WhitePoint W /RangeA [1 2] /DecodeA {pop} >>] setcolorspace",
        "[/CIEBasedA << /WhitePoint W /RangeA [1 2] /DecodeLMN [{} {} {pop [1]}] >>] setcolorspace",
    ] {
        let run = with_defs(program);
        let expected = if program.contains("{pop}") {
            "stackunderflow"
        } else {
            "typecheck"
        };
        assert_eq!(run.error(), Some(expected), "{program}");
    }
    // At `setcolor` too, leaving the colour as it was.
    let run = with_defs(
        "[/CIEBasedA << /WhitePoint W /DecodeA { dup 0.5 gt { pop (x) } if } >>] setcolorspace \
         0.25 setcolor { 0.75 setcolor } stopped == currentcolor ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\n0.25\n");
    let colours = run.colours();
    assert_eq!(colours.len(), 2);
    assert!(near(&colours[1], &lab_of([0.25, 0.25, 0.25])));
}

#[test]
fn a_procedure_that_raises_leaves_the_colour_and_the_stack_as_any_error_would() {
    // The error inside the conversion propagates to `stopped` with the
    // same residue the procedure leaves when run by itself with that
    // input; the colour set before stays, and the operands are gone.
    let run = with_defs(
        "[/CIEBasedA << /WhitePoint W /DecodeA { dup 0.5 gt { 1 0 div } if } >>] setcolorspace \
         0.25 setcolor \
         { 0.75 setcolor } stopped == count == clear \
         { 0.75 { dup 0.5 gt { 1 0 div } if } exec } stopped == count == clear \
         currentcolor ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let lines: Vec<&str> = run.output.lines().collect();
    assert_eq!(lines[0], "true");
    assert_eq!(lines[2], "true");
    assert_eq!(lines[1], lines[3], "{}", run.output);
    assert_eq!(lines[4], "0.25");
    let colours = run.colours();
    assert_eq!(colours.len(), 2);
    assert!(near(&colours[1], &lab_of([0.25, 0.25, 0.25])));
}

#[test]
fn a_procedure_leaving_more_than_its_result_is_cut_back() {
    let run = with_defs(
        "[/CIEBasedA << /WhitePoint W /DecodeA { 7 exch } >>] setcolorspace \
         0.5 setcolor count == currentcolor ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n0.5\n");
    assert!(near(&run.colours()[1], &lab_of([0.5, 0.5, 0.5])));
}

#[test]
fn the_colour_survives_save_and_restore_and_the_conversion_repeats() {
    let run = with_defs(&format!(
        "{LAB_SHAPED} LabSpace setcolorspace 60 10 -10 setcolor \
         save 20 -40 40 setcolor currentcolor == == == restore currentcolor == == == \
         gsave 0 0 0 setcolor grestore currentcolor == == =="
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "40.0\n-40.0\n20.0\n-10.0\n10.0\n60.0\n-10.0\n10.0\n60.0\n"
    );
}

// --- images ----------------------------------------------------------------------------------

/// `image` in the current space: `width`×`height` eight-bit RGB samples
/// from the string `data`.
fn rgb_image(width: u32, height: u32, data: &str) -> String {
    format!(
        "<< /ImageType 1 /Width {width} /Height {height} /BitsPerComponent 8 \
            /ImageMatrix [{width} 0 0 {height} 0 0] /DataSource {data} >> image"
    )
}

#[test]
fn a_two_pixel_image_converts_to_lab_bytes() {
    // The first pixel's A is halved: XYZ (0.5, 0, 0) is L* 0, a* beyond
    // the range, b* 0; the second is XYZ (0, 1, 0): L* 100, a* below the
    // range, b* beyond it.
    let run = with_defs(&format!(
        "[/CIEBasedABC << /WhitePoint W /DecodeABC [{{0.5 mul}} {{}} {{}}] >>] setcolorspace {}",
        rgb_image(2, 1, "<ff000000ff00>")
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let images = run.images();
    assert_eq!(images.len(), 1);
    let (spec, data) = &images[0];
    assert_eq!(data, &[0, 255, 128, 255, 0, 255]);
    assert_eq!(spec.bits_per_component, 8);
    assert_eq!((spec.width, spec.height), (2, 1));
    assert!(matches!(spec.color_space, Some(SpaceSpec::Lab { .. })));
    assert_eq!(spec.decode, [0.0, 100.0, -128.0, 127.0, -128.0, 127.0]);
}

#[test]
fn an_image_calls_each_procedure_once_per_distinct_sample() {
    // 512 pixels whose components run through every byte value twice:
    // the counting procedure runs 256 times for the image (once more
    // for the initial colour at `setcolorspace`), the identity never.
    let run = with_defs(&format!(
        "/n 0 def /d 1536 string def \
         0 1 511 {{ /k exch def d k 3 mul k 255 and put d k 3 mul 1 add k 255 and put \
                    d k 3 mul 2 add 0 put }} for \
         [/CIEBasedABC << /WhitePoint W /DecodeABC [{{/n n 1 add def}} {{}} {{}}] >>] setcolorspace \
         /n 0 def {} n ==",
        rgb_image(512, 1, "d")
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "256\n");
    let images = run.images();
    assert_eq!(images[0].1.len(), 512 * 3);
    // A four-bit image runs it at most sixteen times, a twelve-bit image
    // (reduced to eight) at most 256; the full sample is the white point
    // (the LMN range widened to admit it) and the empty one black.
    let run = with_defs(
        "/n 0 def [/CIEBasedA << /WhitePoint W /MatrixA W /RangeLMN [0 0.9505 0 1 0 1.089] \
           /DecodeA {/n n 1 add def} >>] setcolorspace /n 0 def \
         << /ImageType 1 /Width 4 /Height 2 /BitsPerComponent 4 /ImageMatrix [4 0 0 2 0 0] \
            /DataSource <01234567> >> image n == \
         /n 0 def \
         << /ImageType 1 /Width 2 /Height 1 /BitsPerComponent 12 /ImageMatrix [2 0 0 1 0 0] \
            /DataSource <fff000> >> image n ==",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "8\n2\n");
    let images = run.images();
    assert_eq!(images[1].1, [255, 128, 128, 0, 128, 128]);
}

#[test]
fn a_second_stage_with_many_distinct_inputs_is_snapped_to_a_grid() {
    // 256×64 samples whose L = A + B/256 takes 16384 distinct values
    // over most of `RangeLMN`: the LMN procedure runs at most 4096 times.
    let run = with_defs(&format!(
        "/n 0 def /d 49152 string def \
         0 1 16383 {{ /k exch def d k 3 mul k 255 and put d k 3 mul 1 add k -8 bitshift put \
                      d k 3 mul 2 add 0 put }} for \
         [/CIEBasedABC << /WhitePoint W /MatrixABC [1 0 0  0.00390625 1 0  0 0 1] \
            /RangeLMN [0 1.25 0 1 0 1] \
            /DecodeLMN [{{/n n 1 add def}} {{}} {{}}] >>] setcolorspace \
         {} n ==",
        rgb_image(256, 64, "d")
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let calls: usize = run.output.trim().parse().unwrap();
    // The 16384 values snap onto the grid points they are near — about
    // four per A value, over a thousand in all — never more than the
    // grid has and far more than the 256 samples of one component.
    assert!(calls <= 4096 && calls > 256, "{calls}");
}

#[test]
fn images_in_collapsed_spaces_pass_through_with_the_range_as_default_decode() {
    let run = with_defs(&format!(
        "[/CIEBasedABC << /WhitePoint W /RangeABC [0 0.9505 0 1 0 1] >>] setcolorspace {} \
         [/CIEBasedA << /WhitePoint W /MatrixA W /DecodeA {{2.2 exp}} >>] setcolorspace \
         << /ImageType 1 /Width 1 /Height 1 /BitsPerComponent 8 /ImageMatrix [1 0 0 1 0 0] \
            /Decode [1 0] /DataSource <80> >> image",
        rgb_image(1, 1, "<102030>")
    ));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let images = run.images();
    assert_eq!(images.len(), 2);
    assert!(matches!(
        images[0].0.color_space,
        Some(SpaceSpec::CalRGB { .. })
    ));
    assert_eq!(images[0].0.decode, [0.0, 0.9505, 0.0, 1.0, 0.0, 1.0]);
    assert_eq!(images[0].1, [0x10, 0x20, 0x30]);
    assert!(matches!(
        images[1].0.color_space,
        Some(SpaceSpec::CalGray { .. })
    ));
    assert_eq!(images[1].0.decode, [1.0, 0.0]);
}

#[test]
fn an_indexed_space_over_a_converting_base_converts_its_table_once() {
    let run = with_defs(
        "/S [/Indexed [/CIEBasedABC << /WhitePoint W /DecodeABC [{0.5 mul} {} {}] >>] 1 \
            <ff000000ff00>] def \
         S setcolorspace 1 setcolor currentcolor == currentcolorspace S eq ==",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "1\ntrue\n");
    let spaces = run.spaces();
    let SpaceSpec::Indexed {
        base,
        hival,
        lookup,
    } = &spaces[0]
    else {
        panic!("{spaces:?}");
    };
    assert!(matches!(**base, SpaceSpec::Lab { .. }));
    assert_eq!(*hival, 1);
    assert_eq!(lookup, &[0, 255, 128, 255, 0, 255]);
    // The index set; the initial index is the backend's own.
    assert_eq!(run.colours(), [vec![1.0]]);
}

// --- rendering operators and categories ----------------------------------------------

#[test]
fn a_rendering_dictionary_is_recorded_and_restored() {
    let run = with_defs(
        "currentcolorrendering /ColorRenderingType get == \
         /D /DefaultColorRendering /ColorRendering findresource def \
         currentcolorrendering D eq = D wcheck = \
         D /WhitePoint get == D /TransformPQR get length == \
         /d << /ColorRenderingType 1 /WhitePoint [0.9 1 1] /TransformPQR T >> def \
         d setcolorrendering currentcolorrendering d eq = \
         gsave D setcolorrendering currentcolorrendering D eq = grestore \
         currentcolorrendering d eq = \
         save D setcolorrendering restore currentcolorrendering d eq = \
         initgraphics currentcolorrendering d eq = \
         0.5 setgray 0 0 10 10 rectfill",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "1\ntrue\nfalse\n[0.9505 1.0 1.089]\n3\ntrue\ntrue\ntrue\ntrue\ntrue\n"
    );
    assert!(run.calls().contains(&Call::Color(vec![0.5])));
}

#[test]
fn rendering_dictionaries_are_kept_without_a_backend() {
    let (outcome, output) = exec_without_backend(&format!(
        "{DEFS} currentcolorrendering /ColorRenderingType get == \
         /d << /ColorRenderingType 1 /WhitePoint W /TransformPQR T >> def \
         d setcolorrendering currentcolorrendering d eq = \
         /Perceptual findcolorrendering == =="
    ));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "1\ntrue\nfalse\n/DefaultColorRendering\n");
}

#[test]
fn setcolorrendering_errors() {
    for (program, error) in [
        ("<< /ColorRenderingType 1 >> setcolorrendering", "undefined"),
        (
            "<< /ColorRenderingType 1 /WhitePoint W >> setcolorrendering",
            "undefined",
        ),
        (
            "<< /ColorRenderingType 2 /WhitePoint W /TransformPQR T >> setcolorrendering",
            "rangecheck",
        ),
        (
            "<< /ColorRenderingType 1.0 /WhitePoint W /TransformPQR T >> setcolorrendering",
            "typecheck",
        ),
        (
            "<< /WhitePoint W /TransformPQR T >> setcolorrendering",
            "undefined",
        ),
        (
            "<< /ColorRenderingType 1 /WhitePoint [1 1 1 1] /TransformPQR T >> setcolorrendering",
            "rangecheck",
        ),
        (
            "<< /ColorRenderingType 1 /WhitePoint W /TransformPQR [{} {}] >> setcolorrendering",
            "rangecheck",
        ),
        (
            "<< /ColorRenderingType 1 /WhitePoint W /TransformPQR [{} {} 1] >> setcolorrendering",
            "typecheck",
        ),
        ("5 setcolorrendering", "typecheck"),
        ("setcolorrendering", "stackunderflow"),
        ("5 findcolorrendering", "typecheck"),
        ("findcolorrendering", "stackunderflow"),
    ] {
        let run = with_defs(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
}

/// `findcolorrendering` forms `intent.device.none` — the device from the
/// page device's `PageDeviceName`, else `none` — and answers `true` only
/// when the `ColorRendering` category holds that instance; otherwise
/// the default instance's name and `false`.
#[test]
fn findcolorrendering_consults_the_category_under_the_composed_name() {
    let run = with_defs(
        "/Perceptual findcolorrendering == == \
         (Perceptual) findcolorrendering == == \
         /crd << /ColorRenderingType 1 /WhitePoint W /TransformPQR T >> def \
         /Perceptual crd /ColorRendering defineresource pop \
         /Perceptual findcolorrendering == == \
         /Perceptual.none.none crd /ColorRendering defineresource pop \
         /Perceptual findcolorrendering == == \
         /Perceptual findcolorrendering pop /ColorRendering findresource crd eq = \
         << /PageDeviceName /Foo >> setpagedevice \
         /Perceptual findcolorrendering == == \
         /Perceptual.Foo.none crd /ColorRendering defineresource pop \
         (Perceptual) findcolorrendering == ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "false\n/DefaultColorRendering\nfalse\n/DefaultColorRendering\n\
         false\n/DefaultColorRendering\ntrue\n/Perceptual.none.none\ntrue\n\
         false\n/DefaultColorRendering\ntrue\n/Perceptual.Foo.none\n"
    );
}

#[test]
fn the_rendering_and_colour_space_categories_are_regular() {
    let run = with_defs(
        "/DefaultColorRendering /ColorRendering resourcestatus == == == \
         /ColorRendering /Category resourcestatus == == == \
         /ColorSpace /Category resourcestatus == == == \
         true setglobal /G << /ColorRenderingType 1 /WhitePoint [0.9505 1 1.089] \
           /TransformPQR [{exch pop exch pop exch pop exch pop} dup dup] >> \
           /ColorRendering defineresource pop false setglobal \
         /G /ColorRendering resourcestatus 3 1 roll pop pop = \
         (*) { == } 64 string /ColorRendering resourceforall \
         /G /ColorRendering undefineresource /G /ColorRendering resourcestatus = \
         /DefaultColorRendering /ColorRendering undefineresource \
         /DefaultColorRendering /ColorRendering resourcestatus 3 1 roll pop pop = \
         /X [/DeviceRGB] /ColorSpace defineresource pop \
         /X /ColorSpace findresource == /X /ColorSpace findresource setcolorspace \
         currentcolorspace == \
         /Y [/CIEBasedA << /WhitePoint W >>] /ColorSpace defineresource setcolorspace \
         currentcolorspace 0 get == \
         (*) { == } 64 string /ColorSpace resourceforall \
         { /Z /DeviceRGB /ColorSpace defineresource } stopped pop $error /errorname get == \
         { /Z 5 /ColorSpace defineresource } stopped pop $error /errorname get == \
         { /Z << >> /ColorSpace defineresource } stopped pop $error /errorname get == \
         { /Z 5 /ColorRendering defineresource } stopped pop $error /errorname get == \
         { /Z << /ColorRenderingType 1 >> /ColorRendering defineresource } stopped pop \
           $error /errorname get == \
         { /Z << /ColorRenderingType 2 /WhitePoint W /TransformPQR T >> \
           /ColorRendering defineresource } stopped pop $error /errorname get == \
         (*) { == } 64 string /ColorSpaceFamily resourceforall \
         /CIEBasedDEFG /ColorSpaceFamily resourcestatus == == ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "true\n0\n0\ntrue\n0\n0\ntrue\n0\n0\ntrue\n(G)\n(DefaultColorRendering)\nfalse\ntrue\n\
         [/DeviceRGB]\n[/DeviceRGB]\n/CIEBasedA\n(X)\n(Y)\n\
         /typecheck\n/typecheck\n/typecheck\n/typecheck\n/typecheck\n/rangecheck\n\
         (CIEBasedA)\n(CIEBasedABC)\n(CIEBasedDEF)\n(CIEBasedDEFG)\n(DeviceCMYK)\n(DeviceGray)\n\
         (DeviceN)\n(DeviceRGB)\n(Indexed)\n(Pattern)\n(Separation)\ntrue\n0\n0\n"
    );
}

#[test]
fn usecieccolor_is_recorded_by_the_page_device() {
    let (outcome, output) = exec_without_backend(
        "<< /UseCIEColor true >> setpagedevice currentpagedevice /UseCIEColor get =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "true\n");
}
