// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Differential test harness.
//!
//! `difftest run [--update-ir] [--update-pdf] [path…]` executes every
//! `.ps` file under the given paths (default: `corpus/unit`) in a fresh
//! interpreter with capture streams and the graphics backend installed,
//! and checks the file's own declarations:
//!
//! - `% expect-output: text` — one line of standard output; several such
//!   lines concatenate in order, each ending in a newline. `\n`, `\t`,
//!   `\r`, and `\\` escapes are honoured. No such line means no output.
//! - `% expect-error: name` — the job must end in that uncaught error;
//!   absent, the job must end normally.
//! - `% backend: none` — run without a graphics backend, for files that
//!   check the scripting-only configuration.
//!
//! Declarations are read from the leading comment block only.
//!
//! A file under `corpus/unit` may have sidecar goldens at the same
//! relative path under `corpus/golden/ir` (extension `.ir`) and
//! `corpus/golden/pdf` (extension `.pdf`). The concatenated IR dump of
//! the pages the file produced must match the first exactly, `#` comment
//! lines in the golden aside; the distilled document of those pages —
//! written uncompressed, with provenance comments — must match the second
//! byte for byte, a mismatch reported as a diff of the text lines.
//! `--update-ir` and `--update-pdf` (re)write the goldens of every file
//! that produced pages; goldens are generated, never hand-edited. When
//! `EFTERSCRIPT_PDF_CHECK` names a program, every distilled document is
//! written under `target/difftest` and the program run on it; a non-zero
//! exit fails the file. Semantic comparison against another
//! interpreter's output and the private tier under `EFTERSCRIPT_HELLBOX`
//! are later additions to this tool.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;

use ps_graphics::{Graphics, Page, PageSink, dump};
use ps_vm::{Config, Interp, Io, Outcome, SliceSource};
use remelt::{Options, PdfSink};

/// What a corpus file declares about its own run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub output: String,
    pub error: Option<String>,
    /// Whether the graphics backend is installed for the run.
    pub graphics: bool,
}

