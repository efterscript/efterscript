// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The oracle-free properties and the metamorphic relations, evaluated
//! over one program run in process.
//!
//! - `no-panic`: the run did not panic.
//! - `budget`: the execution budget was not exceeded.
//! - `determinism`: a second run has the same outcome, output, error
//!   report, and IR dump.
//! - `pdf-structure`: the distilled document passes the structural
//!   check.
//! - `save-restore`: with the body (everything after the page setup and
//!   before `showpage` and the final print) wrapped in a named
//!   `save`/`restore`, the program behaves the same — or `restore` itself
//!   raises `invalidrestore` after identical output, which is what a body
//!   that leaves objects created inside it on the stack must do.
//! - `gsave-grestore`: wrapping a paint with the path statements it
//!   consumes in `gsave`/`grestore` leaves outcome, output, and IR
//!   unchanged. Candidates are `fill`, `eofill`, and `stroke` whose
//!   preceding statements are path construction back to a point with no
//!   current path (`newpath`, a paint, the page setup, or the start), and
//!   self-contained `rectfill`/`rectstroke` statements.
//! - `translate`: with `7 -3 translate` prepended (after the page setup),
//!   every IR coordinate and every stroke, text, and image matrix
//!   translation moves by (7, −3) and nothing else changes; numbers on
//!   the output may differ within floating-point rounding, since a
//!   reading through the CTM (`currentpoint`, `pathbbox`) comes back
//!   through a different translation.
//! - `reorder-defs`: swapping two adjacent `/name <closed expression>
//!   def` statements with distinct names leaves outcome, output, and IR
//!   unchanged. A closed expression is built from literals and pure
//!   operators alone; bare names disqualify a statement.

use crate::pdfcheck;
use crate::program::{Program, last_token};
use crate::runner::{self, Run};

pub const PROPERTIES: [&str; 8] = [
    "no-panic",
    "budget",
    "determinism",
    "pdf-structure",
    "save-restore",
    "gsave-grestore",
    "translate",
    "reorder-defs",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub property: String,
    pub detail: String,
}

fn failure(property: &str, detail: impl Into<String>) -> Failure {
    Failure {
        property: property.to_string(),
        detail: detail.into(),
    }
}

/// Everything the properties need about one program: its text and the
/// base run they compare against.
pub struct Checker {
    program: Program,
    budget: u64,
    base: Run,
}

/// Evaluates every property; the failures found.
pub fn check(program: &Program, budget: u64) -> Vec<Failure> {
    Checker::new(program, budget).all()
}

impl Checker {
    pub fn new(program: &Program, budget: u64) -> Checker {
        let base = runner::execute(&program.render(), budget);
        Checker {
            program: program.clone(),
            budget,
            base,
        }
    }

    pub fn base(&self) -> &Run {
        &self.base
    }

    pub fn all(&self) -> Vec<Failure> {
        PROPERTIES
            .iter()
            .filter_map(|name| self.property(name).expect("a listed property"))
            .collect()
    }

    /// One property by name; `Err` for an unknown name.
    pub fn property(&self, name: &str) -> Result<Option<Failure>, String> {
        let result = match name {
            "no-panic" => self.no_panic(),
            "budget" => self.budget(),
            "determinism" => self.determinism(),
            "pdf-structure" => self.pdf_structure(),
            "save-restore" => self.save_restore(),
            "gsave-grestore" => self.gsave_grestore(),
            "translate" => self.translate(),
            "reorder-defs" => self.reorder_defs(),
            other => return Err(format!("unknown property `{other}`")),
        };
        Ok(result)
    }

    fn run(&self, statements: &[String]) -> Run {
        runner::execute(
            &self.program.with_statements(statements.to_vec()).render(),
            self.budget,
        )
    }

    fn no_panic(&self) -> Option<Failure> {
        self.base
            .panic
            .as_ref()
            .map(|message| failure("no-panic", format!("panicked: {message}")))
    }

    fn budget(&self) -> Option<Failure> {
        self.base.budget_exceeded.then(|| {
            failure(
                "budget",
                format!(
                    "{} executed objects exceeded the budget of {}; outcome {}",
                    self.base.steps,
                    self.budget,
                    self.base.outcome()
                ),
            )
        })
    }

