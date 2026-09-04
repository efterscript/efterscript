// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `difftest oracle [--profile <name>] [--dpi <n>] [--json <path>]
//! [path…]`: the differential comparison against the reference
//! converter a profile describes (see [`crate::profile`]).
//!
//! Per corpus file: EfterScript's PDF and the converter's are rendered
//! by the profile's rasteriser and compared page by page (count, media
//! box, pixels, and text when the profile extracts it); separately, the
//! program's standard output is compared with the reference
//! interpreter's after normalisation and reported as `output: same`,
//! `differs`, or `unavailable`. A file carrying `% divergence: <slug>`
//! is reported as `expected-divergence` instead of `fail` and as
//! `divergence-closed` when nothing differs any more; every slug must
//! name a requirement in the expected-divergences registry, or the run
//! aborts before comparing anything. The exit status is non-zero only
//! for `fail`. Everything the commands produce stays under
//! `target/oracle/<path>/`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ps_graphics::{Page, PageSink};
use remelt::{Options, PdfSink};

use crate::pnm;
use crate::profile::{self, Profile};
use crate::{Actual, collect, execute_with_stdin, expectation, workspace_root};

/// The names `% divergence:` may declare, read from the
/// expected-divergences specification: the living spec when it exists,
/// else the delta of every open change that adds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub sources: Vec<PathBuf>,
    slugs: BTreeSet<String>,
}

impl Registry {
    /// The requirement names in a specification's text.
    pub fn slugs_in(text: &str) -> BTreeSet<String> {
        text.lines()
            .filter_map(|line| line.strip_prefix("### Requirement:"))
            .map(|name| name.trim().to_string())
            .collect()
    }

    /// Where the registry is read from, in lookup order.
    pub fn paths(root: &Path) -> Vec<PathBuf> {
        let relative = Path::new("specs")
            .join("expected-divergences")
            .join("spec.md");
        let living = root.join("openspec").join(&relative);
        if living.is_file() {
            return vec![living];
        }
        let Ok(changes) = std::fs::read_dir(root.join("openspec").join("changes")) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = changes
            .flatten()
            .map(|entry| entry.path())
            .filter(|dir| dir.file_name().is_some_and(|n| n != "archive"))
            .map(|dir| dir.join(&relative))
            .filter(|spec| spec.is_file())
            .collect();
        paths.sort();
        paths
    }

    pub fn load(root: &Path) -> Result<Self, String> {
        let sources = Self::paths(root);
        if sources.is_empty() {
            return Err(
                "no expected-divergences specification under openspec/specs or openspec/changes/*"
                    .to_string(),
            );
        }
        let mut slugs = BTreeSet::new();
        for path in &sources {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("registry {}: {e}", path.display()))?;
            slugs.extend(Self::slugs_in(&text));
        }
        Ok(Registry { sources, slugs })
    }

    pub fn contains(&self, slug: &str) -> bool {
        self.slugs.contains(slug)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    ExpectedDivergence,
    DivergenceClosed,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::ExpectedDivergence => "expected-divergence",
            Verdict::DivergenceClosed => "divergence-closed",
        }
    }
}

/// The standard-output channel's own result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Same,
    Differs,
    /// The reference interpreter's output could not be obtained.
    Unavailable,
}

impl Output {
    pub fn name(self) -> &'static str {
        match self {
            Output::Same => "same",
            Output::Differs => "differs",
            Output::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileReport {
    /// The file's path relative to the workspace, or `external/<name>`.
    pub shown: String,
    pub verdict: Verdict,
    pub output: Output,
    pub divergence: Option<String>,
    pub pages_ours: usize,
    /// Pages the rasteriser produced from the converter's document;
    /// none when the converter produced no document.
    pub pages_theirs: Option<usize>,
    /// The differing fraction per compared page.
    pub fractions: Vec<f64>,
    /// Why the documents did not match, or what stopped the comparison.
    pub reasons: Vec<String>,
    /// What the comparison decided that a reader should know without
    /// it counting against the file.
    pub notes: Vec<String>,
}

pub struct Settings<'a> {
    pub root: &'a Path,
    pub out_root: PathBuf,
    pub profile: &'a Profile,
}

/// What `check_file` decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Checked {
    Report(FileReport),
    /// Not run: the build lacks the named feature.
    Skipped(String),
}

enum Exit {
    Status(ExitStatus),
    TimedOut,
}

/// Runs `command` through the shell in `dir`, its streams captured to
/// files, killing it once `deadline` passes.
fn run_command(
    command: &str,
    dir: &Path,
    stdout: &Path,
    stderr: &Path,
    deadline: Instant,
) -> Result<Exit, String> {
    let out =
        File::create(stdout).map_err(|e| format!("cannot create {}: {e}", stdout.display()))?;
    let err =
        File::create(stderr).map_err(|e| format!("cannot create {}: {e}", stderr.display()))?;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("cannot run `{command}`: {e}"))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Exit::Status(status));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Exit::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Our document for `pages`, uncompressed; a zero-page document for none.
fn document(pages: &[Page]) -> Result<Vec<u8>, String> {
    let mut sink =
        PdfSink::new(Vec::new(), Options { compress: false }).map_err(|e| e.to_string())?;
    for page in pages {
        sink.page(page.clone());
    }
    sink.finish().map_err(|e| e.to_string())
}

/// The page files `<side>-1.pnm`, `<side>-2.pnm`, … up to the first
/// missing one.
fn rendered_pages(dir: &Path, side: &str) -> Vec<PathBuf> {
    let mut pages = Vec::new();
    loop {
        let page = dir.join(format!("{side}-{}.pnm", pages.len() + 1));
        if !page.is_file() {
            return pages;
        }
        pages.push(page);
    }
}