impl Default for Expectation {
    fn default() -> Self {
        Expectation {
            output: String::new(),
            error: None,
            graphics: true,
        }
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses the declarations in the leading comment block of a program.
pub fn expectation(program: &str) -> Expectation {
    let mut lines = Vec::new();
    let mut error = None;
    let mut graphics = true;
    for line in program.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('%') {
            break;
        }
        if let Some(rest) = line.strip_prefix("% expect-output:") {
            lines.push(unescape(rest.strip_prefix(' ').unwrap_or(rest)));
        } else if let Some(rest) = line.strip_prefix("% expect-error:") {
            error = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("% backend:") {
            graphics = rest.trim() != "none";
        }
    }
    let mut output = lines.join("\n");
    if !lines.is_empty() {
        output.push('\n');
    }
    Expectation {
        output,
        error,
        graphics,
    }
}

/// What actually happened when a program ran.
#[derive(Clone, Debug, PartialEq)]
pub struct Actual {
    pub output: String,
    pub error: Option<String>,
    pub stderr: String,
    pub pages: Vec<Page>,
    /// The distilled document of `pages` (see [`distil`]), empty when
    /// there were none; the message when it could not be written.
    pub pdf: Result<Vec<u8>, String>,
}

impl Actual {
    /// The concatenated dump of the delivered pages; empty when there
    /// were none.
    pub fn ir(&self) -> String {
        dump::pages(&self.pages)
    }
}

/// The comment lines a PDF golden carries after the header.
const PDF_GOLDEN_COMMENTS: [&str; 3] = [
    "SPDX-FileCopyrightText: 2026 EfterScript contributors",
    "SPDX-License-Identifier: MIT",
    "GENERATED-BY: difftest --update-pdf",
];

/// Writes `pages` as a golden document: uncompressed, so the bytes read
/// as text, with the provenance comments between the header and the
/// first object. Nothing for no pages.
pub fn distil(pages: &[Page]) -> Result<Vec<u8>, remelt::Error> {
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    let mut sink = PdfSink::new(Vec::new(), Options { compress: false })?;
    for comment in PDF_GOLDEN_COMMENTS {
        sink.comment(comment)?;
    }
    for page in pages {
        sink.page(page.clone());
    }
    sink.finish()
}

/// Runs `program` with the graphics backend installed.
pub fn execute(program: &[u8]) -> Actual {
    execute_with(program, true)
}

pub fn execute_with(program: &[u8], graphics: bool) -> Actual {
    let (io, out, err) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let pages = Rc::new(RefCell::new(Vec::new()));
    if graphics {
        interp.set_graphics_backend(Box::new(Graphics::new(pages.clone())));
    }
    let outcome = interp.run(&mut SliceSource::new(program));
    let pages = pages.take();
    let pdf = distil(&pages).map_err(|e| e.to_string());
    Actual {
        output: out.text(),
        error: match outcome {
            Outcome::Error(summary) => Some(summary.name),
            Outcome::Ok => None,
            Outcome::Suspended => Some("<suspended>".to_string()),
        },
        stderr: err.text(),
        pages,
        pdf,
    }
}

fn diff(lines: &mut Vec<String>, expected: &str, actual: &str) {
    let want: Vec<&str> = expected.lines().collect();
    let got: Vec<&str> = actual.lines().collect();
    for k in 0..want.len().max(got.len()) {
        match (want.get(k), got.get(k)) {
            (Some(w), Some(g)) if w == g => lines.push(format!("    {w}")),
            (w, g) => {
                if let Some(w) = w {
                    lines.push(format!("  - {w}"));
                }
                if let Some(g) = g {
                    lines.push(format!("  + {g}"));
                }
            }
        }
    }
    if !expected.is_empty() && !expected.ends_with('\n') != !actual.ends_with('\n') {
        lines.push("  (trailing newline differs)".to_string());
    }
}

/// The sidecar goldens a run is checked against.
#[derive(Clone, Copy, Debug, Default)]
pub struct Goldens<'a> {
    pub ir: Option<&'a str>,
    pub pdf: Option<&'a [u8]>,
}

/// The lines of a failure report, empty when the run met its declarations
/// and its sidecar goldens.
pub fn report(expected: &Expectation, actual: &Actual, goldens: Goldens<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    if expected.error != actual.error {
        let show = |e: &Option<String>| e.clone().unwrap_or_else(|| "ok".to_string());
        lines.push(format!(
            "  outcome: expected {}, got {}",
            show(&expected.error),
            show(&actual.error)
        ));
        if !actual.stderr.is_empty() {
            for line in actual.stderr.lines() {
                lines.push(format!("  stderr: {line}"));
            }
        }
    }
    if expected.output != actual.output {
        lines.push("  output differs (- expected, + actual):".to_string());
        diff(&mut lines, &expected.output, &actual.output);
    }
    if let Some(golden) = goldens.ir {
        let want = golden_body(golden);
        let got = actual.ir();
        if want != got {
            lines.push("  ir differs (- golden, + actual):".to_string());
            diff(&mut lines, &want, &got);
        }
    }
    match (&actual.pdf, goldens.pdf) {
        (Err(e), _) => lines.push(format!("  pdf: {e}")),
        (Ok(got), Some(want)) if got != want => {
            lines.push("  pdf differs (- golden, + actual):".to_string());
            diff(
                &mut lines,
                &String::from_utf8_lossy(want),
                &String::from_utf8_lossy(got),
            );
        }
        _ => {}
    }
    lines
}

/// A golden's content with its `#` comment lines removed.
pub fn golden_body(text: &str) -> String {
    let mut body = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if !line.starts_with('#') {
            body.push_str(line);
        }
    }
    body
}

/// A golden file for `ir`: the version line first, then the provenance
/// comments, then the rest of the dump.
pub fn golden_text(ir: &str) -> String {
    let (version, rest) = ir.split_once('\n').unwrap_or((ir, ""));
    format!(
        "{version}\n\
         # SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
         # SPDX-License-Identifier: MIT\n\
         # GENERATED-BY: difftest --update-ir\n\
         {rest}"
    )
}