    fn determinism(&self) -> Option<Failure> {
        let again = self.run(&self.program.statements);
        compare("determinism", &self.base, &again, true)
    }

    fn pdf_structure(&self) -> Option<Failure> {
        if self.base.panic.is_some() {
            return None;
        }
        let pdf = match runner::distill(&self.program.render(), self.budget) {
            Ok(pdf) => pdf,
            Err(e) => return Some(failure("pdf-structure", format!("distillation: {e}"))),
        };
        pdfcheck::check(&pdf)
            .err()
            .map(|e| failure("pdf-structure", e))
    }

    /// The statements before the body (the page setup), the body, and
    /// the statements after it (`showpage` and the final print).
    fn body_span(&self) -> (usize, usize) {
        let statements = &self.program.statements;
        let mut start = 0;
        if statements
            .first()
            .is_some_and(|s| last_token(s) == "setpagedevice")
        {
            start = 1;
        }
        let mut end = statements.len();
        while end > start
            && matches!(
                last_token(&statements[end - 1]),
                "pstack" | "showpage" | "stack"
            )
        {
            end -= 1;
        }
        (start, end)
    }

    fn save_restore(&self) -> Option<Failure> {
        let (start, end) = self.body_span();
        if start >= end {
            return None;
        }
        let statements = &self.program.statements;
        let wrapped = |before_restore: &[&str]| {
            let mut variant: Vec<String> = statements[..start].to_vec();
            variant.push("/psgen_sr save def".to_string());
            variant.extend_from_slice(&statements[start..end]);
            variant.extend(before_restore.iter().map(|s| s.to_string()));
            variant.push("psgen_sr restore".to_string());
            variant.extend_from_slice(&statements[end..]);
            variant
        };
        let run = self.run(&wrapped(&[]));
        if run.error.as_deref() != Some("invalidrestore")
            || run.command.as_deref() != Some("restore")
            || run.panic.is_some()
        {
            return compare("save-restore", &self.base, &run, true);
        }
        // Objects created in the body were left on the stack, which is
        // what `restore` must reject. With the stack cleared first,
        // `restore` must succeed, the output before the tail must be
        // the original's, and the tail prints nothing of the stack.
        if !self.base.output.starts_with(&run.output) {
            return Some(failure(
                "save-restore",
                "restore raised invalidrestore but the output before it is not a prefix of the original's",
            ));
        }
        let cleared = self.run(&wrapped(&["clear"]));
        let headless = self.run(&statements[..end]);
        let expected = Run {
            output: headless.output,
            stderr: headless.stderr,
            ..self.base.clone()
        };
        compare("save-restore", &expected, &cleared, true).map(|f| Failure {
            detail: format!("after clearing the stack before restore: {}", f.detail),
            ..f
        })
    }

    fn gsave_grestore(&self) -> Option<Failure> {
        let statements = &self.program.statements;
        let (start, end) = self.body_span();
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for i in start..end {
            match last_token(&statements[i]) {
                "rectfill" | "rectstroke" => candidates.push((i, i)),
                "fill" | "eofill" | "stroke" => {
                    let mut from = i;
                    while from > start && is_path_statement(&statements[from - 1]) {
                        from -= 1;
                    }
                    let boundary = from == start
                        || matches!(
                            last_token(&statements[from - 1]),
                            "newpath" | "fill" | "eofill" | "stroke" | "showpage"
                        );
                    if boundary {
                        candidates.push((from, i));
                    }
                }
                _ => {}
            }
        }
        let chosen: Vec<(usize, usize)> = match candidates.len() {
            0 => return None,
            1 => candidates,
            _ => vec![candidates[0], candidates[candidates.len() - 1]],
        };
        for (from, to) in chosen {
            let mut variant: Vec<String> = statements[..from].to_vec();
            variant.push("gsave".to_string());
            variant.extend_from_slice(&statements[from..=to]);
            variant.push("grestore".to_string());
            variant.extend_from_slice(&statements[to + 1..]);
            let run = self.run(&variant);
            if let Some(f) = compare("gsave-grestore", &self.base, &run, true) {
                return Some(Failure {
                    detail: format!(
                        "wrapping statements {}..={} (`{}`): {}",
                        from + 1,
                        to + 1,
                        statements[to],
                        f.detail
                    ),
                    ..f
                });
            }
        }
        None
    }

