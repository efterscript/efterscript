// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `rand`, `srand`, `rrand`, and the clocks, seen from a program: the
//! sequence is fixed without `srand`, reproduced after it, continued
//! from an `rrand` value, and the clocks move forward. The generator's
//! own properties (period, mixing) are unit tests in the interpreter
//! crate; the printed scenarios are corpus files under
//! `corpus/unit/interp`.

use efterscript_vm::{Capabilities, Clock, Config, Interp, Io, Outcome, SliceSource};

fn run(program: &str) -> (Interp, Outcome) {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    (interp, outcome)
}

fn ints(interp: &Interp) -> Vec<i32> {
    interp
        .ostack()
        .iter()
        .map(|o| o.as_i32().expect("integer"))
        .collect()
}

#[test]
fn rand_is_in_range_and_the_sequence_is_fixed_from_a_fresh_interpreter() {
    let (first, outcome) = run("rand rand rand rand");
    assert_eq!(outcome, Outcome::Ok);
    let (second, _) = run("rand rand rand rand");
    let values = ints(&first);
    assert_eq!(values, ints(&second), "the initial state is fixed");
    assert!(values.iter().all(|&v| v >= 0), "{values:?}");
    assert!(values.windows(2).any(|w| w[0] != w[1]), "{values:?}");
}

#[test]
fn srand_reproduces_a_sequence_and_rrand_continues_it() {
    let (interp, outcome) = run("7 srand rand rand rrand 7 srand rand rand rrand");
    assert_eq!(outcome, Outcome::Ok);
    let v = ints(&interp);
    // Two `srand 7` runs give the same pair and the same state.
    assert_eq!(&v[0..3], &v[3..6]);
    assert_ne!(v[0], v[1]);
    // Reseeding from the state `rrand` reported continues the sequence.
    let (interp, outcome) = run("7 srand rand pop rrand rand rand 3 -1 roll srand rand rand");
    assert_eq!(outcome, Outcome::Ok);
    let v = ints(&interp);
    assert_eq!(&v[0..2], &v[2..4]);
}

#[test]
fn srand_takes_any_integer_and_rrand_reports_a_non_negative_state() {
    let (interp, outcome) = run(
        "-1 srand rrand 2147483647 srand rrand -2147483648 srand rrand 0 srand rrand \
         -1 srand rand 2147483647 srand rand",
    );
    assert_eq!(outcome, Outcome::Ok);
    let v = ints(&interp);
    assert_eq!(v[0], 0x7FFF_FFFF, "the seed is masked to 31 bits");
    assert_eq!(v[1], 0x7FFF_FFFF);
    assert_eq!(v[2], 0);
    assert_eq!(v[3], 0);
    // Two seeds equal under the mask start the same sequence.
    assert_eq!(v[4], v[5]);
    let (_, outcome) = run("1.5 srand");
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "typecheck"));
}

#[test]
fn usertime_never_decreases_and_grows_with_execution() {
    let (interp, outcome) =
        run("usertime 0 1 20000 { pop } for usertime 0 1 20000 { pop } for usertime");
    assert_eq!(outcome, Outcome::Ok);
    let v = ints(&interp);
    assert!(v[0] <= v[1] && v[1] <= v[2], "{v:?}");
    assert!(v[2] > v[0], "sixty thousand steps are several ticks: {v:?}");
    assert_eq!(v[0], 0, "a fresh interpreter starts its clock at zero");
}

struct Ticking(i32);

impl Clock for Ticking {
    fn realtime_ms(&mut self) -> i32 {
        self.0 = self.0.wrapping_add(1000);
        self.0
    }
}

#[test]
fn realtime_reads_the_installed_clock() {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        capabilities: Capabilities {
            clock: Some(Box::new(Ticking(i32::MAX - 1500))),
            ..Default::default()
        },
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(b"realtime realtime realtime"));
    assert_eq!(outcome, Outcome::Ok);
    // The clock is read as it is, wrap included.
    assert_eq!(
        ints(&interp),
        vec![i32::MAX - 500, i32::MIN + 499, i32::MIN + 1499]
    );
}

#[test]
fn realtime_without_a_clock_is_the_execution_clock() {
    let (interp, outcome) = run("realtime usertime 0 1 20000 { pop } for realtime usertime");
    assert_eq!(outcome, Outcome::Ok);
    let v = ints(&interp);
    assert_eq!(v[0], v[1]);
    assert_eq!(v[2], v[3]);
    assert!(v[2] > v[0]);
}