/// A kind of sidecar golden: its directory under `corpus/golden` and its
/// extension share the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Golden {
    Ir,
    Pdf,
}

impl Golden {
    fn name(self) -> &'static str {
        match self {
            Golden::Ir => "ir",
            Golden::Pdf => "pdf",
        }
    }
}

/// Where a sidecar golden of a corpus file lives: the same path under
/// `corpus/golden/<kind>` with the kind's extension. Files outside
/// `corpus/unit` have none.
pub fn golden_path(root: &Path, file: &Path, kind: Golden) -> Option<PathBuf> {
    let unit = root.join("corpus").join("unit");
    let relative = file.strip_prefix(&unit).ok()?;
    Some(
        root.join("corpus")
            .join("golden")
            .join(kind.name())
            .join(relative)
            .with_extension(kind.name()),
    )
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for entry in entries {
            collect(&entry, out);
        }
    } else if path.extension().is_some_and(|e| e == "ps") {
        out.push(path.to_path_buf());
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("difftest lives two levels below the workspace root")
        .to_path_buf()
}

fn read_golden(root: &Path, path: &Path, kind: Golden) -> Result<Option<Vec<u8>>, String> {
    match golden_path(root, path, kind) {
        Some(golden) if golden.is_file() => std::fs::read(&golden)
            .map(Some)
            .map_err(|e| format!("golden {} unreadable: {e}", golden.display())),
        _ => Ok(None),
    }
}

/// Runs the program named by `EFTERSCRIPT_PDF_CHECK`, if any, on `pdf`
/// written under `target/difftest`; the failure lines when it objects.
/// The value is a program name, not a shell command line.
fn external_check(root: &Path, path: &Path, pdf: &[u8]) -> Result<Vec<String>, String> {
    let Some(checker) = std::env::var_os("EFTERSCRIPT_PDF_CHECK") else {
        return Ok(Vec::new());
    };
    let relative = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from(path.file_name().unwrap_or_default()));
    let temp = root
        .join("target")
        .join("difftest")
        .join(relative)
        .with_extension("pdf");
    if let Some(dir) = temp.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&temp, pdf).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    let output = Command::new(&checker)
        .arg(&temp)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", checker.to_string_lossy()))?;
    if output.status.success() {
        return Ok(Vec::new());
    }
    let mut lines = vec![format!(
        "  {} rejected {} ({})",
        checker.to_string_lossy(),
        temp.display(),
        output.status
    )];
    for stream in [&output.stdout, &output.stderr] {
        for line in String::from_utf8_lossy(stream).lines() {
            lines.push(format!("    {line}"));
        }
    }
    Ok(lines)
}

/// Runs one corpus file against its declarations, its goldens, and the
/// external checker; the failure lines, or none.
fn check(root: &Path, path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
    let expected = expectation(&String::from_utf8_lossy(&bytes));
    let actual = execute_with(&bytes, expected.graphics);
    let ir = read_golden(root, path, Golden::Ir)?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let pdf = read_golden(root, path, Golden::Pdf)?;
    let mut lines = report(
        &expected,
        &actual,
        Goldens {
            ir: ir.as_deref(),
            pdf: pdf.as_deref(),
        },
    );
    if let Ok(pdf) = &actual.pdf
        && !pdf.is_empty()
    {
        lines.extend(external_check(root, path, pdf)?);
    }
    Ok(lines)
}

/// (Re)writes the goldens of `kinds` for a corpus file that produced
/// pages; the paths written.
fn update_goldens(root: &Path, path: &Path, kinds: &[Golden]) -> Result<Vec<PathBuf>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
    let expected = expectation(&String::from_utf8_lossy(&bytes));
    let actual = execute_with(&bytes, expected.graphics);
    if actual.pages.is_empty() {
        return Ok(Vec::new());
    }
    let mut written = Vec::new();
    for &kind in kinds {
        let Some(golden) = golden_path(root, path, kind) else {
            continue;
        };
        let data = match kind {
            Golden::Ir => golden_text(&actual.ir()).into_bytes(),
            Golden::Pdf => actual.pdf.clone().map_err(|e| format!("pdf: {e}"))?,
        };
        if let Some(dir) = golden.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::write(&golden, data)
            .map_err(|e| format!("cannot write {}: {e}", golden.display()))?;
        written.push(golden);
    }
    Ok(written)
}