    fn translate(&self) -> Option<Failure> {
        if self.base.pages == 0 || self.base.panic.is_some() {
            return None;
        }
        let (tx, ty) = (7.0f32, -3.0f32);
        let (start, _) = self.body_span();
        let statements = &self.program.statements;
        let mut variant: Vec<String> = statements[..start].to_vec();
        variant.push("7 -3 translate".to_string());
        variant.extend_from_slice(&statements[start..]);
        let run = self.run(&variant);
        if let Some(f) = compare_within("translate", &self.base, &run, false, Numbers::Close) {
            return Some(f);
        }
        compare_shifted(&self.base.ir, &run.ir, tx, ty).map(|detail| failure("translate", detail))
    }

    fn reorder_defs(&self) -> Option<Failure> {
        let statements = &self.program.statements;
        let defs: Vec<Option<String>> = statements.iter().map(|s| closed_def_name(s)).collect();
        let pairs: Vec<usize> = (0..statements.len().saturating_sub(1))
            .filter(|&i| match (&defs[i], &defs[i + 1]) {
                (Some(a), Some(b)) => a != b,
                _ => false,
            })
            .collect();
        let chosen: Vec<usize> = match pairs.len() {
            0 => return None,
            1 => pairs,
            _ => vec![pairs[0], pairs[pairs.len() - 1]],
        };
        for i in chosen {
            let mut variant = statements.clone();
            variant.swap(i, i + 1);
            let run = self.run(&variant);
            if let Some(f) = compare("reorder-defs", &self.base, &run, true) {
                return Some(Failure {
                    detail: format!(
                        "swapping statements {} and {} (`{}` / `{}`): {}",
                        i + 1,
                        i + 2,
                        statements[i],
                        statements[i + 1],
                        f.detail
                    ),
                    ..f
                });
            }
        }
        None
    }
}

/// How numbers on the output are compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Numbers {
    Exact,
    /// Within the rounding of a reading back through the CTM:
    /// `0.01 + 10⁻⁵·|v|` (the grammar keeps the CTM's scale within
    /// [1/8, 8] and readings away from arithmetic that amplifies error).
    Close,
}

/// The first difference between two runs, as a failure of `property`.
fn compare(property: &str, base: &Run, variant: &Run, ir: bool) -> Option<Failure> {
    compare_within(property, base, variant, ir, Numbers::Exact)
}

fn compare_within(
    property: &str,
    base: &Run,
    variant: &Run,
    ir: bool,
    numbers: Numbers,
) -> Option<Failure> {
    if let Some(message) = &variant.panic {
        return Some(failure(property, format!("variant panicked: {message}")));
    }
    if base.error != variant.error {
        return Some(failure(
            property,
            format!("outcome {} became {}", base.outcome(), variant.outcome()),
        ));
    }
    let output_differs = match numbers {
        Numbers::Exact => base.output != variant.output,
        Numbers::Close => !output_close(&base.output, &variant.output),
    };
    if output_differs {
        let shown = match numbers {
            Numbers::Exact => first_line_difference(&base.output, &variant.output),
            Numbers::Close => first_line_beyond_tolerance(&base.output, &variant.output),
        };
        return Some(failure(property, format!("output differs: {shown}")));
    }
    if base.stderr != variant.stderr {
        return Some(failure(
            property,
            format!(
                "error report differs: {}",
                first_line_difference(&base.stderr, &variant.stderr)
            ),
        ));
    }
    if ir && base.ir != variant.ir {
        return Some(failure(
            property,
            format!(
                "ir differs: {}",
                first_line_difference(&base.ir, &variant.ir)
            ),
        ));
    }
    None
}

