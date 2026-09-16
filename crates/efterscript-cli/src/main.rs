// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Command-line front end. The library holds no ambient authority, so this
//! binary is where host files and host stdio are read and handed to the
//! interpreter as streams.

use std::cell::RefCell;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use ps_graphics::{Collected, Graphics};
use ps_vm::{Config, Interp, Io, Outcome, SliceSource, Stream, VmError};
use remelt::{MarkValue, NotHonoured, Options, PdfSink};

struct HostStdout;

impl Stream for HostStdout {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        std::io::stdout()
            .write_all(buf)
            .map(|()| buf.len())
            .map_err(|_| VmError::IoError)
    }

    fn flush(&mut self) -> Result<(), VmError> {
        std::io::stdout().flush().map_err(|_| VmError::IoError)
    }
}

struct HostStderr;

impl Stream for HostStderr {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        std::io::stderr()
            .write_all(buf)
            .map(|()| buf.len())
            .map_err(|_| VmError::IoError)
    }
}

struct HostStdin;

impl Stream for HostStdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        std::io::stdin().read(buf).map_err(|_| VmError::IoError)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: efterscript run <file.ps>");
    eprintln!("       efterscript ir <file.ps>");
    eprintln!("       efterscript pdf [options] <file.ps> [<out.pdf> | -]");
    eprintln!("       efterscript --version");
    eprintln!("pdf options:");
    eprintln!("       --param <Key>=<Value>   a distillation parameter (repeatable)");
    eprintln!("       --no-compress           CompressPages false");
    eprintln!("       --embed-all             EmbedAllFonts true");
    eprintln!("       --no-subset             SubsetFonts false");
    eprintln!("       --lock <Key>            the job may not change <Key> (repeatable)");
    eprintln!(
        "       --identity <Key>=<Value> a statusdict entry (repeatable; a string in parentheses;"
    );
    eprintln!(
        "                                product, version, revision, serialnumber reach systemdict too)"
    );
    eprintln!(
        "       --prelude <file.ps>     a program run once before the job, at the server level"
    );
    ExitCode::from(2)
}

fn exit_code(outcome: Outcome) -> ExitCode {
    match outcome {
        Outcome::Ok | Outcome::Suspended => ExitCode::SUCCESS,
        Outcome::Error(_) => ExitCode::FAILURE,
    }
}