fn render(
    settings: &Settings<'_>,
    dir: &Path,
    side: &str,
    pdf: &Path,
    deadline: Instant,
) -> Result<Vec<PathBuf>, String> {
    let pattern = dir.join(format!("{side}-%d.pnm"));
    let command = profile::fill(
        &settings.profile.render,
        pdf,
        Some(&pattern),
        settings.profile.dpi,
    );
    let stderr = dir.join(format!("render-{side}.stderr"));
    match run_command(
        &command,
        dir,
        &dir.join(format!("render-{side}.stdout")),
        &stderr,
        deadline,
    )? {
        Exit::TimedOut => Err(format!("rendering {side} timed out")),
        Exit::Status(status) if !status.success() => Err(format!(
            "rendering {side} failed ({status}); see {}",
            stderr.display()
        )),
        Exit::Status(_) => Ok(rendered_pages(dir, side)),
    }
}

fn extract_text(
    settings: &Settings<'_>,
    dir: &Path,
    side: &str,
    pdf: &Path,
    deadline: Instant,
) -> Result<String, String> {
    let template = settings.profile.text.as_deref().unwrap_or_default();
    let command = profile::fill(template, pdf, None, settings.profile.dpi);
    let text = dir.join(format!("{side}.txt"));
    match run_command(
        &command,
        dir,
        &text,
        &dir.join(format!("text-{side}.stderr")),
        deadline,
    )? {
        Exit::TimedOut => Err(format!("text extraction of {side} timed out")),
        Exit::Status(status) if !status.success() => {
            Err(format!("text extraction of {side} failed ({status})"))
        }
        Exit::Status(_) => {
            Ok(String::from_utf8_lossy(&std::fs::read(&text).unwrap_or_default()).into_owned())
        }
    }
}

/// Whitespace runs collapsed to one space, ends trimmed.
pub fn normalise_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A number token in its shortest form — trailing fraction zeros and a
/// bare point dropped, negative zero made zero — or `None` when `token`
/// is not a number.
pub fn canonical_number(token: &str) -> Option<String> {
    let (sign, rest) = match token.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", token.strip_prefix('+').unwrap_or(token)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    if !exponent.is_empty() {
        let digits = exponent[1..].trim_start_matches(['+', '-']);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let (int, frac) = match mantissa.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || frac.is_some_and(|f| !digits(f)) {
        return None;
    }
    if int.is_empty() && frac.is_none_or(str::is_empty) {
        return None;
    }
    let mut out = String::from(if int.is_empty() { "0" } else { int });
    if let Some(frac) = frac {
        let frac = frac.trim_end_matches('0');
        if !frac.is_empty() {
            out.push('.');
            out.push_str(frac);
        }
    }
    let zero = out.bytes().all(|b| b == b'0');
    Some(format!("{}{out}{exponent}", if zero { "" } else { sign }))
}