fn first_line_difference(a: &str, b: &str) -> String {
    let (left, right): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    for i in 0..left.len().max(right.len()) {
        match (left.get(i), right.get(i)) {
            (Some(x), Some(y)) if x == y => continue,
            (x, y) => {
                return format!(
                    "line {}: `{}` vs `{}`",
                    i + 1,
                    x.copied().unwrap_or("<end>"),
                    y.copied().unwrap_or("<end>")
                );
            }
        }
    }
    "trailing newline".to_string()
}

/// Splits text into runs of number characters and the rest.
fn number_runs(text: &str) -> Vec<&str> {
    let numeric = |c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E');
    let mut runs = Vec::new();
    let mut start = 0;
    let mut in_number = false;
    for (i, c) in text.char_indices() {
        if numeric(c) != in_number {
            if i > start {
                runs.push(&text[start..i]);
            }
            start = i;
            in_number = numeric(c);
        }
    }
    if start < text.len() {
        runs.push(&text[start..]);
    }
    runs
}

/// Whether two texts agree up to rounding of the numbers they carry:
/// the text around the numbers must match exactly, and a run that
/// parses as a number in both must be within `0.01 + 10⁻⁵·|v|`.
fn output_close(a: &str, b: &str) -> bool {
    let (left, right) = (number_runs(a), number_runs(b));
    left.len() == right.len()
        && left.iter().zip(&right).all(|(x, y)| {
            x == y
                || match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(p), Ok(q)) => (p - q).abs() <= 1e-2 + 1e-5 * p.abs().max(q.abs()),
                    _ => false,
                }
        })
}

/// The first line pair that [`output_close`] rejects.
fn first_line_beyond_tolerance(a: &str, b: &str) -> String {
    let (left, right): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    for i in 0..left.len().max(right.len()) {
        match (left.get(i), right.get(i)) {
            (Some(x), Some(y)) if output_close(x, y) => continue,
            (x, y) => {
                return format!(
                    "line {}: `{}` vs `{}` (beyond rounding)",
                    i + 1,
                    x.copied().unwrap_or("<end>"),
                    y.copied().unwrap_or("<end>")
                );
            }
        }
    }
    "trailing newline".to_string()
}

fn is_path_statement(statement: &str) -> bool {
    matches!(
        last_token(statement),
        "moveto"
            | "lineto"
            | "rmoveto"
            | "rlineto"
            | "curveto"
            | "rcurveto"
            | "arc"
            | "arcn"
            | "arcto"
            | "closepath"
            | "charpath"
    )
}

// --- translation ---------------------------------------------------------------

/// A dump line with its coordinate fields identified; a stroke carries
/// its matrix (the identity when the dump omitted it).
#[derive(Clone, Debug, PartialEq)]
struct Line {
    tokens: Vec<String>,
    /// Indices of the tokens that move with the translation, in
    /// (x, y) pairs.
    shifted: Vec<usize>,
}

fn normalise(ir: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut pending_ctm: Option<Vec<String>> = None;
    for raw in ir.lines() {
        let tokens: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
        let Some(op) = tokens.first().map(String::as_str) else {
            lines.push(Line {
                tokens,
                shifted: Vec::new(),
            });
            continue;
        };
        let shifted: Vec<usize> = match op {
            "stroke-ctm" => {
                pending_ctm = Some(tokens[1..].to_vec());
                continue;
            }
            "S" => {
                let ctm = pending_ctm
                    .take()
                    .unwrap_or_else(|| ["1", "0", "0", "1", "0", "0"].map(String::from).to_vec());
                let mut tokens = tokens;
                tokens.extend(ctm);
                lines.push(Line {
                    tokens,
                    shifted: vec![5, 6],
                });
                continue;
            }
            "m" | "l" => vec![1, 2],
            "c" => vec![1, 2, 3, 4, 5, 6],
            "text" => vec![6, 7],
            "Do" => vec![7, 8],
            _ => Vec::new(),
        };
        lines.push(Line { tokens, shifted });
    }
    lines
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.002 + 1e-5 * a.abs().max(b.abs())
}