/// Runs the program in `bytes` against the host streams with a graphics
/// backend whose pages are discarded; the exit code is 0 for a job that
/// ended normally and 1 for one ended by an error.
fn run(bytes: &[u8]) -> ExitCode {
    let config = Config {
        io: Io::new(HostStdout, HostStderr).with_stdin(HostStdin),
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    interp.set_graphics_backend(Box::new(Graphics::new(())));
    let outcome = interp.run(&mut SliceSource::new(bytes));
    let _ = std::io::stdout().flush();
    exit_code(outcome)
}

/// Runs the program and prints the IR dump of every page it produced,
/// pages separated by a blank line, then the document marks after the
/// pages when there are any. Only the dump goes to standard output; the
/// program's own output joins the error report on standard error, so
/// the dump can be piped.
fn ir(bytes: &[u8]) -> ExitCode {
    let config = Config {
        io: Io::new(HostStderr, HostStderr).with_stdin(HostStdin),
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let collected = Rc::new(RefCell::new(Collected::default()));
    interp.set_graphics_backend(Box::new(Graphics::new(collected.clone())));
    let outcome = interp.run(&mut SliceSource::new(bytes));
    let text = collected.borrow().dump();
    print!("{text}");
    let _ = std::io::stdout().flush();
    exit_code(outcome)
}

/// What the `pdf` command was asked for.
struct PdfArgs {
    input: String,
    target: PathBuf,
    options: Options,
    /// Command-line parameters the writer does not honour.
    refused: Vec<NotHonoured>,
    /// `statusdict` entries from `--identity`.
    identity: Vec<(String, MarkValue)>,
    /// The prelude file from `--prelude`.
    prelude: Option<PathBuf>,
}

/// A `--param` or `--identity` value: a boolean, an integer, a real, a
/// string in parentheses, else a name (with or without its slash).
fn param_value(text: &str) -> MarkValue {
    match text {
        "true" => MarkValue::Bool(true),
        "false" => MarkValue::Bool(false),
        _ => {
            if let Some(inner) = text
                .strip_prefix('(')
                .and_then(|rest| rest.strip_suffix(')'))
            {
                MarkValue::String(inner.as_bytes().to_vec())
            } else if let Ok(i) = text.parse::<i32>() {
                MarkValue::Int(i)
            } else if let Ok(r) = text.parse::<f32>() {
                MarkValue::Real(r)
            } else {
                MarkValue::Name(text.trim_start_matches('/').as_bytes().to_vec())
            }
        }
    }
}

/// Parses the `pdf` command's arguments. Locks apply after every
/// parameter, so their order on the line does not matter.
fn pdf_args(args: &[&str]) -> Result<PdfArgs, String> {
    let mut options = Options::default();
    let mut refused = Vec::new();
    let mut locks = Vec::new();
    let mut identity = Vec::new();
    let mut prelude = None;
    let mut positional = Vec::new();
    let mut at = 0;
    while at < args.len() {
        let arg = args[at];
        at += 1;
        let mut value = || {
            let value = args.get(at).ok_or_else(|| format!("{arg} needs a value"))?;
            at += 1;
            Ok::<&str, String>(value)
        };
        match arg {
            "--param" => {
                let pair = value()?;
                let (key, text) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--param wants Key=Value, got {pair}"))?;
                let entry = (key.as_bytes().to_vec(), param_value(text));
                refused.extend(options.params.merge(&[entry]));
            }
            "--no-compress" => options.params.compress_pages = false,
            "--embed-all" => options.params.embed_all_fonts = true,
            "--no-subset" => options.params.subset_fonts = false,
            "--lock" => locks.push(value()?.to_string()),
            "--identity" => {
                let pair = value()?;
                let (key, text) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--identity wants Key=Value, got {pair}"))?;
                identity.push((key.to_string(), param_value(text)));
            }
            "--prelude" => prelude = Some(PathBuf::from(value()?)),
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}")),
            _ => positional.push(arg),
        }
    }
    for key in locks {
        options = options.lock(&key);
    }
    let (input, target) = match positional.as_slice() {
        [input] => (*input, Path::new(input).with_extension("pdf")),
        [input, target] => (*input, PathBuf::from(target)),
        _ => return Err("pdf takes an input and at most one output".to_string()),
    };
    Ok(PdfArgs {
        input: input.to_string(),
        target,
        options,
        refused,
        identity,
        prelude,
    })
}

/// Prints the report's lines and returns the job's exit code, or 2 when
/// the document itself could not be written.
fn conclude(
    result: Result<(remelt::Report, ()), remelt::Error>,
    refused: Vec<NotHonoured>,
) -> ExitCode {
    let _ = std::io::stdout().flush();
    match result {
        Ok((report, ())) => {
            if !report.substitutions.is_empty() {
                let pairs: Vec<String> = report
                    .substitutions
                    .iter()
                    .map(|s| {
                        format!(
                            "{} -> {}",
                            String::from_utf8_lossy(&s.requested),
                            s.substitute
                        )
                    })
                    .collect();
                let plural = if pairs.len() == 1 { "" } else { "s" };
                eprintln!(
                    "efterscript: {} font{plural} substituted: {}",
                    pairs.len(),
                    pairs.join(", ")
                );
            }
            for note in &report.notes {
                eprintln!("efterscript: {note}");
            }
            if !report.marks_ignored.is_empty() {
                let total: usize = report.marks_ignored.values().sum();
                let kinds: Vec<String> = report
                    .marks_ignored
                    .iter()
                    .map(|(kind, n)| format!("{kind}×{n}"))
                    .collect();
                eprintln!(
                    "efterscript: {total} pdfmark(s) ignored: {}",
                    kinds.join(", ")
                );
            }
            let refused: Vec<String> = refused
                .iter()
                .chain(&report.not_honoured)
                .map(|(key, text)| format!("{key}={text}"))
                .collect();
            if !refused.is_empty() {
                eprintln!(
                    "efterscript: {} parameter(s) not honoured: {}",
                    refused.len(),
                    refused.join(", ")
                );
            }
            exit_code(report.outcome)
        }
        Err(e) => {
            eprintln!("efterscript: {e}");
            ExitCode::from(2)
        }
    }
}

/// Distils the program into `sink`. The exit code follows the job's
/// outcome once the document is written, so a partially distilled job is
/// still inspectable.
fn distill_to<W: Write + 'static>(
    bytes: &[u8],
    io: Io,
    sink: PdfSink<W>,
    refused: Vec<NotHonoured>,
    identity: Vec<(String, MarkValue)>,
    prelude: Option<Vec<u8>>,
) -> ExitCode {
    let config = Config {
        io,
        identity,
        prelude,
        ..Default::default()
    };
    let result = remelt::distill_into(bytes, config, sink).map(|(report, out)| {
        drop(out);
        (report, ())
    });
    conclude(result, refused)
}