/// Program output in comparison form: line endings unified, trailing
/// whitespace and trailing blank lines dropped, numbers canonical.
pub fn normalise_output(text: &str) -> String {
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .lines()
        .map(|line| {
            line.trim_end()
                .split(' ')
                .map(|token| canonical_number(token).unwrap_or_else(|| token.to_string()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

/// Whether every pixel of `image` is white.
fn blank(image: &pnm::Image) -> bool {
    image.data.iter().all(|&byte| byte == 255)
}

/// The document comparison: mismatches go into `report.reasons`; an
/// error is anything that stopped the comparison itself. `declared` is
/// the error the file expects to end with, when it declares one.
fn compare_documents(
    settings: &Settings<'_>,
    path: &Path,
    dir: &Path,
    actual: &Actual,
    declared: Option<&str>,
    report: &mut FileReport,
    deadline: Instant,
) -> Result<(), String> {
    let profile = settings.profile;
    let ours_pdf = dir.join("ours.pdf");
    std::fs::write(&ours_pdf, document(&actual.pages)?)
        .map_err(|e| format!("cannot write {}: {e}", ours_pdf.display()))?;
    let theirs_pdf = dir.join("theirs.pdf");
    let command = profile::fill(&profile.ps2pdf, path, Some(&theirs_pdf), profile.dpi);
    match run_command(
        &command,
        dir,
        &dir.join("ps2pdf.stdout"),
        &dir.join("ps2pdf.stderr"),
        deadline,
    )? {
        Exit::TimedOut => return Err("reference converter timed out".to_string()),
        Exit::Status(status) if !status.success() => match declared {
            // The file ends in an error on both sides; the document
            // written up to it is still compared.
            Some(error) => report.notes.push(format!(
                "reference converter ended abnormally ({status}), as the file declares {error}"
            )),
            None => {
                report
                    .reasons
                    .push(format!("reference converter failed ({status})"));
                return Ok(());
            }
        },
        Exit::Status(_) => {
            if let Some(error) = declared {
                report.reasons.push(format!(
                    "reference converter ended normally where the file declares {error}"
                ));
            }
        }
    }
    if !theirs_pdf.is_file() {
        report
            .reasons
            .push("reference converter wrote no document".to_string());
        return Ok(());
    }
    let ours = if actual.pages.is_empty() {
        Vec::new()
    } else {
        render(settings, dir, "ours", &ours_pdf, deadline)?
    };
    if ours.len() != actual.pages.len() {
        return Err(format!(
            "the rasteriser produced {} pages from ours, which has {}",
            ours.len(),
            actual.pages.len()
        ));
    }
    let mut theirs = render(settings, dir, "theirs", &theirs_pdf, deadline)?;
    report.pages_theirs = Some(theirs.len());
    // A converter may close a job that showed nothing with one empty
    // page; such a page is not a page the program showed.
    let theirs_text = if profile.text.is_some() {
        Some(extract_text(
            settings,
            dir,
            "theirs",
            &theirs_pdf,
            deadline,
        )?)
    } else {
        None
    };
    if ours.is_empty() && theirs.len() == 1 {
        let bytes =
            std::fs::read(&theirs[0]).map_err(|e| format!("{}: {e}", theirs[0].display()))?;
        let image = pnm::parse(&bytes).map_err(|e| format!("{}: {e}", theirs[0].display()))?;
        let wordless = theirs_text
            .as_deref()
            .is_none_or(|text| normalise_text(text).is_empty());
        if blank(&image) && wordless {
            report
                .notes
                .push("theirs: one blank page where ours shows none, taken as no page".to_string());
            theirs.clear();
        }
    }
    if ours.len() != theirs.len() {
        report.reasons.push(format!(
            "pages: ours {}, theirs {}",
            ours.len(),
            theirs.len()
        ));
    }
    let scale = 72.0 / f64::from(profile.dpi);
    // The converter's media box is known only through whole pixels.
    let tolerance = 0.5 + scale;
    for (index, (ours, theirs)) in ours.iter().zip(&theirs).enumerate() {
        let page = index + 1;
        let read = |file: &Path| -> Result<pnm::Image, String> {
            let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
            pnm::parse(&bytes).map_err(|e| format!("{}: {e}", file.display()))
        };
        let (a, b) = (read(ours)?, read(theirs)?);
        let bounds = actual.pages[index].media_box;
        let (width, height) = (
            f64::from(bounds.urx - bounds.llx),
            f64::from(bounds.ury - bounds.lly),
        );
        let (theirs_width, theirs_height) = (b.width as f64 * scale, b.height as f64 * scale);
        if (theirs_width - width).abs() > tolerance || (theirs_height - height).abs() > tolerance {
            report.reasons.push(format!(
                "page {page}: media box ours {width}x{height}, theirs {theirs_width}x{theirs_height} (from {}x{} pixels at {} dpi)",
                b.width, b.height, profile.dpi
            ));
        }
        if (a.width, a.height) != (b.width, b.height) {
            report.reasons.push(format!(
                "page {page}: rendering ours {}x{}, theirs {}x{} pixels",
                a.width, a.height, b.width, b.height
            ));
            report.fractions.push(1.0);
            continue;
        }
        let diff = pnm::compare(&a, &b, profile.threshold)?;
        report.fractions.push(diff.fraction());
        if diff.fraction() > profile.limit {
            report.reasons.push(format!(
                "page {page}: {:.3}% of pixels differ (limit {:.3}%, largest channel difference {})",
                diff.fraction() * 100.0,
                profile.limit * 100.0,
                diff.max
            ));
        }
    }
    if let Some(theirs_text) = theirs_text {
        // A document without pages has no text to extract.
        let ours_text = if ours.is_empty() {
            String::new()
        } else {
            extract_text(settings, dir, "ours", &ours_pdf, deadline)?
        };
        if normalise_text(&ours_text) != normalise_text(&theirs_text) {
            report
                .reasons
                .push("text differs (ours.txt against theirs.txt)".to_string());
        }
    }
    Ok(())
}

/// Whether the reference interpreter's standard output equals ours after
/// normalisation; an error when it could not be obtained.
fn compare_output(
    settings: &Settings<'_>,
    path: &Path,
    dir: &Path,
    actual: &Actual,
    deadline: Instant,
) -> Result<bool, String> {
    let ours = dir.join("ours.stdout");
    std::fs::write(&ours, &actual.output)
        .map_err(|e| format!("cannot write {}: {e}", ours.display()))?;
    let theirs = dir.join("theirs.stdout");
    let command = profile::fill(&settings.profile.run, path, None, settings.profile.dpi);
    if let Exit::TimedOut = run_command(&command, dir, &theirs, &dir.join("run.stderr"), deadline)?
    {
        return Err("reference interpreter timed out".to_string());
    }
    let theirs = std::fs::read(&theirs).map_err(|e| format!("{}: {e}", theirs.display()))?;
    Ok(normalise_output(&actual.output) == normalise_output(&String::from_utf8_lossy(&theirs)))
}

/// A file's path relative to the workspace; `external/<name>` for one
/// outside it.
fn shown(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => format!(
            "external/{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
    }
}

/// Compares one corpus file, its outputs under `<out_root>/<shown>/`.
pub fn check_file(settings: &Settings<'_>, path: &Path) -> Checked {
    let shown = shown(settings.root, path);
    let mut report = FileReport {
        shown: shown.clone(),
        verdict: Verdict::Fail,
        output: Output::Unavailable,
        divergence: None,
        pages_ours: 0,
        pages_theirs: None,
        fractions: Vec::new(),
        reasons: Vec::new(),
        notes: Vec::new(),
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            report.reasons.push(format!("unreadable: {e}"));
            return Checked::Report(report);
        }
    };
    let expected = expectation(&String::from_utf8_lossy(&bytes));
    if let Some(feature) = expected.unmet_requirement() {
        return Checked::Skipped(feature.to_string());
    }
    report.divergence = expected.divergence.clone();
    let actual = execute_with_stdin(&bytes, expected.graphics);
    report.pages_ours = actual.pages.len();
    let dir = settings.out_root.join(&shown);
    let prepared = match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
    .and_then(|()| std::fs::create_dir_all(&dir));
    if let Err(e) = prepared {
        report
            .reasons
            .push(format!("cannot prepare {}: {e}", dir.display()));
        return Checked::Report(report);
    }
    let deadline = Instant::now() + Duration::from_millis(settings.profile.timeout_ms);
    let mut error = false;
    if let Err(e) = compare_documents(
        settings,
        path,
        &dir,
        &actual,
        expected.error.as_deref(),
        &mut report,
        deadline,
    ) {
        error = true;
        report.reasons.push(e);
    }
    match compare_output(settings, path, &dir, &actual, deadline) {
        Ok(true) => report.output = Output::Same,
        Ok(false) => report.output = Output::Differs,
        Err(e) => {
            error = true;
            report.reasons.push(e);
        }
    }
    let matched = report.reasons.is_empty();
    report.verdict = match (error, report.divergence.is_some()) {
        (true, _) => Verdict::Fail,
        (false, false) if matched => Verdict::Pass,
        (false, false) => Verdict::Fail,
        (false, true) if matched && report.output == Output::Same => Verdict::DivergenceClosed,
        (false, true) => Verdict::ExpectedDivergence,
    };
    Checked::Report(report)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub pass: usize,
    pub fail: usize,
    pub expected_divergence: usize,
    pub divergence_closed: usize,
    pub skipped: usize,
    pub output_same: usize,
    pub output_differs: usize,
    pub output_unavailable: usize,
}

impl Summary {
    pub fn of(reports: &[FileReport], skipped: usize) -> Self {
        let mut summary = Summary {
            files: reports.len() + skipped,
            skipped,
            ..Default::default()
        };
        for report in reports {
            match report.verdict {
                Verdict::Pass => summary.pass += 1,
                Verdict::Fail => summary.fail += 1,
                Verdict::ExpectedDivergence => summary.expected_divergence += 1,
                Verdict::DivergenceClosed => summary.divergence_closed += 1,
            }
            match report.output {
                Output::Same => summary.output_same += 1,
                Output::Differs => summary.output_differs += 1,
                Output::Unavailable => summary.output_unavailable += 1,
            }
        }
        summary
    }

    pub fn line(&self) -> String {
        format!(
            "{} files, {} pass, {} fail, {} expected-divergence, {} divergence-closed, {} skipped; output: {} same, {} differs, {} unavailable",
            self.files,
            self.pass,
            self.fail,
            self.expected_divergence,
            self.divergence_closed,
            self.skipped,
            self.output_same,
            self.output_differs,
            self.output_unavailable
        )
    }
}

/// Checks every file, printing a line per file as it goes.
pub fn run_files(settings: &Settings<'_>, files: &[PathBuf]) -> (Vec<FileReport>, usize) {
    let mut reports = Vec::new();
    let mut skipped = 0;
    for path in files {
        match check_file(settings, path) {
            Checked::Report(report) => {
                let divergence = report
                    .divergence
                    .as_ref()
                    .map(|slug| format!("  ({slug})"))
                    .unwrap_or_default();
                println!(
                    "{:<19} {}  output: {}{divergence}",
                    report.verdict.name(),
                    report.shown,
                    report.output.name()
                );
                for reason in &report.reasons {
                    println!("  {reason}");
                }
                for note in &report.notes {
                    println!("  note: {note}");
                }
                reports.push(report);
            }
            Checked::Skipped(feature) => {
                skipped += 1;
                println!(
                    "skip                {} (requires the {feature} feature, absent from this build)",
                    shown(settings.root, path)
                );
            }
        }
    }
    (reports, skipped)
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The run as a JSON document.
pub fn json_report(profile: &Profile, reports: &[FileReport], summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{{\n  \"profile\": {{\"name\": {}, \"version\": {}, \"dpi\": {}, \"threshold\": {}, \"limit\": {}, \"timeout_ms\": {}}},\n  \"files\": [",
        json_string(&profile.name),
        json_string(&profile.version),
        profile.dpi,
        profile.threshold,
        profile.limit,
        profile.timeout_ms
    ));
    for (index, report) in reports.iter().enumerate() {
        let divergence = report
            .divergence
            .as_deref()
            .map_or("null".to_string(), json_string);
        let theirs = report
            .pages_theirs
            .map_or("null".to_string(), |n| n.to_string());
        let fractions: Vec<String> = report.fractions.iter().map(|f| format!("{f}")).collect();
        let reasons: Vec<String> = report.reasons.iter().map(|r| json_string(r)).collect();
        let notes: Vec<String> = report.notes.iter().map(|n| json_string(n)).collect();
        out.push_str(&format!(
            "{}\n    {{\"path\": {}, \"verdict\": {}, \"output\": {}, \"divergence\": {divergence}, \"pages\": {{\"ours\": {}, \"theirs\": {theirs}}}, \"fractions\": [{}], \"reasons\": [{}], \"notes\": [{}]}}",
            if index == 0 { "" } else { "," },
            json_string(&report.shown),
            json_string(report.verdict.name()),
            json_string(report.output.name()),
            report.pages_ours,
            fractions.join(", "),
            reasons.join(", "),
            notes.join(", ")
        ));
    }
    out.push_str(&format!(
        "\n  ],\n  \"summary\": {{\"files\": {}, \"pass\": {}, \"fail\": {}, \"expected-divergence\": {}, \"divergence-closed\": {}, \"skipped\": {}, \"output-same\": {}, \"output-differs\": {}, \"output-unavailable\": {}}}\n}}\n",
        summary.files,
        summary.pass,
        summary.fail,
        summary.expected_divergence,
        summary.divergence_closed,
        summary.skipped,
        summary.output_same,
        summary.output_differs,
        summary.output_unavailable
    ));
    out
}

fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// `given` as an absolute path, accepted only under the build directory.
pub fn json_path(root: &Path, given: &Path) -> Result<PathBuf, String> {
    let target = normal(&root.join("target"));
    let absolute =
        std::path::absolute(given).map_err(|e| format!("--json {}: {e}", given.display()))?;
    let path = normal(&absolute);
    if !path.starts_with(&target) {
        return Err(format!(
            "--json {} must name a path under {}",
            given.display(),
            target.display()
        ));
    }
    Ok(path)
}

/// The environment the profile is located from.
pub struct Env {
    pub profile_path: Option<OsString>,
    pub vault: Option<OsString>,
}

impl Env {
    pub fn from_process() -> Self {
        Env {
            profile_path: std::env::var_os("EFTERSCRIPT_ORACLE_PROFILE"),
            vault: std::env::var_os("EFTERSCRIPT_HELLBOX").filter(|v| !v.is_empty()),
        }
    }
}

#[derive(Debug)]
struct Args {
    profile: Option<String>,
    dpi: Option<u32>,
    json: Option<PathBuf>,
    paths: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        profile: None,
        dpi: None,
        json: None,
        paths: Vec::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--profile" => parsed.profile = Some(value("--profile")?),
            "--dpi" => {
                let raw = value("--dpi")?;
                parsed.dpi = Some(
                    raw.parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("--dpi {raw}: a positive integer is needed"))?,
                );
            }
            "--json" => parsed.json = Some(PathBuf::from(value("--json")?)),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => parsed.paths.push(path.to_string()),
        }
    }
    Ok(parsed)
}

fn failure(message: &str) -> ExitCode {
    eprintln!("difftest oracle: {message}");
    ExitCode::from(2)
}

pub fn run(args: &[String], env: &Env) -> ExitCode {
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(e) => return failure(&e),
    };
    let root = workspace_root();
    let located = profile::locate(
        args.profile.as_deref(),
        env.profile_path.as_deref(),
        env.vault.as_deref(),
    );
    let path = match located {
        Ok(Some(path)) => path,
        Ok(None) => {
            println!("oracle tier skipped: no profile");
            return ExitCode::SUCCESS;
        }
        Err(e) => return failure(&e),
    };
    let mut profile = match profile::load(&path, &root) {
        Ok(profile) => profile,
        Err(e) => return failure(&e),
    };
    if let Some(dpi) = args.dpi {
        profile.dpi = dpi;
    }
    let json = match args.json.as_deref().map(|p| json_path(&root, p)) {
        Some(Ok(path)) => Some(path),
        Some(Err(e)) => return failure(&e),
        None => None,
    };
    let roots: Vec<PathBuf> = if args.paths.is_empty() {
        vec![root.join("corpus").join("unit")]
    } else {
        args.paths
            .iter()
            .map(|p| std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p)))
            .collect()
    };
    let mut files = Vec::new();
    for path in &roots {
        collect(path, &mut files);
    }
    if files.is_empty() {
        return failure("no .ps files found");
    }
    let mut declared = Vec::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        if let Some(slug) = expectation(&String::from_utf8_lossy(&bytes)).divergence {
            declared.push((shown(&root, file), slug));
        }
    }
    if !declared.is_empty() {
        let registry = match Registry::load(&root) {
            Ok(registry) => registry,
            Err(e) => return failure(&e),
        };
        for (file, slug) in &declared {
            if !registry.contains(slug) {
                let sources: Vec<String> =
                    registry.sources.iter().map(|p| shown(&root, p)).collect();
                return failure(&format!(
                    "{file}: unknown divergence `{slug}` (registry: {})",
                    sources.join(", ")
                ));
            }
        }
    }
    println!(
        "profile {} {}: {} dpi, threshold {}, limit {}, timeout {} ms",
        profile.name,
        profile.version,
        profile.dpi,
        profile.threshold,
        profile.limit,
        profile.timeout_ms
    );
    let settings = Settings {
        root: &root,
        out_root: root.join("target").join("oracle"),
        profile: &profile,
    };
    let (reports, skipped) = run_files(&settings, &files);
    let summary = Summary::of(&reports, skipped);
    println!();
    println!("{}", summary.line());
    if let Some(path) = json {
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, json_report(&profile, &reports, &summary)));
        match written {
            Ok(()) => println!("wrote {}", shown(&root, &path)),
            Err(e) => return failure(&format!("cannot write {}: {e}", path.display())),
        }
    }
    if summary.fail > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A converter that copies our own document and appends the
    /// `mark` lines of the control file, refuses when the control says
    /// `reject`, stalls when it says `hang`, and ends abnormally after
    /// writing when it says `fail-after`.
    const PS2PDF: &str = "#!/bin/sh