/// Whether `variant` is `base` with every coordinate moved by (tx, ty);
/// the first differing line otherwise.
fn compare_shifted(base: &str, variant: &str, tx: f32, ty: f32) -> Option<String> {
    let (left, right) = (normalise(base), normalise(variant));
    for i in 0..left.len().max(right.len()) {
        let (Some(a), Some(b)) = (left.get(i), right.get(i)) else {
            return Some(format!(
                "line {}: `{}` vs `{}`",
                i + 1,
                left.get(i)
                    .map_or("<end>".to_string(), |l| l.tokens.join(" ")),
                right
                    .get(i)
                    .map_or("<end>".to_string(), |l| l.tokens.join(" "))
            ));
        };
        let same = a.tokens.len() == b.tokens.len()
            && a.shifted == b.shifted
            && a.tokens.iter().enumerate().all(|(k, token)| {
                let other = &b.tokens[k];
                match a.shifted.iter().position(|&s| s == k) {
                    None => token == other,
                    Some(pair) => {
                        let delta = if pair % 2 == 0 { tx } else { ty };
                        match (token.parse::<f32>(), other.parse::<f32>()) {
                            (Ok(x), Ok(y)) => close(x + delta, y),
                            _ => false,
                        }
                    }
                }
            });
        if !same {
            return Some(format!(
                "line {}: `{}` vs `{}` (expected a shift of {tx} {ty})",
                i + 1,
                a.tokens.join(" "),
                b.tokens.join(" ")
            ));
        }
    }
    None
}

// --- closed definitions ----------------------------------------------------------

/// The statement's tokens, with strings and procedure bodies as single
/// tokens.
pub fn tokenize(statement: &str) -> Vec<String> {
    let bytes = statement.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        match b {
            b'(' => {
                let mut depth = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
            }
            b'{' => {
                let mut depth = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                i += 1;
            }
            b'[' | b']' | b'}' => i += 1,
            b'<' | b'>' => {
                i += 1;
                if bytes.get(i) == Some(&b) {
                    i += 1;
                }
            }
            _ => {
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(
                        bytes[i],
                        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'<' | b'>'
                    )
                {
                    i += 1;
                }
                if bytes[i.min(bytes.len())..].is_empty() && i == start {
                    i += 1;
                }
            }
        }
        let end = i.min(bytes.len());
        if end > start {
            tokens.push(statement[start..end].to_string());
        }
    }
    tokens
}

/// Pure operators a closed expression may use, with what they pop and
/// push.
fn pure_arity(token: &str) -> Option<(usize, usize)> {
    Some(match token {
        "add" | "sub" | "mul" | "div" | "idiv" | "mod" | "atan" | "exp" | "bitshift" | "and"
        | "or" | "xor" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" => (2, 1),
        "neg" | "abs" | "sqrt" | "sin" | "cos" | "ln" | "log" | "round" | "truncate" | "floor"
        | "ceiling" | "cvi" | "cvr" | "cvn" | "cvx" | "cvlit" | "not" | "string" | "array"
        | "dict" | "length" | "type" | "xcheck" => (1, 1),
        "cvs" => (2, 1),
        "cvrs" => (3, 1),
        _ => return None,
    })
}