/// Writes the PDF to the target, or to standard output for `-`, in which
/// case the program's own output moves to standard error so the PDF can
/// be piped. A file can seek, so its header names the compatibility
/// level; standard output cannot, so a level below 1.7 is reported.
fn pdf(bytes: &[u8], args: PdfArgs) -> ExitCode {
    let PdfArgs {
        target,
        options,
        refused,
        identity,
        prelude,
        ..
    } = args;
    let prelude = match prelude.map(|path| read_program(&path.to_string_lossy())) {
        Some(Ok(bytes)) => Some(bytes),
        Some(Err(code)) => return code,
        None => None,
    };
    if target == Path::new("-") {
        let io = Io::new(HostStderr, HostStderr).with_stdin(HostStdin);
        return match PdfSink::new(BufWriter::new(std::io::stdout()), options) {
            Ok(sink) => distill_to(bytes, io, sink, refused, identity, prelude),
            Err(e) => conclude(Err(e), refused),
        };
    }
    match std::fs::File::create(&target) {
        Ok(file) => {
            let io = Io::new(HostStdout, HostStderr).with_stdin(HostStdin);
            match PdfSink::new_seekable(BufWriter::new(file), options) {
                Ok(sink) => distill_to(bytes, io, sink, refused, identity, prelude),
                Err(e) => conclude(Err(e), refused),
            }
        }
        Err(e) => {
            eprintln!("efterscript: cannot create {}: {e}", target.display());
            ExitCode::from(2)
        }
    }
}

fn read_program(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        eprintln!("efterscript: cannot read {path}: {e}");
        ExitCode::from(2)
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version" | "-V"] => {
            println!("efterscript {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [command @ ("run" | "ir"), path] => match read_program(path) {
            Ok(bytes) if *command == "run" => run(&bytes),
            Ok(bytes) => ir(&bytes),
            Err(code) => code,
        },
        ["pdf", rest @ ..] => match pdf_args(rest) {
            Ok(parsed) => match read_program(&parsed.input) {
                Ok(bytes) => pdf(&bytes, parsed),
                Err(code) => code,
            },
            Err(message) => {
                eprintln!("efterscript: {message}");
                usage()
            }
        },
        _ => usage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_by_shape() {
        assert_eq!(param_value("true"), MarkValue::Bool(true));
        assert_eq!(param_value("false"), MarkValue::Bool(false));
        assert_eq!(param_value("72"), MarkValue::Int(72));
        assert_eq!(param_value("1.4"), MarkValue::Real(1.4));
        assert_eq!(
            param_value("/Average"),
            MarkValue::Name(b"Average".to_vec())
        );
        assert_eq!(param_value("All"), MarkValue::Name(b"All".to_vec()));
        assert_eq!(
            param_value("(Fictional Press)"),
            MarkValue::String(b"Fictional Press".to_vec())
        );
        assert_eq!(param_value("()"), MarkValue::String(Vec::new()));
    }

    #[test]
    fn identity_and_prelude_are_collected() {
        let parsed = pdf_args(&[
            "--identity",
            "product=(Fictional Press)",
            "--prelude",
            "host.ps",
            "--identity",
            "manualfeed=false",
            "in.ps",
        ])
        .unwrap();
        assert_eq!(
            parsed.identity,
            vec![
                (
                    "product".to_string(),
                    MarkValue::String(b"Fictional Press".to_vec())
                ),
                ("manualfeed".to_string(), MarkValue::Bool(false)),
            ]
        );
        assert_eq!(parsed.prelude, Some(PathBuf::from("host.ps")));
        assert!(pdf_args(&["--identity", "NoEquals", "a.ps"]).is_err());
        assert!(pdf_args(&["--prelude"]).is_err());
    }

    #[test]
    fn flags_shape_the_options_and_locks_apply_last() {
        let parsed = pdf_args(&[
            "--lock",
            "CompressPages",
            "--param",
            "CompressPages=false",
            "--no-subset",
            "--embed-all",
            "in.ps",
            "--param",
            "AutoRotatePages=/All",
            "--param",
            "GrayImageResolution=5",
        ])
        .unwrap();
        assert_eq!(parsed.input, "in.ps");
        assert_eq!(parsed.target, PathBuf::from("in.pdf"));
        assert!(!parsed.options.params.compress_pages);
        assert!(!parsed.options.params.subset_fonts);
        assert!(parsed.options.params.embed_all_fonts);
        assert!(parsed.options.params.locked.contains("CompressPages"));
        assert_eq!(
            parsed.refused,
            vec![
                ("AutoRotatePages".to_string(), "/All".to_string()),
                (
                    "GrayImageResolution".to_string(),
                    "5 (9 to 2400)".to_string()
                ),
            ]
        );
        let parsed = pdf_args(&["--no-compress", "a.ps", "-"]).unwrap();
        assert_eq!(parsed.target, PathBuf::from("-"));
        assert!(!parsed.options.params.compress_pages);
    }

    #[test]
    fn malformed_lines_are_refused() {
        assert!(pdf_args(&["a.ps", "b.pdf", "c"]).is_err());
        assert!(pdf_args(&[]).is_err());
        assert!(pdf_args(&["--param", "NoEquals", "a.ps"]).is_err());
        assert!(pdf_args(&["--lock"]).is_err());
        assert!(pdf_args(&["--bogus", "a.ps"]).is_err());
    }
}
