// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `difftest oracle [--profile <name>] [--dpi <n>] [--json <path>]
//! [path…]`: the differential comparison against the reference
//! converter a profile describes (see [`crate::profile`]).
//!
//! Per corpus file: EfterScript's PDF and the converter's are rendered
//! by the profile's rasteriser and compared page by page (count, media
//! box, pixels, and text when the profile extracts it — except that a
//! page whose fonts in EfterScript's document lack a `ToUnicode` map
//! makes the text not comparable, which is noted and never counted);
//! separately, the program's standard output is compared with the
//! reference interpreter's after normalisation and reported as
//! `output: same`, `differs`, or `unavailable`. When the profile gives
//! an `error_marker`, the reference's output is cut at the marker's
//! first occurrence and the reference is recorded as having ended in
//! error; on a file declaring `% expect-error:` that record, not the
//! converter's exit status, is what counts as agreement. The reference
//! interpreter runs before the converter, so its output is at hand for
//! that decision. A file carrying `% divergence: <slug>`
//! is reported as `expected-divergence` instead of `fail` and as
//! `divergence-closed` when nothing differs any more; every slug must
//! name a requirement in the expected-divergences registry, or the run
//! aborts before comparing anything. A file carrying `% oracle: skip
//! <reason>` is reported as `skipped` with the reason and nothing is run
//! or compared for it. The exit status is non-zero only for `fail`.
//! Everything the commands produce stays under `target/oracle/<path>/`.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ps_graphics::{Collected, DocMark};
use remelt::{Options, PdfSink};

use crate::pnm;
use crate::profile::{self, Profile};
use crate::{Actual, collect, execute_with_stdin, expectation, workspace_root};

/// The names `% divergence:` may declare, read from the
/// expected-divergences specification: the living spec when it exists
/// and the delta of every open change that adds to it, so a slug
/// resolves from the moment its change is proposed until the change is
/// archived into the living text.
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

    /// Where the registry is read from: the living specification first,
    /// then the open changes' deltas in name order; archived changes are
    /// not read.
    pub fn paths(root: &Path) -> Vec<PathBuf> {
        let relative = Path::new("specs")
            .join("expected-divergences")
            .join("spec.md");
        let mut paths = Vec::new();
        let living = root.join("openspec").join(&relative);
        if living.is_file() {
            paths.push(living);
        }
        if let Ok(changes) = std::fs::read_dir(root.join("openspec").join("changes")) {
            let mut deltas: Vec<PathBuf> = changes
                .flatten()
                .map(|entry| entry.path())
                .filter(|dir| dir.file_name().is_some_and(|n| n != "archive"))
                .map(|dir| dir.join(&relative))
                .filter(|spec| spec.is_file())
                .collect();
            deltas.sort();
            paths.extend(deltas);
        }
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
    /// The file declares `% oracle: skip`; nothing was run or compared.
    Skipped,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::ExpectedDivergence => "expected-divergence",
            Verdict::DivergenceClosed => "divergence-closed",
            Verdict::Skipped => "skipped",
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
    /// Meaningless for a skipped file, which compares nothing.
    pub output: Output,
    pub divergence: Option<String>,
    /// The declared reason when the verdict is skipped.
    pub skip: Option<String>,
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
    /// Not run and not reported: the build lacks the named feature.
    Skipped(String),
}

enum Exit {
    Status(ExitStatus),
    TimedOut,
}

