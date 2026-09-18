// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! In-process execution of one program: a fresh interpreter with capture
//! streams (an empty, readable `%stdin`), the graphics backend with a
//! collecting sink, and the profile's execution budget. A panic inside
//! the run is caught and reported; the interpreter it happened in is
//! never touched again.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use efterscript_graphics::{Graphics, Page, dump};
use efterscript_remelt::Options;
use efterscript_vm::{Capture, Config, Interp, Io, Limits, Outcome, SliceSource};

/// What one run produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    /// The uncaught error the job ended in, by name.
    pub error: Option<String>,
    /// The offending command of that error.
    pub command: Option<String>,
    pub output: String,
    pub stderr: String,
    /// The concatenated `ir/1` dump of the delivered pages.
    pub ir: String,
    pub pages: usize,
    pub steps: u64,
    pub budget_exceeded: bool,
    /// The panic message, when the run panicked; the other fields then
    /// hold what was captured before it.
    pub panic: Option<String>,
}

impl Run {
    /// The outcome as one word: `ok`, the error name, or `panic`.
    pub fn outcome(&self) -> String {
        if self.panic.is_some() {
            "panic".to_string()
        } else {
            self.error.clone().unwrap_or_else(|| "ok".to_string())
        }
    }
}

fn config(budget: u64) -> (Config, Capture, Capture) {
    let (io, out, err) = Io::capture();
    let io = io.with_stdin(Capture::new());
    let config = Config {
        io,
        limits: Limits {
            steps: Some(budget),
            ..Limits::default()
        },
        ..Default::default()
    };
    (config, out, err)
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-text payload".to_string()
    }
}

/// Runs `program` with the graphics backend and `budget` executed
/// objects.
pub fn execute(program: &str, budget: u64) -> Run {
    let (config, out, err) = config(budget);
    let pages: Rc<RefCell<Vec<Page>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = pages.clone();
    let result = catch_unwind(AssertUnwindSafe(move || {
        let mut interp = Interp::with_config(config);
        interp.set_graphics_backend(Box::new(Graphics::new(sink)));
        let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
        (outcome, interp.steps(), interp.budget_exceeded())
    }));
    let mut run = Run {
        output: out.text(),
        stderr: err.text(),
        ..Default::default()
    };
    match result {
        Ok((outcome, steps, budget_exceeded)) => {
            match outcome {
                Outcome::Ok => {}
                Outcome::Error(summary) => {
                    run.error = Some(summary.name);
                    run.command = Some(summary.command);
                }
                Outcome::Suspended => run.error = Some("<suspended>".to_string()),
            }
            run.steps = steps;
            run.budget_exceeded = budget_exceeded;
            let pages = pages.take();
            run.pages = pages.len();
            run.ir = dump::pages(&pages);
        }
        Err(payload) => run.panic = Some(panic_message(&*payload)),
    }
    run
}

/// Distils `program` to an uncompressed document in memory.
pub fn distill(program: &str, budget: u64) -> Result<Vec<u8>, String> {
    let (config, _, _) = config(budget);
    let options = Options::compress(false).lock("CompressPages");
    let result = catch_unwind(AssertUnwindSafe(move || {
        efterscript_remelt::distill(program.as_bytes(), config, &options, Vec::new())
    }));
    match result {
        Ok(Ok((_, bytes))) => Ok(bytes),
        Ok(Err(e)) => Err(e.to_string()),
        Err(payload) => Err(format!("panic: {}", panic_message(&*payload))),
    }
}

/// Installs a panic hook that prints nothing, so a caught panic does not
/// clutter the report; the message still reaches [`Run::panic`].
pub fn silence_panics() {
    std::panic::set_hook(Box::new(|_| {}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_capture_output_outcome_and_pages() {
        let run = execute("1 2 add = 0 0 moveto 10 10 lineto stroke showpage", 10_000);
        assert_eq!(run.output, "3\n");
        assert_eq!(run.error, None);
        assert_eq!(run.outcome(), "ok");
        assert_eq!(run.pages, 1);
        assert!(run.ir.contains("m 0 0\nl 10 10\nS\n"));
        assert!(run.steps > 0);
        assert!(!run.budget_exceeded);
        let failed = execute("(x) = 1 0 div", 10_000);
        assert_eq!(failed.error.as_deref(), Some("undefinedresult"));
        assert_eq!(failed.command.as_deref(), Some("div"));
        assert_eq!(failed.outcome(), "undefinedresult");
        assert!(failed.stderr.contains("OffendingCommand: div"));
        let stdin = execute("(%stdin) (r) file closefile", 10_000);
        assert_eq!(stdin.error, None);
    }

    #[test]
    fn the_budget_holds() {
        let run = execute("{ } loop", 5_000);
        assert_eq!(run.error.as_deref(), Some("limitcheck"));
        assert!(run.budget_exceeded);
    }

    #[test]
    fn distillation_yields_a_document() {
        let pdf = distill("0 0 10 10 rectfill showpage", 10_000).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.7\n"));
        assert!(String::from_utf8_lossy(&pdf).contains("\nf\n"));
        let none = distill("1 2 add", 10_000).unwrap();
        assert!(none.starts_with(b"%PDF-1.7\n"));
    }

    #[test]
    fn panics_are_caught_with_their_message() {
        let text: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&*text), "boom");
        let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("bang"));
        assert_eq!(panic_message(&*owned), "bang");
        let other: Box<dyn std::any::Any + Send> = Box::new(7u8);
        assert_eq!(panic_message(&*other), "panic with a non-text payload");
    }
}