export LC_ALL=C
ctl=\"$(dirname \"$0\")/control\"
if grep -q '^reject' \"$ctl\"; then echo refused >&2; exit 3; fi
if grep -q '^hang' \"$ctl\"; then sleep 5; fi
cp \"$(dirname \"$2\")/ours.pdf\" \"$2\" || exit 1
sed -n 's/^mark //p' \"$ctl\" >> \"$2\"
if grep -q '^fail-after' \"$ctl\"; then echo failed >&2; exit 3; fi
";

    /// A rasteriser: one white P5 page per `/MediaBox` line, sized from
    /// the box at the given dpi, plus what `%fake-` marks in the input
    /// ask for (extra pages, a fixed size, some pixels of another value).
    const RENDER: &str = "#!/bin/sh
export LC_ALL=C
in=$1; out=$2; dpi=$3
count=$(grep -a -c '/MediaBox' \"$in\")
extra=$(sed -n 's/^%fake-extra-pages //p' \"$in\" | tail -n 1)
fill=$(sed -n 's/^%fake-fill //p' \"$in\" | tail -n 1)
size=$(sed -n 's/^%fake-size //p' \"$in\" | tail -n 1)
n=1
total=$((count + ${extra:-0}))
while [ \"$n\" -le \"$total\" ]; do
  box=$(grep -a -o '/MediaBox \\[[^]]*\\]' \"$in\" | sed -n \"${n}p\")
  w=8; h=8
  if [ -n \"$box\" ]; then
    set -- $box
    w=$(( ( ($5 - $3) * dpi + 36 ) / 72 ))
    h=$(( ( ($6 - $4) * dpi + 36 ) / 72 ))
  fi
  if [ -n \"$size\" ]; then set -- $size; w=$1; h=$2; fi
  v=255; c=0
  if [ -n \"$fill\" ]; then set -- $fill; v=$1; c=$2; fi
  file=$(printf \"$out\" \"$n\")
  printf 'P5\\n%d %d\\n255\\n' \"$w\" \"$h\" > \"$file\"
  if [ \"$c\" -gt 0 ]; then head -c \"$c\" /dev/zero | tr '\\0' \"\\\\$(printf '%03o' \"$v\")\" >> \"$file\"; fi
  head -c $(( w * h - c )) /dev/zero | tr '\\0' '\\377' >> \"$file\"
  n=$((n + 1))
done
";

    /// An interpreter whose output is the file's own declarations plus
    /// the control's `output` lines.
    const RUN: &str = "#!/bin/sh
export LC_ALL=C
sed -n 's/^% expect-output: //p' \"$1\"
sed -n 's/^output //p' \"$(dirname \"$0\")/control\"
exit 0
";

    /// A text extractor: a constant for a document with a page, plus
    /// whatever `%fake-text` says.
    const TEXT: &str = "#!/bin/sh
export LC_ALL=C
if grep -a -q '/MediaBox' \"$1\"; then printf 'hello '; fi
printf '%s\\n' \"$(sed -n 's/^%fake-text //p' \"$1\")\"
";

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        dir: PathBuf,
        profile: Profile,
    }

    impl Fixture {
        fn new(limit: f64, timeout_ms: u64, text: bool) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "efterscript-oracle-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let script = |name: &str, body: &str| {
                std::fs::write(dir.join(name), body).unwrap();
                dir.join(name).display().to_string()
            };
            let mut text_line = String::new();
            if text {
                text_line = format!("text = \"sh {} {{in}}\"\n", script("text.sh", TEXT));
            }
            let profile_text = format!(
                "name = \"fake\"\nversion = \"0\"\nps2pdf = \"sh {} {{in}} {{out}}\"\nrender = \"sh {} {{in}} {{out}} {{dpi}}\"\nrun = \"sh {} {{in}}\"\n{text_line}limit = {limit}\ntimeout_ms = {timeout_ms}\n",
                script("ps2pdf.sh", PS2PDF),
                script("render.sh", RENDER),
                script("run.sh", RUN)
            );
            std::fs::write(dir.join("fake.toml"), profile_text).unwrap();
            std::fs::write(dir.join("control"), "").unwrap();
            let profile = profile::load(&dir.join("fake.toml"), &workspace_root()).unwrap();
            Fixture { dir, profile }
        }

        fn control(&self, text: &str) -> &Self {
            std::fs::write(self.dir.join("control"), text).unwrap();
            self
        }

        fn program(&self, name: &str, text: &str) -> PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, text).unwrap();
            path
        }

        fn check(&self, path: &Path) -> FileReport {
            let root = workspace_root();
            let settings = Settings {
                root: &root,
                out_root: self.dir.join("out"),
                profile: &self.profile,
            };
            match check_file(&settings, path) {
                Checked::Report(report) => report,
                Checked::Skipped(feature) => panic!("skipped for {feature}"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const DRAWING: &str = "%!PS\n% expect-output: 3\n% expect-output: 1.5\n1 2 add =\n1.5 =\n0 0 10 10 rectfill showpage\n";

    #[test]
    fn identical_documents_pass_and_leave_their_outputs_behind() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program("drawing.ps", DRAWING);
        let report = fixture.check(&path);
        assert_eq!(report.reasons, Vec::<String>::new());
        assert_eq!(report.verdict, Verdict::Pass);
        assert_eq!(report.output, Output::Same);
        assert_eq!(report.shown, "external/drawing.ps");
        assert_eq!(report.pages_ours, 1);
        assert_eq!(report.pages_theirs, Some(1));
        assert_eq!(report.fractions, [0.0]);
        let out = fixture.dir.join("out").join("external").join("drawing.ps");
        for name in [
            "ours.pdf",
            "theirs.pdf",
            "ours-1.pnm",
            "theirs-1.pnm",
            "ours.stdout",
            "theirs.stdout",
            "ours.txt",
            "theirs.txt",
            "ps2pdf.stderr",
            "run.stderr",
        ] {
            assert!(out.join(name).is_file(), "{name}");
        }
        let page = pnm::parse(&std::fs::read(out.join("ours-1.pnm")).unwrap()).unwrap();
        assert_eq!((page.width, page.height), (306, 396));
        assert_eq!(
            std::fs::read_to_string(out.join("ours.stdout")).unwrap(),
            "3\n1.5\n"
        );
    }

    #[test]
    fn a_page_free_program_compares_output_only() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program("sum.ps", "%!PS\n% expect-output: 3\n1 2 add =\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass);
        assert_eq!(report.output, Output::Same);
        assert_eq!(report.pages_ours, 0);
        assert_eq!(report.pages_theirs, Some(0));
        assert!(report.fractions.is_empty());
    }

    #[test]
    fn the_oracle_run_opens_an_empty_stdin() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program(
            "stdin.ps",
            "%!PS\n% expect-output: 0\n% expect-output: ok\n(%stdin) (r) file dup 4 string readstring pop length = closefile (ok) =\n",
        );
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.output, Output::Same);
        let out = fixture.dir.join("out").join("external").join("stdin.ps");
        assert_eq!(
            std::fs::read_to_string(out.join("ours.stdout")).unwrap(),
            "0\nok\n"
        );
    }

    #[test]
    fn a_blank_page_closing_a_page_free_job_is_no_page() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program("sum.ps", "%!PS\n% expect-output: 3\n1 2 add =\n");
        fixture.control("mark %fake-extra-pages 1\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.pages_theirs, Some(1));
        assert!(report.fractions.is_empty());
        assert_eq!(
            report.notes,
            ["theirs: one blank page where ours shows none, taken as no page"]
        );

        fixture.control("mark %fake-extra-pages 1\nmark %fake-fill 0 1\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.reasons, ["pages: ours 0, theirs 1"]);
        assert!(report.notes.is_empty());

        fixture.control("mark %fake-extra-pages 1\nmark %fake-text words\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            [
                "pages: ours 0, theirs 1",
                "text differs (ours.txt against theirs.txt)"
            ]
        );

        // Two pages are never taken as none.
        fixture.control("mark %fake-extra-pages 2\n");
        let report = fixture.check(&path);
        assert_eq!(report.reasons, ["pages: ours 0, theirs 2"]);
    }

    #[test]
    fn a_declared_error_expects_the_converter_to_end_abnormally() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let declared = fixture.program(
            "erring.ps",
            "%!PS\n% expect-error: undefined\n0 0 10 10 rectfill showpage nosuchname\n",
        );
        fixture.control("fail-after\n");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.pages_theirs, Some(1));
        assert_eq!(report.fractions, [0.0]);
        assert_eq!(
            report.notes,
            [
                "reference converter ended abnormally (exit status: 3), as the file declares undefined"
            ]
        );

        fixture.control("");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["reference converter ended normally where the file declares undefined"]
        );

        let plain = fixture.program("drawing.ps", DRAWING);
        fixture.control("fail-after\n");
        let report = fixture.check(&plain);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["reference converter failed (exit status: 3)"]
        );
        assert_eq!(report.pages_theirs, None);
    }

    #[test]
    fn a_pixel_within_the_limit_passes_and_beyond_it_fails() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program("drawing.ps", DRAWING);
        fixture.control("mark %fake-fill 0 1\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert!(report.fractions[0] > 0.0 && report.fractions[0] < 0.005);

        fixture.control("mark %fake-fill 0 1000\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.output, Output::Same);
        assert!(
            report.reasons[0].contains("of pixels differ"),
            "{:?}",
            report.reasons
        );

        let strict = Fixture::new(0.0, 20_000, false);
        let path = strict.program("drawing.ps", DRAWING);
        strict.control("mark %fake-fill 0 1\n");
        assert_eq!(strict.check(&path).verdict, Verdict::Fail);
        strict.control("mark %fake-fill 240 1000\n");
        assert_eq!(strict.check(&path).verdict, Verdict::Pass);
    }

    #[test]
    fn page_count_and_media_box_mismatches_fail() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program("drawing.ps", DRAWING);
        fixture.control("mark %fake-extra-pages 1\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.pages_theirs, Some(2));
        assert!(
            report
                .reasons
                .contains(&"pages: ours 1, theirs 2".to_string()),
            "{:?}",
            report.reasons
        );

        fixture.control("mark %fake-size 4 4\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert!(
            report.reasons[0].contains("media box ours 612x792, theirs 8x8"),
            "{:?}",
            report.reasons
        );
        assert!(report.reasons[1].contains("rendering ours 306x396, theirs 4x4"));
        assert_eq!(report.fractions, [1.0]);
    }

    #[test]
    fn extracted_text_is_compared_when_the_profile_extracts_it() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program("drawing.ps", DRAWING);
        fixture.control("mark %fake-text world\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["text differs (ours.txt against theirs.txt)"]
        );
        let plain = Fixture::new(0.005, 20_000, false);
        let path = plain.program("drawing.ps", DRAWING);
        plain.control("mark %fake-text world\n");
        assert_eq!(plain.check(&path).verdict, Verdict::Pass);
    }

    #[test]
    fn the_output_channel_is_independent_of_the_document_verdict() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program("drawing.ps", DRAWING);
        fixture.control("output extra\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass);
        assert_eq!(report.output, Output::Differs);
        assert!(report.reasons.is_empty());
        fixture.control("output extra\nmark %fake-fill 0 1000\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.output, Output::Differs);
    }

    #[test]
    fn a_refusing_converter_fails_or_is_the_expected_divergence() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let path = fixture.program("drawing.ps", DRAWING);
        fixture.control("reject\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.pages_theirs, None);
        assert!(
            report.reasons[0].starts_with("reference converter failed (exit status: 3)"),
            "{:?}",
            report.reasons
        );

        let declared = fixture.program(
            "declared.ps",
            &DRAWING.replacen("%!PS\n", "%!PS\n% divergence: font-substitution\n", 1),
        );
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::ExpectedDivergence);
        assert_eq!(report.divergence.as_deref(), Some("font-substitution"));

        fixture.control("");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::DivergenceClosed);
        assert_eq!(report.output, Output::Same);

        // A divergence in the output channel alone keeps it open.
        fixture.control("output extra\n");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::ExpectedDivergence);
        assert_eq!(report.output, Output::Differs);
    }

    #[test]
    fn a_stalled_converter_times_out_and_fails_even_under_a_divergence() {
        let fixture = Fixture::new(0.005, 300, false);
        let path = fixture.program(
            "declared.ps",
            &DRAWING.replacen("%!PS\n", "%!PS\n% divergence: font-substitution\n", 1),
        );
        fixture.control("hang\n");
        let started = Instant::now();
        let report = fixture.check(&path);
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.reasons[0], "reference converter timed out");
        assert_eq!(report.output, Output::Unavailable);
    }

    #[test]
    fn run_files_summarises_and_the_json_report_names_every_file() {
        let fixture = Fixture::new(0.005, 20_000, false);
        let good = fixture.program("a.ps", DRAWING);
        let bad = fixture.program("b.ps", "%!PS\n% expect-output: 3\n1 2 add =\n");
        fixture.control("output extra\n");
        let root = workspace_root();
        let settings = Settings {
            root: &root,
            out_root: fixture.dir.join("out"),
            profile: &fixture.profile,
        };
        let (reports, skipped) = run_files(&settings, &[good, bad]);
        let summary = Summary::of(&reports, skipped);
        assert_eq!(
            summary,
            Summary {
                files: 2,
                pass: 2,
                output_differs: 2,
                ..Default::default()
            }
        );
        assert!(summary.line().starts_with("2 files, 2 pass, 0 fail,"));
        let json = json_report(&fixture.profile, &reports, &summary);
        assert!(json.contains("\"name\": \"fake\""));
        assert!(json.contains("\"path\": \"external/a.ps\", \"verdict\": \"pass\", \"output\": \"differs\", \"divergence\": null, \"pages\": {\"ours\": 1, \"theirs\": 1}, \"fractions\": [0], \"reasons\": [], \"notes\": []"));
        assert!(json.contains("\"summary\": {\"files\": 2, \"pass\": 2, \"fail\": 0,"));
        assert_eq!(json_string("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
    }

    #[test]
    fn json_reports_go_under_the_build_directory_only() {
        let root = workspace_root();
        assert_eq!(
            json_path(
                &root,
                &root.join("target").join("x").join("..").join("r.json")
            )
            .unwrap(),
            root.join("target").join("r.json")
        );
        assert!(
            json_path(&root, &root.join("corpus").join("r.json"))
                .unwrap_err()
                .contains("must name a path under")
        );
        assert!(json_path(&root, &root.join("target").join("..").join("r.json")).is_err());
        assert!(json_path(&root, Path::new("/tmp/r.json")).is_err());
    }

    #[test]
    fn the_registry_is_read_by_requirement_name() {
        let slugs = Registry::slugs_in(
            "# x\n\n### Requirement: font-substitution\n\ntext\n### Requirement:  spaced  \n#### Scenario: not one\n",
        );
        assert_eq!(
            slugs.iter().map(String::as_str).collect::<Vec<_>>(),
            ["font-substitution", "spaced"]
        );

        let root = workspace_root();
        let registry = Registry::load(&root).unwrap();
        assert!(!registry.sources.is_empty());
        assert!(registry.contains("font-substitution"));
        assert!(Registry::load(Path::new("/nonexistent/efterscript")).is_err());
        assert!(Registry::paths(Path::new("/nonexistent/efterscript")).is_empty());
    }

    #[test]
    fn every_declared_divergence_in_the_corpus_resolves() {
        let root = workspace_root();
        let registry = Registry::load(&root).unwrap();
        let mut files = Vec::new();
        collect(&root.join("corpus").join("unit"), &mut files);
        let mut declared = Vec::new();
        for path in files {
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            if let Some(slug) = expectation(&text).divergence {
                assert!(registry.contains(&slug), "{}: {slug}", path.display());
                declared.push(shown(&root, &path));
            }
        }
        for name in [
            "substitution-aliases",
            "substitution-heuristics",
            "substitution-arial",
            "laserwriter-aliases",
        ] {
            assert!(
                declared.contains(&format!("corpus/unit/text/{name}.ps")),
                "{name} declares no divergence"
            );
        }
        assert_eq!(declared.len(), 4);
    }

    #[test]
    fn numbers_are_canonical_and_output_is_normalised() {
        for (given, want) in [
            ("12.0", "12"),
            ("12.500", "12.5"),
            ("-0", "0"),
            ("-0.0", "0"),
            ("+3", "3"),
            (".5", "0.5"),
            ("1.", "1"),
            ("1e10", "1e10"),
            ("-2.50E-3", "-2.5E-3"),
        ] {
            assert_eq!(canonical_number(given).as_deref(), Some(want), "{given}");
        }
        for not in ["", "-", ".", "e5", "1e", "1.2.3", "abc", "12a", "[1.0"] {
            assert_eq!(canonical_number(not), None, "{not}");
        }
        assert_eq!(
            normalise_output("a 1.0  -0 \r\n[1.50 2]\n\n  \n"),
            "a 1  0\n[1.50 2]"
        );
        assert_eq!(normalise_output("x\n"), normalise_output("x"));
        assert_ne!(normalise_output("x\ny"), normalise_output("x\n\ny"));
        assert_eq!(normalise_text("  a \n\tb  c\n"), "a b c");
    }

    #[test]
    fn arguments_are_parsed() {
        let args: Vec<String> = [
            "--profile",
            "p",
            "--dpi",
            "72",
            "--json",
            "target/r.json",
            "a.ps",
            "b",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.profile.as_deref(), Some("p"));
        assert_eq!(parsed.dpi, Some(72));
        assert_eq!(parsed.json.as_deref(), Some(Path::new("target/r.json")));
        assert_eq!(parsed.paths, ["a.ps", "b"]);
        let err = |args: &[&str]| {
            parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_err()
        };
        assert!(err(&["--dpi", "0"]).contains("positive integer"));
        assert!(err(&["--dpi"]).contains("needs a value"));
        assert!(err(&["--what"]).contains("unknown option"));
    }

    #[test]
    fn without_a_profile_the_tier_is_skipped() {
        let env = Env {
            profile_path: None,
            vault: None,
        };
        assert_eq!(run(&[], &env), ExitCode::SUCCESS);
        let named = ["--profile".to_string(), "default".to_string()];
        assert_eq!(run(&named, &env), ExitCode::from(2));
        let missing = Env {
            profile_path: Some(OsString::from("/nonexistent/profile.toml")),
            vault: None,
        };
        assert_eq!(run(&[], &missing), ExitCode::from(2));
    }
}