/// Kills `child` and, on Unix, every process in the group it leads:
/// the shell's own children — the converter, a pipeline — would
/// otherwise outlive it.
fn kill_all(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `command` through the shell in `dir`, its streams captured to
/// files, killing it and everything it started once `deadline` passes.
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
    let mut shell = Command::new("sh");
    shell
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    // The shell leads a process group of its own, so a timeout can take
    // its children with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        shell.process_group(0);
    }
    let mut child = shell
        .spawn()
        .map_err(|e| format!("cannot run `{command}`: {e}"))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Exit::Status(status));
        }
        if Instant::now() >= deadline {
            kill_all(&mut child);
            return Ok(Exit::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The rotation page `index` (from 0) is written with: its own mark's
/// over the document default, counting only marks delivered before it,
/// as the writer does.
fn rotation(collected: &Collected, index: usize) -> i32 {
    let mut default = None;
    let mut own = None;
    for (at, mark) in &collected.marks {
        if *at > index {
            break;
        }
        match mark {
            DocMark::PagesDefault(attrs) => default = attrs.rotate.or(default),
            DocMark::PageAttr { page, attrs } if *page == index + 1 => {
                own = attrs.rotate.or(own);
            }
            _ => {}
        }
    }
    own.or(default).unwrap_or(0)
}

/// Our document for what a run delivered, uncompressed; a zero-page
/// document for nothing.
fn document(collected: &Collected) -> Result<Vec<u8>, String> {
    let mut sink =
        PdfSink::new(Vec::new(), Options { compress: false }).map_err(|e| e.to_string())?;
    collected.replay(&mut sink);
    sink.finish().map_err(|e| e.to_string())
}

/// The objects of an uncompressed document by number: the bytes
/// between a line `N G obj` and the next line `endobj`. A byte scan
/// that relies on the layout our writer produces — one object per
/// `obj`/`endobj` pair on lines of their own, no object streams — so
/// it reads only the documents this harness writes.
fn objects(pdf: &[u8]) -> HashMap<u32, &[u8]> {
    let mut found = HashMap::new();
    let mut current: Option<(u32, usize)> = None;
    let mut at = 0;
    for line in pdf.split(|&b| b == b'\n') {
        let start = at;
        at += line.len() + 1;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line == b"endobj" {
            if let Some((number, body)) = current.take() {
                found.insert(number, &pdf[body..start]);
            }
            continue;
        }
        if current.is_some() {
            continue;
        }
        let mut words = line.split(|&b| b == b' ');
        let number = words
            .next()
            .and_then(|w| std::str::from_utf8(w).ok())
            .and_then(|w| w.parse::<u32>().ok());
        let generation = words
            .next()
            .is_some_and(|w| w.iter().all(u8::is_ascii_digit) && !w.is_empty());
        if let (Some(number), true, Some(b"obj"), None) =
            (number, generation, words.next(), words.next())
        {
            current = Some((number, at));
        }
    }
    found
}

/// The position after `needle` in `haystack`, from `from`.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p + needle.len())
}

/// The fonts named by a page object's `/Font << /F0 2 0 R … >>`:
/// resource name and object number.
fn page_fonts(page: &[u8]) -> Vec<(String, u32)> {
    let Some(start) = find(page, b"/Font <<", 0) else {
        return Vec::new();
    };
    let end = find(page, b">>", start).map_or(page.len(), |e| e - 2);
    let text = String::from_utf8_lossy(&page[start..end]);
    let words: Vec<&str> = text.split_whitespace().collect();
    words
        .windows(4)
        .filter(|w| w[0].starts_with('/') && w[3] == "R")
        .filter_map(|w| Some((w[0][1..].to_string(), w[1].parse::<u32>().ok()?)))
        .collect()
}

/// Whether the object is a page (not the page tree) dictionary.
fn is_page(body: &[u8]) -> bool {
    let mut from = 0;
    while let Some(after) = find(body, b"/Type /Page", from) {
        if !body.get(after).is_some_and(u8::is_ascii_alphabetic) {
            return true;
        }
        from = after;
    }
    false
}

/// The pages, numbered from 1 in the document's order, whose font
/// resources include one without a `ToUnicode` entry, with those
/// fonts' resource names; a font whose object cannot be found counts
/// as lacking one. Empty for a document without such a page.
pub fn pages_without_unicode(pdf: &[u8]) -> Vec<(usize, Vec<String>)> {
    let objects = objects(pdf);
    let mut pages: Vec<(u32, &[u8])> = objects
        .iter()
        .filter(|(_, body)| is_page(body))
        .map(|(&number, &body)| (number, body))
        .collect();
    // Document order: where each object lies in the file.
    pages.sort_by_key(|(_, body)| body.as_ptr() as usize);
    let mut found = Vec::new();
    for (index, (_, page)) in pages.iter().enumerate() {
        let unmapped: Vec<String> = page_fonts(page)
            .into_iter()
            .filter(|(_, number)| {
                !objects
                    .get(number)
                    .is_some_and(|font| find(font, b"/ToUnicode", 0).is_some())
            })
            .map(|(name, _)| name)
            .collect();
        if !unmapped.is_empty() {
            found.push((index + 1, unmapped));
        }
    }
    found
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

/// Whitespace runs collapsed to one space, ends trimmed, numbers
/// canonical — a shown `cvs` rendering differs in digits between
/// interpreters exactly as a printed one does.
pub fn normalise_text(text: &str) -> String {
    text.split_whitespace()
        .map(canonical_token)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A number token in its shortest form — trailing fraction zeros and a
/// bare point dropped, negative zero made zero, a decimal fraction
/// rounded to six significant digits — or `None` when `token` is not a
/// number. Six digits are what single precision guarantees: interpreters
/// print the same real with six, eight, or nine, and the digits beyond
/// the sixth say nothing about the value.
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
    // A real: a fraction, an exponent, or a whole value of a hundred
    // million or more, which one interpreter prints as digits and
    // another in exponent form. Smaller integers compare exactly.
    let whole_large =
        frac.is_none() && (int.len() > 18 || int.parse::<i64>().is_ok_and(|v| v >= 100_000_000));
    if !exponent.is_empty() || whole_large {
        let value: f64 = format!("{out}{exponent}").parse().ok()?;
        out = six_significant_digits_exponent(value);
        return Some(format!("{}{out}", if out == "0" { "" } else { sign }));
    }
    if out.contains('.') {
        out = six_significant_digits(&out);
    }
    let zero = out.bytes().all(|b| b == b'0' || b == b'.');
    Some(format!("{}{out}", if zero { "" } else { sign }))
}

/// A non-negative value in exponent form with six significant digits,
/// trailing zeros dropped: `1.73631e10`.
fn six_significant_digits_exponent(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let text = format!("{value:.5e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    format!("{mantissa}e{exponent}")
}

/// A non-negative decimal with more than six significant digits, rounded
/// to six; anything shorter unchanged.
fn six_significant_digits(decimal: &str) -> String {
    let significant = decimal
        .bytes()
        .filter(u8::is_ascii_digit)
        .skip_while(|&b| b == b'0')
        .count();
    let Ok(value) = decimal.parse::<f64>() else {
        return decimal.to_string();
    };
    if significant <= 6 || value == 0.0 {
        return decimal.to_string();
    }
    // Digits before the point count toward the six; below one, the
    // zeros after the point do not.
    let magnitude = value.log10().floor() as i32;
    let places = (5 - magnitude).max(0) as usize;
    let rounded = format!("{value:.places$}");
    match rounded.split_once('.') {
        Some((int, frac)) => {
            let frac = frac.trim_end_matches('0');
            if frac.is_empty() {
                int.to_string()
            } else {
                format!("{int}.{frac}")
            }
        }
        None => rounded,
    }
}

/// A token with the brackets and parentheses around it kept and the
/// number inside made canonical: `[1.50]` reads `[1.5]`, `(2.7399902)`
/// as a `cvs` result reads `(2.73999)`.
pub fn canonical_token(token: &str) -> String {
    let open = token.len() - token.trim_start_matches(['[', '(', '{']).len();
    let close = token.len() - token.trim_end_matches([']', ')', '}']).len();
    if open + close >= token.len() {
        return token.to_string();
    }
    let core = &token[open..token.len() - close];
    match canonical_number(core) {
        Some(number) => format!(
            "{}{number}{}",
            &token[..open],
            &token[token.len() - close..]
        ),
        None => token.to_string(),
    }
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
                .map(canonical_token)
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

/// How a file is expected to end: the error it declares, if any, and
/// whether the reference interpreter's output showed the profile's
/// error marker (`None` for a profile without one).
#[derive(Clone, Copy)]
struct Ending<'a> {
    declared: Option<&'a str>,
    reference_error: Option<bool>,
}

/// The document comparison: mismatches go into `report.reasons`; an
/// error is anything that stopped the comparison itself.
fn compare_documents(
    settings: &Settings<'_>,
    path: &Path,
    dir: &Path,
    actual: &Actual,
    ending: Ending<'_>,
    report: &mut FileReport,
    deadline: Instant,
) -> Result<(), String> {
    let profile = settings.profile;
    let Ending {
        declared,
        reference_error,
    } = ending;
    let ours_pdf = dir.join("ours.pdf");
    let ours_document = document(&actual.collected)?;
    let unmapped = pages_without_unicode(&ours_document);
    std::fs::write(&ours_pdf, ours_document)
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
        Exit::Status(status) if !status.success() => match (declared, reference_error) {
            // The file ends in an error on both sides; the document
            // written up to it is still compared.
            (Some(error), None) => report.notes.push(format!(
                "reference converter ended abnormally ({status}), as the file declares {error}"
            )),
            (Some(error), Some(ended)) => {
                report
                    .notes
                    .push(format!("reference converter ended abnormally ({status})"));
                declared_error(error, ended, report);
            }
            (None, _) => {
                report
                    .reasons
                    .push(format!("reference converter failed ({status})"));
                return Ok(());
            }
        },
        Exit::Status(_) => match (declared, reference_error) {
            (Some(error), None) => report.reasons.push(format!(
                "reference converter ended normally where the file declares {error}"
            )),
            (Some(error), Some(ended)) => declared_error(error, ended, report),
            (None, _) => {}
        },
    }
    if !theirs_pdf.is_file() {
        report
            .reasons
            .push("reference converter wrote no document".to_string());
        return Ok(());
    }
    let ours = if actual.collected.pages.is_empty() {
        Vec::new()
    } else {
        render(settings, dir, "ours", &ours_pdf, deadline)?
    };
    if ours.len() != actual.collected.pages.len() {
        return Err(format!(
            "the rasteriser produced {} pages from ours, which has {}",
            ours.len(),
            actual.collected.pages.len()
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
    // Whether every page agreed in size and pixels: with that, text the
    // reference did not extract is text it dropped as invisible.
    let mut pixels_agree = ours.len() == theirs.len();
    for (index, (ours, theirs)) in ours.iter().zip(&theirs).enumerate() {
        let page = index + 1;
        let read = |file: &Path| -> Result<pnm::Image, String> {
            let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
            pnm::parse(&bytes).map_err(|e| format!("{}: {e}", file.display()))
        };
        let (a, b) = (read(ours)?, read(theirs)?);
        let bounds = actual.collected.pages[index].media_box;
        let (width, height) = (
            f64::from(bounds.urx - bounds.llx),
            f64::from(bounds.ury - bounds.lly),
        );
        // A page rotated a quarter turn renders with its sides swapped.
        let (width, height) = if rotation(&actual.collected, index).rem_euclid(180) == 90 {
            (height, width)
        } else {
            (width, height)
        };
        let (theirs_width, theirs_height) = (b.width as f64 * scale, b.height as f64 * scale);
        if (theirs_width - width).abs() > tolerance || (theirs_height - height).abs() > tolerance {
            report.reasons.push(format!(
                "page {page}: media box ours {width}x{height}, theirs {theirs_width}x{theirs_height} (from {}x{} pixels at {} dpi)",
                b.width, b.height, profile.dpi
            ));
            pixels_agree = false;
        }
        if (a.width, a.height) != (b.width, b.height) {
            report.reasons.push(format!(
                "page {page}: rendering ours {}x{}, theirs {}x{} pixels",
                a.width, a.height, b.width, b.height
            ));
            report.fractions.push(1.0);
            pixels_agree = false;
            continue;
        }
        let diff = pnm::compare(&a, &b, profile.threshold)?;
        report.fractions.push(diff.fraction());
        if diff.fraction() > profile.limit {
            pixels_agree = false;
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
        if unmapped.is_empty() {
            let (ours_text, theirs_text) =
                (normalise_text(&ours_text), normalise_text(&theirs_text));
            if ours_text != theirs_text {
                // The reference drops text it deems invisible — off the
                // page, under an empty clip — where ours keeps it; the
                // rasters agreeing is the evidence.
                if pixels_agree && theirs_text.is_empty() {
                    report
                        .notes
                        .push("text: invisible (reference extracted nothing)".to_string());
                } else {
                    report
                        .reasons
                        .push("text differs (ours.txt against theirs.txt)".to_string());
                }
            }
        } else {
            for (page, fonts) in &unmapped {
                report.notes.push(format!(
                    "text not comparable: page {page} font{} {} of ours carries no Unicode mapping",
                    if fonts.len() == 1 { "" } else { "s" },
                    fonts
                        .iter()
                        .map(|f| format!("/{f}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    Ok(())
}

/// What the error marker decided for a file declaring `error`: the
/// reference ending in error is agreement, its ending normally is not.
fn declared_error(error: &str, ended: bool, report: &mut FileReport) {
    if ended {
        report.notes.push(format!(
            "reference interpreter ended in error, as the file declares {error}"
        ));
    } else {
        report.reasons.push(format!(
            "reference interpreter ended normally where the file declares {error}"
        ));
    }
}

/// What running the file through the reference interpreter gave.
struct ReferenceOutput {
    /// Whether its standard output equals ours after normalisation.
    same: bool,
    /// Whether the profile's error marker appeared in it: `None` for a
    /// profile without one.
    ended_in_error: Option<bool>,
}

/// Runs the file through the reference interpreter and compares its
/// standard output with ours, cut at the profile's error marker when
/// there is one; an error when it could not be obtained.
fn compare_output(
    settings: &Settings<'_>,
    path: &Path,
    dir: &Path,
    actual: &Actual,
    deadline: Instant,
) -> Result<ReferenceOutput, String> {
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
    let mut theirs = String::from_utf8_lossy(&theirs).into_owned();
    let ended_in_error =
        settings
            .profile
            .error_marker
            .as_deref()
            .map(|marker| match theirs.find(marker) {
                Some(at) => {
                    theirs.truncate(at);
                    true
                }
                None => false,
            });
    Ok(ReferenceOutput {
        same: normalise_output(&actual.output) == normalise_output(&theirs),
        ended_in_error,
    })
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
        skip: None,
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
    let dir = settings.out_root.join(&shown);
    let cleared = match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    };
    if let Some(reason) = expected.oracle_skip {
        report.verdict = Verdict::Skipped;
        report.skip = Some(reason);
        return Checked::Report(report);
    }
    let actual = execute_with_stdin(&bytes, expected.graphics);
    report.pages_ours = actual.collected.pages.len();
    let prepared = cleared.and_then(|()| std::fs::create_dir_all(&dir));
    if let Err(e) = prepared {
        report
            .reasons
            .push(format!("cannot prepare {}: {e}", dir.display()));
        return Checked::Report(report);
    }
    let deadline = Instant::now() + Duration::from_millis(settings.profile.timeout_ms);
    let mut error = false;
    let mut reference_error = None;
    match compare_output(settings, path, &dir, &actual, deadline) {
        Ok(reference) => {
            report.output = if reference.same {
                Output::Same
            } else {
                Output::Differs
            };
            reference_error = reference.ended_in_error;
            if reference.ended_in_error == Some(true) {
                report.notes.push(
                    "reference interpreter ended in error; its output is compared up to the marker"
                        .to_string(),
                );
            }
        }
        Err(e) => {
            error = true;
            report.reasons.push(e);
        }
    }
    if let Err(e) = compare_documents(
        settings,
        path,
        &dir,
        &actual,
        Ending {
            declared: expected.error.as_deref(),
            reference_error,
        },
        &mut report,
        deadline,
    ) {
        error = true;
        report.reasons.push(e);
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
    /// `unbuilt` counts the files left out for a feature this build
    /// lacks; they and the skipped verdicts share the `skipped` total.
    pub fn of(reports: &[FileReport], unbuilt: usize) -> Self {
        let mut summary = Summary {
            files: reports.len() + unbuilt,
            skipped: unbuilt,
            ..Default::default()
        };
        for report in reports {
            match report.verdict {
                Verdict::Pass => summary.pass += 1,
                Verdict::Fail => summary.fail += 1,
                Verdict::ExpectedDivergence => summary.expected_divergence += 1,
                Verdict::DivergenceClosed => summary.divergence_closed += 1,
                Verdict::Skipped => {
                    summary.skipped += 1;
                    continue;
                }
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
            Checked::Report(report) if report.verdict == Verdict::Skipped => {
                println!(
                    "{:<19} {}  skip: {}",
                    report.verdict.name(),
                    report.shown,
                    report.skip.as_deref().unwrap_or_default()
                );
                reports.push(report);
            }
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
        let skip = report
            .skip
            .as_deref()
            .map_or("null".to_string(), json_string);
        let output = if report.verdict == Verdict::Skipped {
            "null".to_string()
        } else {
            json_string(report.output.name())
        };
        let theirs = report
            .pages_theirs
            .map_or("null".to_string(), |n| n.to_string());
        let fractions: Vec<String> = report.fractions.iter().map(|f| format!("{f}")).collect();
        let reasons: Vec<String> = report.reasons.iter().map(|r| json_string(r)).collect();
        let notes: Vec<String> = report.notes.iter().map(|n| json_string(n)).collect();
        out.push_str(&format!(
            "{}\n    {{\"path\": {}, \"verdict\": {}, \"output\": {output}, \"divergence\": {divergence}, \"skip\": {skip}, \"pages\": {{\"ours\": {}, \"theirs\": {theirs}}}, \"fractions\": [{}], \"reasons\": [{}], \"notes\": [{}]}}",
            if index == 0 { "" } else { "," },
            json_string(&report.shown),
            json_string(report.verdict.name()),
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
    /// `reject`, stalls when it says `hang` (leaving its `sleep` child's
    /// pid in `orphan.pid`), and ends abnormally after writing when it
    /// says `fail-after`.
    const PS2PDF: &str = "#!/bin/sh
export LC_ALL=C
ctl=\"$(dirname \"$0\")/control\"
if grep -q '^reject' \"$ctl\"; then echo refused >&2; exit 3; fi
if grep -q '^hang' \"$ctl\"; then sleep 30 & echo $! > \"$(dirname \"$0\")/orphan.pid\"; wait; fi
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
    /// whatever `%fake-text` says; nothing at all when the document
    /// carries `%fake-silent`.
    const TEXT: &str = "#!/bin/sh
export LC_ALL=C
if grep -a -q '%fake-silent' \"$1\"; then exit 0; fi
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
            Self::build(limit, timeout_ms, text, "")
        }

        /// A fixture whose profile also carries `extra` lines.
        fn build(limit: f64, timeout_ms: u64, text: bool, extra: &str) -> Self {
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
                "name = \"fake\"\nversion = \"0\"\nps2pdf = \"sh {} {{in}} {{out}}\"\nrender = \"sh {} {{in}} {{out}} {{dpi}}\"\nrun = \"sh {} {{in}}\"\n{text_line}limit = {limit}\ntimeout_ms = {timeout_ms}\n{extra}",
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
    fn text_the_reference_did_not_extract_is_invisible_when_the_rasters_agree() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program(
            "offpage.ps",
            "%!PS\n/Helvetica findfont 10 scalefont setfont -100 -100 moveto (a) show showpage\n",
        );
        fixture.control("mark %fake-silent\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(
            report.notes,
            ["text: invisible (reference extracted nothing)"]
        );
        // Differing pixels make it a text difference again.
        fixture.control("mark %fake-silent\nmark %fake-fill 0 1000\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert!(
            report.reasons[0].contains("of pixels differ"),
            "{:?}",
            report.reasons
        );
        assert_eq!(
            report.reasons[1],
            "text differs (ours.txt against theirs.txt)"
        );
        assert!(report.notes.is_empty());
        // So does a page count that differs, and text on both sides.
        fixture.control("mark %fake-silent\nmark %fake-extra-pages 1\n");
        let report = fixture.check(&path);
        assert_eq!(
            report.reasons,
            [
                "pages: ours 1, theirs 2",
                "text differs (ours.txt against theirs.txt)"
            ]
        );
        fixture.control("mark %fake-text world\n");
        let report = fixture.check(&path);
        assert_eq!(
            report.reasons,
            ["text differs (ours.txt against theirs.txt)"]
        );
        assert!(report.notes.is_empty());
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
    fn a_skipped_scenario_runs_nothing_and_is_counted() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program(
            "skipped.ps",
            &DRAWING.replacen(
                "%!PS\n",
                "%!PS\n% oracle: skip build without a graphics backend\n",
                1,
            ),
        );
        // A converter that would refuse, and a stale artefact from an
        // earlier run: neither is seen by a skipped file.
        fixture.control("reject\n");
        let out = fixture.dir.join("out").join("external").join("skipped.ps");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("theirs.pdf"), "stale").unwrap();
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Skipped);
        assert_eq!(
            report.skip.as_deref(),
            Some("build without a graphics backend")
        );
        assert!(report.reasons.is_empty());
        assert_eq!(report.pages_ours, 0);
        assert!(!out.exists());

        let summary = Summary::of(std::slice::from_ref(&report), 1);
        assert_eq!(
            summary,
            Summary {
                files: 2,
                skipped: 2,
                ..Default::default()
            }
        );
        assert!(summary.line().contains("0 fail, 0 expected-divergence, 0 divergence-closed, 2 skipped; output: 0 same, 0 differs, 0 unavailable"));
        let json = json_report(&fixture.profile, &[report], &summary);
        assert!(json.contains("\"verdict\": \"skipped\", \"output\": null, \"divergence\": null, \"skip\": \"build without a graphics backend\""));
        assert!(json.contains("\"skipped\": 2,"));

        // The skip is a header of the leading block; a divergence beside
        // it is recorded but not judged.
        let both = fixture.program(
            "both.ps",
            &DRAWING.replacen(
                "%!PS\n",
                "%!PS\n% divergence: font-substitution\n% oracle: skip no reference\n",
                1,
            ),
        );
        let report = fixture.check(&both);
        assert_eq!(report.verdict, Verdict::Skipped);
        assert_eq!(report.divergence.as_deref(), Some("font-substitution"));
        assert_eq!(report.skip.as_deref(), Some("no reference"));
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
        // The reference interpreter runs first, within the same deadline.
        assert_eq!(report.output, Output::Same);
        // The converter the shell started went down with it.
        let pid = std::fs::read_to_string(fixture.dir.join("orphan.pid")).unwrap();
        let pid = pid.trim();
        let alive = || {
            Command::new("kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        let until = Instant::now() + Duration::from_secs(3);
        while alive() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the converter's child {pid} outlived the timeout");
    }

    /// A Type 3 font whose glyph names the glyph list lacks, so the
    /// document carries no ToUnicode for it.
    const UNMAPPED: &str = "%!PS\n/G << /FontType 3 /FontMatrix [1 0 0 1 0 0] \
        /Encoding 256 array 0 1 255 { 1 index exch /g1 put } for \
        /BuildGlyph { pop pop 1 0 setcharwidth 0 0 0.5 0.5 rectfill } >> definefont \
        10 scalefont setfont 100 100 moveto (a) show showpage\n";

    #[test]
    fn pages_whose_fonts_lack_a_unicode_mapping_are_found() {
        let none = crate::execute(DRAWING.as_bytes());
        assert_eq!(pages_without_unicode(none.pdf.as_ref().unwrap()), []);
        let mapped = crate::execute(
            b"/Helvetica findfont 10 scalefont setfont 100 100 moveto (a) show showpage",
        );
        assert_eq!(pages_without_unicode(mapped.pdf.as_ref().unwrap()), []);
        let unmapped = crate::execute(UNMAPPED.as_bytes());
        assert_eq!(
            pages_without_unicode(unmapped.pdf.as_ref().unwrap()),
            [(1, vec!["F0".to_string()])]
        );
        // A second page with the unmapped font after a page without it.
        let second = format!(
            "/Helvetica findfont 10 scalefont setfont 100 100 moveto (a) show showpage\n{}",
            UNMAPPED.trim_start_matches("%!PS\n")
        );
        let two = crate::execute(second.as_bytes());
        assert_eq!(two.collected.pages.len(), 2);
        let found = pages_without_unicode(two.pdf.as_ref().unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 2);
        assert_eq!(found[0].1.len(), 1);
        // The scan reads only what the writer lays out.
        assert_eq!(pages_without_unicode(b"%PDF-1.7\n"), []);
        assert_eq!(
            pages_without_unicode(
                b"1 0 obj\n<< /Type /Page /Resources << /Font << /F0 9 0 R >> >> >>\nendobj\n2 0 obj\n<< /Type /Pages >>\nendobj\n"
            ),
            [(1, vec!["F0".to_string()])]
        );
    }

    #[test]
    fn text_without_a_unicode_mapping_is_not_comparable() {
        let fixture = Fixture::new(0.005, 20_000, true);
        let path = fixture.program("unmapped.ps", UNMAPPED);
        fixture.control("mark %fake-text world\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(
            report.notes,
            ["text not comparable: page 1 font /F0 of ours carries no Unicode mapping"]
        );
        let out = fixture.dir.join("out").join("external").join("unmapped.ps");
        assert!(out.join("ours.txt").is_file());
        // The other checks still count.
        fixture.control("mark %fake-text world\nmark %fake-extra-pages 1\n");
        let report = fixture.check(&path);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.reasons, ["pages: ours 1, theirs 2"]);
        // A mapped font on the same page keeps the comparison.
        let mapped = fixture.program(
            "mapped.ps",
            "%!PS\n/Helvetica findfont 10 scalefont setfont 100 100 moveto (a) show showpage\n",
        );
        fixture.control("mark %fake-text world\n");
        let report = fixture.check(&mapped);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["text differs (ours.txt against theirs.txt)"]
        );
        assert!(report.notes.is_empty());
    }

    #[test]
    fn an_error_marker_cuts_the_reference_output_and_judges_declared_errors() {
        let fixture = Fixture::build(0.005, 20_000, false, "error_marker = \"Oops: /\"\n");
        assert_eq!(fixture.profile.error_marker.as_deref(), Some("Oops: /"));
        let plain = fixture.program("drawing.ps", DRAWING);
        // Everything from the marker on is left out of the comparison.
        fixture.control("output Oops: /undefined in nosuchname\noutput Operand stack:\n");
        let report = fixture.check(&plain);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.output, Output::Same);
        assert_eq!(
            report.notes,
            ["reference interpreter ended in error; its output is compared up to the marker"]
        );
        fixture.control("output extra\noutput Oops: /undefined in nosuchname\n");
        let report = fixture.check(&plain);
        assert_eq!(report.output, Output::Differs);
        fixture.control("");
        let report = fixture.check(&plain);
        assert_eq!(report.output, Output::Same);
        assert!(report.notes.is_empty());

        // On a declared-error file the marker, not the converter's exit
        // status, is the agreement.
        let declared = fixture.program(
            "erring.ps",
            "%!PS\n% expect-error: undefined\n0 0 10 10 rectfill showpage nosuchname\n",
        );
        fixture.control("output Oops: /undefined in nosuchname\n");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.output, Output::Same);
        assert_eq!(
            report.notes,
            [
                "reference interpreter ended in error; its output is compared up to the marker",
                "reference interpreter ended in error, as the file declares undefined"
            ]
        );
        fixture.control("fail-after\noutput Oops: /undefined in nosuchname\n");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Pass, "{:?}", report.reasons);
        assert_eq!(report.pages_theirs, Some(1));
        assert_eq!(
            report.notes,
            [
                "reference interpreter ended in error; its output is compared up to the marker",
                "reference converter ended abnormally (exit status: 3)",
                "reference interpreter ended in error, as the file declares undefined"
            ]
        );
        fixture.control("fail-after\n");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["reference interpreter ended normally where the file declares undefined"]
        );
        assert_eq!(
            report.notes,
            ["reference converter ended abnormally (exit status: 3)"]
        );
        fixture.control("");
        let report = fixture.check(&declared);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["reference interpreter ended normally where the file declares undefined"]
        );
        // An undeclared file still fails on the converter's exit status.
        fixture.control("fail-after\noutput Oops: /x in y\n");
        let report = fixture.check(&plain);
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(
            report.reasons,
            ["reference converter failed (exit status: 3)"]
        );
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
        assert!(json.contains("\"path\": \"external/a.ps\", \"verdict\": \"pass\", \"output\": \"differs\", \"divergence\": null, \"skip\": null, \"pages\": {\"ours\": 1, \"theirs\": 1}, \"fractions\": [0], \"reasons\": [], \"notes\": []"));
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
    fn the_registry_is_the_union_of_the_living_spec_and_open_deltas() {
        let root = workspace_root()
            .join("target")
            .join("registry-tests")
            .join(std::process::id().to_string());
        let _ = std::fs::remove_dir_all(&root);
        let spec = |dir: &Path, slug: &str| {
            let dir = dir.join("specs").join("expected-divergences");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("spec.md"),
                format!("# expected-divergences\n\n### Requirement: {slug}\n\ntext\n"),
            )
            .unwrap();
            dir.join("spec.md")
        };
        let openspec = root.join("openspec");
        let living = spec(&openspec, "living");
        let open = spec(&openspec.join("changes").join("open"), "proposed");
        spec(
            &openspec
                .join("changes")
                .join("archive")
                .join("2026-01-01-old"),
            "archived",
        );
        let registry = Registry::load(&root).unwrap();
        assert_eq!(registry.sources, [living.clone(), open.clone()]);
        assert!(registry.contains("living"));
        assert!(registry.contains("proposed"));
        assert!(!registry.contains("archived"));

        std::fs::remove_file(&living).unwrap();
        let registry = Registry::load(&root).unwrap();
        assert_eq!(registry.sources, [open]);
        assert!(!registry.contains("living"));
        assert!(registry.contains("proposed"));
        std::fs::remove_dir_all(&root).unwrap();

        // Every slug an open change proposes resolves in this workspace.
        let root = workspace_root();
        let registry = Registry::load(&root).unwrap();
        let mut deltas = 0;
        for path in Registry::paths(&root) {
            if path.starts_with(root.join("openspec").join("changes")) {
                deltas += 1;
            }
            for slug in Registry::slugs_in(&std::fs::read_to_string(&path).unwrap()) {
                assert!(registry.contains(&slug), "{}: {slug}", path.display());
            }
        }
        assert_eq!(registry.sources.len(), 1 + deltas);
    }

    #[test]
    fn a_pages_rotation_comes_from_the_marks_before_it() {
        use ps_graphics::PageAttrs;
        let rotate = |r: i32| PageAttrs {
            crop_box: None,
            rotate: Some(r),
        };
        let collected = Collected {
            pages: vec![
                ps_graphics::Page::new(ps_vm::Bounds::new(0.0, 0.0, 1.0, 1.0)),
                ps_graphics::Page::new(ps_vm::Bounds::new(0.0, 0.0, 1.0, 1.0)),
            ],
            marks: vec![
                (0, DocMark::PagesDefault(rotate(90))),
                (
                    0,
                    DocMark::PageAttr {
                        page: 1,
                        attrs: rotate(180),
                    },
                ),
                (2, DocMark::PagesDefault(rotate(270))),
            ],
        };
        assert_eq!(rotation(&collected, 0), 180, "the page's own wins");
        assert_eq!(rotation(&collected, 1), 90, "the default before it");
        assert_eq!(rotation(&Collected::default(), 0), 0);
    }

    #[test]
    fn every_declared_divergence_in_the_corpus_resolves() {
        let root = workspace_root();
        let registry = Registry::load(&root).unwrap();
        let mut files = Vec::new();
        collect(&root.join("corpus").join("unit"), &mut files);
        let mut declared: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        let mut skipped = Vec::new();
        for path in files {
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            let expected = expectation(&text);
            if let Some(slug) = expected.divergence {
                assert!(registry.contains(&slug), "{}: {slug}", path.display());
                declared.entry(slug).or_default().push(shown(&root, &path));
            }
            if let Some(reason) = expected.oracle_skip {
                assert!(
                    !reason.is_empty(),
                    "{}: skip without a reason",
                    path.display()
                );
                skipped.push(shown(&root, &path));
            }
        }
        let counts: Vec<(&str, usize)> = declared
            .iter()
            .map(|(slug, files)| (slug.as_str(), files.len()))
            .collect();
        assert_eq!(
            counts,
            [
                ("bitshift-zero-fill", 1),
                ("cvrs-negative-unsigned", 1),
                ("file-access-policy", 1),
                ("fmaptype-cmap-only", 1),
                ("font-substitution", 5),
                ("integer-range", 2),
                ("job-server-save-level", 1),
                ("malformed-font-invalidfont", 2),
                ("pagedevice-records-unknown-keys", 1),
                ("procedure-nesting-limit", 1),
                ("radix-without-digits", 1),
                ("resident-inventory", 9),
                ("resident-metrics-only", 1),
                ("resource-size-unknown", 5),
                ("unspecified-forall-order", 1),
                ("vertical-default-metrics", 1),
            ]
        );
        for name in [
            "substitution-aliases",
            "substitution-heuristics",
            "substitution-arial",
            "laserwriter-aliases",
            "derived-fonts",
        ] {
            assert!(
                declared["font-substitution"].contains(&format!("corpus/unit/text/{name}.ps")),
                "{name} declares no divergence"
            );
        }
        assert_eq!(
            skipped,
            [
                "corpus/unit/graphics/no-backend-moveto-undefined.ps",
                "corpus/unit/graphics/no-backend-names-unknown.ps",
                "corpus/unit/interp/deep-recursion.ps",
                "corpus/unit/pdfmark/guarded-idiom-no-backend.ps",
                "corpus/unit/text/no-backend-fonts.ps",
            ]
        );
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
            ("-2.50E-3", "-2.5e-3"),
            ("17363068928", "1.73631e10"),
            ("1.73630689e+10", "1.73631e10"),
            ("-230401056", "-2.30401e8"),
            ("-2.30401e+08", "-2.30401e8"),
            ("99999999", "99999999"),
            ("100000000", "1e8"),
            ("2147483648", "2.14748e9"),
            // Six significant digits: the same single-precision value
            // printed with eight, nine, or six digits reads the same.
            ("1.9098268", "1.90983"),
            ("1.90982676", "1.90983"),
            ("-1.2018454", "-1.20185"),
            ("-1.20185", "-1.20185"),
            ("-128831.22", "-128831"),
            ("-128831.219", "-128831"),
            ("0.0026764297", "0.00267643"),
            ("1234567.5", "1234568"),
            ("0.5000002", "0.5"),
            ("0.73138183", "0.731382"),
            ("0.7313537", "0.731354"),
            ("12345678", "12345678"),
            ("0.0000001", "0.0000001"),
        ] {
            assert_eq!(canonical_number(given).as_deref(), Some(want), "{given}");
        }
        for not in ["", "-", ".", "e5", "1e", "1.2.3", "abc", "12a", "[1.0"] {
            assert_eq!(canonical_number(not), None, "{not}");
        }
        assert_eq!(
            normalise_output("a 1.0  -0 \r\n[1.50 2]\n\n  \n"),
            "a 1  0\n[1.5 2]"
        );
        assert_eq!(canonical_token("[26.662521]"), "[26.6625]");
        assert_eq!(canonical_token("[26.6625214]"), "[26.6625]");
        assert_eq!(canonical_token("(2.7399902)"), "(2.73999)");
        assert_eq!(canonical_token("[-dict-"), "[-dict-");
        assert_eq!(canonical_token("[]"), "[]");
        assert_eq!(canonical_token("()"), "()");
        assert_eq!(canonical_token("(abc)"), "(abc)");
        assert_eq!(normalise_output("x\n"), normalise_output("x"));
        assert_ne!(normalise_output("x\ny"), normalise_output("x\n\ny"));
        assert_eq!(normalise_text("  a \n\tb  c\n"), "a b c");
        assert_eq!(normalise_text(" 1.4115009 x"), normalise_text("1.4115 x"));
        assert_ne!(normalise_text("1.4115 x"), normalise_text("1.4116 x"));
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