fn run(args: &[String]) -> ExitCode {
    let root = workspace_root();
    let mut kinds = Vec::new();
    if args.iter().any(|a| a == "--update-ir") {
        kinds.push(Golden::Ir);
    }
    if args.iter().any(|a| a == "--update-pdf") {
        kinds.push(Golden::Pdf);
    }
    let paths: Vec<&String> = args
        .iter()
        .filter(|a| *a != "--update-ir" && *a != "--update-pdf")
        .collect();
    // Goldens are located by the file's path under the workspace, so a
    // path given relative to the working directory is made absolute first.
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![root.join("corpus").join("unit")]
    } else {
        paths
            .iter()
            .map(|p| std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p)))
            .collect()
    };
    let mut files = Vec::new();
    for path in &roots {
        collect(path, &mut files);
    }
    let mut failures = 0;
    for path in &files {
        let shown = path.strip_prefix(&root).unwrap_or(path).display();
        if !kinds.is_empty() {
            match update_goldens(&root, path, &kinds) {
                Ok(written) => {
                    for golden in written {
                        let golden = golden.strip_prefix(&root).unwrap_or(&golden).display();
                        println!("wrote {golden}");
                    }
                }
                Err(e) => {
                    failures += 1;
                    println!("FAIL  {shown}\n  {e}");
                    continue;
                }
            }
        }
        match check(&root, path) {
            Ok(lines) if lines.is_empty() => println!("ok    {shown}"),
            Ok(lines) => {
                failures += 1;
                println!("FAIL  {shown}");
                for line in lines {
                    println!("{line}");
                }
            }
            Err(e) => {
                failures += 1;
                println!("FAIL  {shown}\n  {e}");
            }
        }
    }
    println!();
    println!(
        "{} files, {} passed, {} failed",
        files.len(),
        files.len() - failures,
        failures
    );
    if failures > 0 || files.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: difftest run [--update-ir] [--update-pdf] [path…]    (default: corpus/unit)");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        _ => usage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_declarations_are_parsed() {
        let e = expectation(
            "%!PS\n% SPDX-License-Identifier: MIT\n% expect-output: a\n\n% expect-output: b\\nc\n% expect-error: typecheck\n1 =\n% expect-output: ignored\n",
        );
        assert_eq!(e.output, "a\nb\nc\n");
        assert_eq!(e.error.as_deref(), Some("typecheck"));
        assert!(e.graphics);
        assert_eq!(expectation("1 ="), Expectation::default());
        assert_eq!(expectation("% expect-output:\n").output, "\n");
        assert!(!expectation("%!PS\n% backend: none\n").graphics);
    }

    #[test]
    fn escapes_are_honoured() {
        assert_eq!(unescape("a\\tb\\\\c\\q\\"), "a\tb\\c\\q\\");
    }

    #[test]
    fn execution_captures_output_and_outcome() {
        let ok = execute(b"1 2 add =");
        assert_eq!(ok.output, "3\n");
        assert_eq!(ok.error, None);
        assert_eq!(ok.pdf, Ok(Vec::new()));
        let failed = execute(b"(x) = 1 0 div");
        assert_eq!(failed.output, "x\n");
        assert_eq!(failed.error.as_deref(), Some("undefinedresult"));
        assert!(failed.stderr.contains("OffendingCommand: div"));
    }

    #[test]
    fn execution_collects_pages_only_with_a_backend() {
        let drawn = execute(b"0 0 moveto 1 1 lineto stroke showpage showpage");
        assert_eq!(drawn.error, None);
        assert_eq!(drawn.pages.len(), 2);
        assert!(
            drawn
                .ir()
                .starts_with("ir/1\npage 612 792\nresources:\nops:\nm 0 0\n")
        );
        assert!(drawn.ir().contains("\n\nir/1\n"));
        let bare = execute_with(b"0 0 moveto", false);
        assert_eq!(bare.error.as_deref(), Some("undefined"));
        assert!(bare.pages.is_empty());
        assert_eq!(bare.ir(), "");
    }

    #[test]
    fn distilled_documents_are_uncompressed_and_self_describing() {
        let drawn = execute(b"2 setlinewidth 10 10 moveto 100 10 lineto stroke showpage");
        let pdf = drawn.pdf.clone().unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.7\n"));
        assert!(text.contains(
            "\n% SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
             % SPDX-License-Identifier: MIT\n\
             % GENERATED-BY: difftest --update-pdf\n"
        ));
        assert!(text.contains("stream\n2 w\n10 10 m\n100 10 l\nS\n\nendstream"));
        assert!(!text.contains("FlateDecode"));
        assert_eq!(distil(&drawn.pages).unwrap(), pdf);
    }

    #[test]
    fn reports_name_every_difference() {
        let expected = expectation("% expect-output: 3\n");
        assert!(report(&expected, &execute(b"1 2 add ="), Goldens::default()).is_empty());
        let lines = report(&expected, &execute(b"4 = 1 0 div"), Goldens::default());
        assert!(lines[0].contains("expected ok, got undefinedresult"));
        assert!(lines.iter().any(|l| l == "  - 3"));
        assert!(lines.iter().any(|l| l == "  + 4"));

        let actual = execute(b"0 0 10 10 rectfill showpage");
        let golden = golden_text(&actual.ir());
        assert!(golden.starts_with("ir/1\n# SPDX-FileCopyrightText"));
        assert!(golden.contains("\n# GENERATED-BY: difftest --update-ir\npage 612 792\n"));
        assert_eq!(golden_body(&golden), actual.ir());
        let goldens = Goldens {
            ir: Some(&golden),
            pdf: None,
        };
        assert!(report(&Expectation::default(), &actual, goldens).is_empty());
        let stale = golden.replace("l 10 0", "l 20 0");
        let goldens = Goldens {
            ir: Some(&stale),
            pdf: None,
        };
        let lines = report(&Expectation::default(), &actual, goldens);
        assert!(lines[0].contains("ir differs"));
        assert!(lines.iter().any(|l| l == "  - l 20 0"));
        assert!(lines.iter().any(|l| l == "  + l 10 0"));

        let pdf = actual.pdf.clone().unwrap();
        let goldens = Goldens {
            ir: None,
            pdf: Some(&pdf),
        };
        assert!(report(&Expectation::default(), &actual, goldens).is_empty());
        let stale = String::from_utf8_lossy(&pdf)
            .replace("10 0 l", "20 0 l")
            .into_bytes();
        let goldens = Goldens {
            ir: None,
            pdf: Some(&stale),
        };
        let lines = report(&Expectation::default(), &actual, goldens);
        assert!(lines[0].contains("pdf differs"));
        assert!(lines.iter().any(|l| l == "  - 20 0 l"));
        assert!(lines.iter().any(|l| l == "  + 10 0 l"));
    }

    #[test]
    fn goldens_sit_beside_the_unit_tree() {
        let root = Path::new("/w");
        let file = Path::new("/w/corpus/unit/graphics/stroked-line.ps");
        assert_eq!(
            golden_path(root, file, Golden::Ir),
            Some(PathBuf::from(
                "/w/corpus/golden/ir/graphics/stroked-line.ir"
            ))
        );
        assert_eq!(
            golden_path(root, file, Golden::Pdf),
            Some(PathBuf::from(
                "/w/corpus/golden/pdf/graphics/stroked-line.pdf"
            ))
        );
        assert_eq!(
            golden_path(root, Path::new("/elsewhere/a.ps"), Golden::Ir),
            None
        );
    }

    #[test]
    fn the_public_corpus_passes() {
        let root = workspace_root();
        let mut files = Vec::new();
        collect(&root.join("corpus").join("unit"), &mut files);
        assert!(!files.is_empty());
        for path in files {
            let lines = check(&root, &path).unwrap();
            assert!(
                lines.is_empty(),
                "{}:\n{}",
                path.display(),
                lines.join("\n")
            );
        }
    }
}