/// The name a `/name <closed expression> def` statement defines.
pub fn closed_def_name(statement: &str) -> Option<String> {
    let tokens = tokenize(statement);
    let name = tokens.first()?.strip_prefix('/')?;
    if name.is_empty() || tokens.last().map(String::as_str) != Some("def") {
        return None;
    }
    // Simulate the expression's stack: `true` entries are marks.
    let mut stack: Vec<bool> = Vec::new();
    for token in &tokens[1..tokens.len() - 1] {
        match token.as_str() {
            "[" | "<<" => stack.push(true),
            "]" | ">>" => {
                let mark = stack.iter().rposition(|&m| m)?;
                stack.truncate(mark);
                stack.push(false);
            }
            "true" | "false" | "null" | "mark" => stack.push(token == "mark"),
            t if t.starts_with('/') || t.starts_with('(') || t.starts_with('{') => {
                stack.push(false)
            }
            t if t.parse::<f64>().is_ok() => stack.push(false),
            t => {
                let (pops, pushes) = pure_arity(t)?;
                if stack.len() < pops || stack[stack.len() - pops..].iter().any(|&m| m) {
                    return None;
                }
                stack.truncate(stack.len() - pops);
                stack.extend(std::iter::repeat_n(false, pushes));
            }
        }
    }
    (stack.len() == 1 && !stack[0]).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(statements: &[&str]) -> Program {
        Program {
            header: vec!["%!PS".to_string()],
            statements: statements.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(failures: &[Failure]) -> Vec<&str> {
        failures.iter().map(|f| f.property.as_str()).collect()
    }

    const BUDGET: u64 = 100_000;

    #[test]
    fn a_plain_program_passes_everything() {
        let p = program(&[
            "/v0 3 def",
            "/v1 (abc) length def",
            "v0 v1 add =",
            "0 0 moveto 50 50 lineto",
            "stroke",
            "10 10 20 20 rectfill",
            "showpage",
            "pstack",
        ]);
        assert_eq!(check(&p, BUDGET), Vec::new());
    }

    #[test]
    fn budget_and_panic_properties() {
        let p = program(&["{ } loop"]);
        assert_eq!(names(&check(&p, 5_000)), ["budget"]);
        let base = Run {
            panic: Some("boom".to_string()),
            ..Default::default()
        };
        let checker = Checker {
            program: program(&["1"]),
            budget: BUDGET,
            base,
        };
        assert_eq!(checker.no_panic().unwrap().detail, "panicked: boom");
        assert_eq!(checker.pdf_structure(), None);
        assert_eq!(checker.translate(), None);
    }

    #[test]
    fn determinism_compares_two_runs() {
        // No operator of the interpreter is non-deterministic, so the
        // comparison is exercised on runs that differ by construction.
        let base = runner::execute("1 =", BUDGET);
        let other = runner::execute("2 =", BUDGET);
        let f = compare("determinism", &base, &other, true).unwrap();
        assert_eq!(f.property, "determinism");
        assert!(f.detail.contains("output differs"));
        assert!(f.detail.contains("`1` vs `2`"));
        assert_eq!(compare("determinism", &base, &base, true), None);
        let errored = runner::execute("1 0 div", BUDGET);
        assert!(
            compare("x", &base, &errored, true)
                .unwrap()
                .detail
                .contains("outcome ok became undefinedresult")
        );
        let drawn = runner::execute("0 0 10 10 rectfill showpage", BUDGET);
        let drawn_more = runner::execute("0 0 10 20 rectfill showpage", BUDGET);
        assert!(
            compare("x", &drawn, &drawn_more, true)
                .unwrap()
                .detail
                .contains("ir differs")
        );
        assert_eq!(compare("x", &drawn, &drawn_more, false), None);
        let checker = Checker::new(&program(&["1 2 add ="]), BUDGET);
        assert_eq!(checker.determinism(), None);
        assert_eq!(checker.base().output, "3\n");
    }

    #[test]
    fn pdf_structure_runs_the_check() {
        let checker = Checker::new(&program(&["0 0 10 10 rectfill showpage"]), BUDGET);
        assert_eq!(checker.pdf_structure(), None);
        assert!(pdfcheck::check(b"%PDF-1.7\n").is_err());
    }

    #[test]
    fn save_restore_accepts_transparent_bodies_and_invalidrestore() {
        let clean = program(&["/v0 5 def", "v0 2 mul =", "(done) =", "pstack"]);
        assert_eq!(Checker::new(&clean, BUDGET).save_restore(), None);
        // Objects created in the body and left on the stack make
        // `restore` fail: the expected shape, checked again with the
        // stack cleared.
        let leaves = program(&["[1 2 3]", "(abc) dup =", "pstack"]);
        assert_eq!(Checker::new(&leaves, BUDGET).save_restore(), None);
        // A body that observes the save level violates the relation.
        let observes = program(&["vmstatus pop pop =", "pstack"]);
        let f = Checker::new(&observes, BUDGET).save_restore().unwrap();
        assert_eq!(f.property, "save-restore");
        assert!(f.detail.contains("output differs"), "{}", f.detail);
        // So does one that observes it after leaving objects behind.
        let observes_late = program(&["[1 2 3]", "vmstatus pop pop =", "pstack"]);
        let f = Checker::new(&observes_late, BUDGET).save_restore().unwrap();
        assert!(f.detail.contains("not a prefix"), "{}", f.detail);
        assert_eq!(
            Checker::new(&program(&["pstack"]), BUDGET).save_restore(),
            None
        );
        let graphics = program(&[
            "<< /PageSize [300 300] >> setpagedevice",
            "0 0 moveto 20 20 lineto clip",
            "2 setlinewidth stroke",
            "(s)",
            "showpage",
            "pstack",
        ]);
        assert_eq!(Checker::new(&graphics, BUDGET).save_restore(), None);
    }

    #[test]
    fn gsave_grestore_wraps_paint_segments() {
        let p = program(&[
            "<< /PageSize [300 300] >> setpagedevice",
            "0.5 setgray",
            "10 10 moveto",
            "100 10 lineto",
            "stroke",
            "20 20 moveto 30 30 lineto",
            "fill",
            "5 5 50 50 rectfill",
            "showpage",
            "pstack",
        ]);
        let checker = Checker::new(&p, BUDGET);
        assert_eq!(checker.gsave_grestore(), None);
        // Statements: the first paint's segment starts after the colour
        // (not a boundary) so it is not a candidate; the fill's segment
        // follows a stroke and is.
        assert_eq!(
            Checker::new(&program(&["1 ="]), BUDGET).gsave_grestore(),
            None
        );
        // The comparison behind the relation reports IR differences.
        let base = runner::execute("0 0 10 10 rectfill showpage", BUDGET);
        let other = runner::execute("0 0 10 10 rectfill 0 0 5 5 rectfill showpage", BUDGET);
        assert!(compare("gsave-grestore", &base, &other, true).is_some());
    }

    #[test]
    fn translation_shifts_every_coordinate() {
        let p = program(&[
            "<< /PageSize [300 300] >> setpagedevice",
            "10 10 100 50 rectfill",
            "2 setlinewidth 0 0 moveto 50 50 lineto 60 60 70 70 80 80 curveto stroke",
            "2 2 scale 5 5 moveto 20 20 lineto stroke",
            "10 10 moveto 30 30 lineto clip",
            "/Helvetica findfont 12 scalefont setfont 40 40 moveto (Hi) show",
            "showpage",
            "pstack",
        ]);
        assert_eq!(Checker::new(&p, BUDGET).translate(), None);
        // A program that resets the matrix ignores the translation.
        let resets = program(&[
            "<< /PageSize [300 300] >> setpagedevice",
            "matrix defaultmatrix setmatrix",
            "10 10 100 50 rectfill",
            "showpage",
        ]);
        let f = Checker::new(&resets, BUDGET).translate().unwrap();
        assert_eq!(f.property, "translate");
        assert!(f.detail.contains("expected a shift"), "{}", f.detail);
        // A program that prints the device coordinates of a point
        // changes its output.
        let prints = program(&["10 10 transform pop =", "0 0 5 5 rectfill showpage"]);
        let f = Checker::new(&prints, BUDGET).translate().unwrap();
        assert!(f.detail.contains("output differs"));
        assert_eq!(Checker::new(&program(&["1 ="]), BUDGET).translate(), None);
    }

    #[test]
    fn translated_output_tolerates_rounding_of_readings() {
        assert!(output_close("441.00003\n", "441.0\n"));
        assert!(output_close("[383.0 7.0]\n", "[383.0 7.000001]\n"));
        assert!(output_close("-0.17195506 x", "-0.17195503 x"));
        assert!(output_close("1e-7", "0.0"));
        assert!(output_close("-3.3962867", "-3.396141"));
        assert!(!output_close("441.1", "441.0"));
        assert!(!output_close("10 x", "11 x"));
        assert!(!output_close("0.1", "0.2"));
        assert_eq!(
            first_line_beyond_tolerance("441.00003\n5\n", "441.0\n6\n"),
            "line 2: `5` vs `6` (beyond rounding)"
        );
        assert!(!output_close("(abc)", "(abd)"));
        assert!(!output_close("1 2", "1 2 3"));
        assert!(!output_close("1e", "1f"));
        assert_eq!(number_runs("m -1.5e3 (x)"), ["m ", "-1.5e3", " (x)"]);
        // A program reading its current point back through a rotated,
        // scaled space prints a value the translation may move by one
        // unit in the last place; the relation accepts that, and only
        // that.
        let p = program(&[
            "<< /PageSize [300 300] >> setpagedevice",
            "-45 rotate 0.80 0.89 scale",
            "12.3 45.6 moveto currentpoint = =",
            "0 0 10 10 rectfill",
            "showpage",
            "pstack",
        ]);
        assert_eq!(Checker::new(&p, BUDGET).translate(), None);
        let base = runner::execute("441.00003 =", BUDGET);
        let other = runner::execute("441.0 =", BUDGET);
        assert!(compare("translate", &base, &other, false).is_some());
        assert_eq!(
            compare_within("translate", &base, &other, false, Numbers::Close),
            None
        );
        let far = runner::execute("442 =", BUDGET);
        let f = compare_within("translate", &base, &far, false, Numbers::Close).unwrap();
        assert!(f.detail.contains("beyond rounding"), "{}", f.detail);
    }

    #[test]
    fn shifted_comparison_tolerates_formatting() {
        assert_eq!(
            compare_shifted(
                "m 0.123456 1\nl 2 3\nS\n",
                "m 7.12346 -2\nl 9 0\nstroke-ctm 1 0 0 1 7 -3\nS\n",
                7.0,
                -3.0
            ),
            None
        );
        assert!(compare_shifted("m 0 0\nS\n", "m 7 -3\nS\n", 7.0, -3.0).is_some());
        assert!(compare_shifted("m 0 0\n", "m 0 0\nl 1 1\n", 7.0, -3.0).is_some());
        assert!(compare_shifted("w 2\n", "w 3\n", 7.0, -3.0).is_some());
        assert_eq!(
            compare_shifted(
                "text 0 0.012 0 0 0.012 100 700 (Hi) 722 0\n",
                "text 0 0.012 0 0 0.012 107 697 (Hi) 722 0\n",
                7.0,
                -3.0
            ),
            None
        );
    }

    #[test]
    fn reordering_independent_definitions() {
        let p = program(&["/a 1 def", "/b 2 def", "a b add =", "pstack"]);
        assert_eq!(Checker::new(&p, BUDGET).reorder_defs(), None);
        // Enumeration order follows insertion order, so a program that
        // lists its dictionary observes the swap.
        let lists = program(&["/a 1 def", "/b 2 def", "currentdict { pop == } forall"]);
        let f = Checker::new(&lists, BUDGET).reorder_defs().unwrap();
        assert_eq!(f.property, "reorder-defs");
        assert!(f.detail.contains("swapping statements 1 and 2"));
        assert_eq!(
            Checker::new(&program(&["/a 1 def"]), BUDGET).reorder_defs(),
            None
        );
    }

    #[test]
    fn closed_definitions_are_recognised() {
        assert_eq!(closed_def_name("/a 1 def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a 1 2 add def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a [ 1 2 ] def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a << /k 1 >> def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a { 1 add } def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a (x y) def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a 3 string def").as_deref(), Some("a"));
        assert_eq!(closed_def_name("/a exch def"), None);
        assert_eq!(closed_def_name("/a b def"), None);
        assert_eq!(closed_def_name("/a 1 2 def"), None);
        assert_eq!(closed_def_name("/a add def"), None);
        assert_eq!(closed_def_name("/a ] def"), None);
        assert_eq!(closed_def_name("/a 1 def ="), None);
        assert_eq!(closed_def_name("1 2 add"), None);
        assert_eq!(closed_def_name("/ 1 def"), None);
        assert_eq!(
            tokenize("/a (x y) { 1 { 2 } } [ 1 ] << >> def"),
            [
                "/a",
                "(x y)",
                "{ 1 { 2 } }",
                "[",
                "1",
                "]",
                "<<",
                ">>",
                "def"
            ]
        );
    }

    #[test]
    fn unknown_properties_are_rejected() {
        let checker = Checker::new(&program(&["1"]), BUDGET);
        assert!(checker.property("nope").is_err());
        assert_eq!(checker.property("budget"), Ok(None));
        assert!(checker.all().is_empty());
    }
}
