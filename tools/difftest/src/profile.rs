// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Reference-converter profiles: the flat `key = value` file that tells
//! the oracle subcommand how to convert, render, run, and optionally
//! extract text, and where such a file may come from.
//!
//! Grammar: one `key = value` per line; blank lines and lines starting
//! with `#` are ignored; a `"…"` string honours `\"` and `\\`; a bare
//! value is a number. Keys: `name`, `version`, `ps2pdf`, `render`, `run`
//! (strings, required), `text` and `error_marker` (strings, optional),
//! `dpi`, `threshold`, `limit`, `timeout_ms` (numbers, optional).
//! Command strings carry the placeholders `{in}`, `{out}`, and `{dpi}`;
//! the harness substitutes shell-quoted paths and the resolution, and a
//! command runs through `sh -c` from the file's output directory.
//! `error_marker` is the text the reference interpreter's error report
//! starts with on standard output: the harness compares its output only
//! up to the marker's first occurrence and records that the reference
//! ended in error.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const DEFAULT_DPI: u32 = 36;
pub const DEFAULT_THRESHOLD: u8 = 48;
pub const DEFAULT_LIMIT: f64 = 0.005;
pub const DEFAULT_TIMEOUT_MS: u64 = 20_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub version: String,
    /// `{in}` the PostScript file, `{out}` the PDF to write.
    pub ps2pdf: String,
    /// `{in}` a PDF, `{out}` a path pattern with `%d` for the 1-based
    /// page number, `{dpi}` the resolution; one PNM per page.
    pub render: String,
    /// `{in}` the PostScript file; standard output is the program's.
    pub run: String,
    /// `{in}` a PDF; standard output is its text.
    pub text: Option<String>,
    /// Where the reference interpreter's error report begins on its
    /// standard output, when the profile describes it.
    pub error_marker: Option<String>,
    pub dpi: u32,
    /// The channel difference above which a pixel differs.
    pub threshold: u8,
    /// The fraction of differing pixels above which a page fails.
    pub limit: f64,
    pub timeout_ms: u64,
}

const STRING_KEYS: [&str; 7] = [
    "name",
    "version",
    "ps2pdf",
    "render",
    "run",
    "text",
    "error_marker",
];
const NUMBER_KEYS: [&str; 4] = ["dpi", "threshold", "limit", "timeout_ms"];

enum Value {
    Text(String),
    Number(f64),
}

fn parse_string(rest: &str, line: usize) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = rest.chars();
    loop {
        match chars.next() {
            None => return Err(format!("line {line}: unterminated string")),
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    return Err(format!("line {line}: unknown escape `\\{other}`"));
                }
                None => return Err(format!("line {line}: unterminated string")),
            },
            Some(c) => out.push(c),
        }
    }
    let trailing = chars.as_str().trim();
    if !trailing.is_empty() && !trailing.starts_with('#') {
        return Err(format!("line {line}: text after the closing quote"));
    }
    Ok(out)
}

fn parse_value(raw: &str, line: usize) -> Result<Value, String> {
    if let Some(rest) = raw.strip_prefix('"') {
        return parse_string(rest, line).map(Value::Text);
    }
    let bare = raw.split('#').next().unwrap_or("").trim();
    bare.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map(Value::Number)
        .ok_or_else(|| format!("line {line}: `{bare}` is neither a quoted string nor a number"))
}

fn integer(n: f64, key: &str, min: f64, max: f64) -> Result<u64, String> {
    if n.fract() != 0.0 || n < min || n > max {
        return Err(format!(
            "`{key}` must be an integer between {min} and {max}, not {n}"
        ));
    }
    Ok(n as u64)
}

fn placeholders(command: &str, key: &str, needed: &[&str]) -> Result<(), String> {
    for placeholder in needed {
        if !command.contains(placeholder) {
            return Err(format!("`{key}` lacks the {placeholder} placeholder"));
        }
    }
    Ok(())
}

/// Parses a profile; every violation of the grammar is an error naming
/// the line or key.
pub fn parse(text: &str) -> Result<Profile, String> {
    let mut strings: Vec<(String, String)> = Vec::new();
    let mut numbers: Vec<(String, f64)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            return Err(format!("line {number}: expected `key = value`"));
        };
        let key = key.trim();
        let raw = raw.trim();
        if strings.iter().any(|(k, _)| k == key) || numbers.iter().any(|(k, _)| k == key) {
            return Err(format!("line {number}: `{key}` given twice"));
        }
        match (parse_value(raw, number)?, key) {
            (Value::Text(s), k) if STRING_KEYS.contains(&k) => strings.push((k.to_string(), s)),
            (Value::Number(n), k) if NUMBER_KEYS.contains(&k) => numbers.push((k.to_string(), n)),
            (Value::Text(_), k) if NUMBER_KEYS.contains(&k) => {
                return Err(format!("line {number}: `{k}` must be a number"));
            }
            (Value::Number(_), k) if STRING_KEYS.contains(&k) => {
                return Err(format!("line {number}: `{k}` must be a quoted string"));
            }
            (_, k) => return Err(format!("line {number}: unknown key `{k}`")),
        }
    }
    let string = |key: &str| -> Result<String, String> {
        strings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| format!("`{key}` is missing"))
    };
    let number = |key: &str| numbers.iter().find(|(k, _)| k == key).map(|(_, n)| *n);
    let name = string("name")?;
    let version = string("version")?;
    let ps2pdf = string("ps2pdf")?;
    placeholders(&ps2pdf, "ps2pdf", &["{in}", "{out}"])?;
    let render = string("render")?;
    placeholders(&render, "render", &["{in}", "{out}"])?;
    let run = string("run")?;
    placeholders(&run, "run", &["{in}"])?;
    let text = string("text").ok();
    if let Some(text) = &text {
        placeholders(text, "text", &["{in}"])?;
    }
    let error_marker = string("error_marker").ok();
    if error_marker.as_deref() == Some("") {
        return Err("`error_marker` must not be empty".to_string());
    }
    let limit = number("limit").unwrap_or(DEFAULT_LIMIT);
    if !(0.0..=1.0).contains(&limit) {
        return Err(format!("`limit` must be between 0 and 1, not {limit}"));
    }
    Ok(Profile {
        name,
        version,
        ps2pdf,
        render,
        run,
        text,
        error_marker,
        dpi: number("dpi").map_or(Ok(DEFAULT_DPI), |n| {
            integer(n, "dpi", 1.0, f64::from(u32::MAX)).map(|n| n as u32)
        })?,
        threshold: number("threshold").map_or(Ok(DEFAULT_THRESHOLD), |n| {
            integer(n, "threshold", 0.0, 255.0).map(|n| n as u8)
        })?,
        limit,
        timeout_ms: number("timeout_ms").map_or(Ok(DEFAULT_TIMEOUT_MS), |n| {
            integer(n, "timeout_ms", 1.0, 1e12)
        })?,
    })
}

/// Whether `name` may name a profile under the vault: one path component
/// of unsurprising characters.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Where the profile comes from: `--profile <name>` under the vault's
/// `oracles/` directory, else the path in `EFTERSCRIPT_ORACLE_PROFILE`,
/// else nowhere (the tier is skipped).
pub fn locate(
    name: Option<&str>,
    env_path: Option<&OsStr>,
    vault: Option<&OsStr>,
) -> Result<Option<PathBuf>, String> {
    if let Some(name) = name {
        if !valid_name(name) {
            return Err(format!("`{name}` is not a profile name"));
        }
        let Some(vault) = vault else {
            return Err(format!(
                "--profile {name} needs EFTERSCRIPT_HELLBOX to name the vault"
            ));
        };
        return Ok(Some(
            Path::new(vault)
                .join("oracles")
                .join(format!("{name}.toml")),
        ));
    }
    Ok(env_path.map(PathBuf::from))
}

/// Reads and parses the profile at `path`, refusing one that lies inside
/// the repository at `root`: profile commands run through the shell, so
/// they come from the vault only.
pub fn load(path: &Path, root: &Path) -> Result<Profile, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("profile {}: {e}", path.display()))?;
    if let Ok(root) = root.canonicalize()
        && canonical.starts_with(&root)
    {
        return Err(format!(
            "profile {} lies inside the repository; profiles come from the vault",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("profile {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("profile {}: {e}", path.display()))
}

/// `value` as one shell word.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The command line for `template` with its placeholders filled: paths
/// quoted, the resolution bare.
pub fn fill(template: &str, input: &Path, output: Option<&Path>, dpi: u32) -> String {
    let mut command = template.replace("{in}", &quote(&input.to_string_lossy()));
    if let Some(output) = output {
        command = command.replace("{out}", &quote(&output.to_string_lossy()));
    }
    command.replace("{dpi}", &dpi.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "name = \"fake\"\nversion = \"1\"\nps2pdf = \"a {in} {out}\"\nrender = \"b {in} {out} {dpi}\"\nrun = \"c {in}\"\n";

    #[test]
    fn defaults_apply_to_a_minimal_profile() {
        let p = parse(MINIMAL).unwrap();
        assert_eq!(p.name, "fake");
        assert_eq!(p.version, "1");
        assert_eq!(p.ps2pdf, "a {in} {out}");
        assert_eq!(p.text, None);
        assert_eq!(p.error_marker, None);
        assert_eq!(p.dpi, DEFAULT_DPI);
        assert_eq!(p.threshold, DEFAULT_THRESHOLD);
        assert_eq!(p.limit, DEFAULT_LIMIT);
        assert_eq!(p.timeout_ms, DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn every_key_comments_and_escapes_are_read() {
        let text = format!(
            "# a comment\n\n{MINIMAL}text = \"t \\\"q\\\" \\\\ {{in}}\"  # trailing\n  dpi = 72\nthreshold=0\nlimit = 0.25 # note\ntimeout_ms = 500\nerror_marker = \"Oops: \"\n"
        );
        let p = parse(&text).unwrap();
        assert_eq!(p.text.as_deref(), Some("t \"q\" \\ {in}"));
        assert_eq!(p.error_marker.as_deref(), Some("Oops: "));
        assert_eq!(p.dpi, 72);
        assert_eq!(p.threshold, 0);
        assert_eq!(p.limit, 0.25);
        assert_eq!(p.timeout_ms, 500);
    }

    #[test]
    fn violations_name_the_line_or_key() {
        let err = |text: &str| parse(text).unwrap_err();
        assert!(err("nonsense\n").contains("line 1: expected `key = value`"));
        assert!(
            err(&format!("{MINIMAL}colour = \"x\"\n")).contains("line 6: unknown key `colour`")
        );
        assert!(err(&format!("{MINIMAL}dpi = \"36\"\n")).contains("`dpi` must be a number"));
        assert!(err(&format!("{MINIMAL}name = \"again\"\n")).contains("`name` given twice"));
        assert!(err("name = 3\n").contains("`name` must be a quoted string"));
        assert!(err("name = \"open\n").contains("unterminated string"));
        assert!(err("name = \"a\" b\n").contains("after the closing quote"));
        assert!(err("name = \"\\x\"\n").contains("unknown escape"));
        assert!(err("name = yes\n").contains("neither a quoted string nor a number"));
        assert!(err("name = \"a\"\n").contains("`version` is missing"));
        assert!(err(&MINIMAL.replace("{out} {dpi}", "{dpi}")).contains("`render` lacks the {out}"));
        assert!(err(&MINIMAL.replace("c {in}", "c")).contains("`run` lacks the {in}"));
        assert!(err(&format!("{MINIMAL}text = \"t\"\n")).contains("`text` lacks the {in}"));
        assert!(
            err(&format!("{MINIMAL}error_marker = \"\"\n"))
                .contains("`error_marker` must not be empty")
        );
        assert!(err(&format!("{MINIMAL}error_marker = 1\n")).contains("must be a quoted string"));
        assert!(
            err(&format!("{MINIMAL}threshold = 300\n"))
                .contains("`threshold` must be an integer between 0 and 255")
        );
        assert!(err(&format!("{MINIMAL}dpi = 1.5\n")).contains("`dpi` must be an integer"));
        assert!(err(&format!("{MINIMAL}limit = 2\n")).contains("`limit` must be between 0 and 1"));
        assert!(err(&format!("{MINIMAL}timeout_ms = 0\n")).contains("`timeout_ms`"));
    }

    #[test]
    fn location_prefers_the_named_vault_profile() {
        let vault = OsStr::new("/vault");
        let env = OsStr::new("/elsewhere/p.toml");
        assert_eq!(locate(None, None, None).unwrap(), None);
        assert_eq!(
            locate(None, Some(env), Some(vault)).unwrap(),
            Some(PathBuf::from("/elsewhere/p.toml"))
        );
        assert_eq!(
            locate(Some("default"), Some(env), Some(vault)).unwrap(),
            Some(PathBuf::from("/vault/oracles/default.toml"))
        );
        assert!(
            locate(Some("default"), None, None)
                .unwrap_err()
                .contains("EFTERSCRIPT_HELLBOX")
        );
        assert!(locate(Some("../x"), None, Some(vault)).is_err());
        assert!(locate(Some(".hidden"), None, Some(vault)).is_err());
        assert!(locate(Some(""), None, Some(vault)).is_err());
    }

    #[test]
    fn profiles_inside_the_repository_are_refused() {
        let root = crate::workspace_root();
        let dir = root.join("target").join("profile-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let inside = dir.join(format!("inside-{}.toml", std::process::id()));
        std::fs::write(&inside, MINIMAL).unwrap();
        let err = load(&inside, &root).unwrap_err();
        assert!(err.contains("inside the repository"), "{err}");
        std::fs::remove_file(&inside).unwrap();

        let outside =
            std::env::temp_dir().join(format!("efterscript-profile-{}.toml", std::process::id()));
        std::fs::write(&outside, MINIMAL).unwrap();
        assert_eq!(load(&outside, &root).unwrap().name, "fake");
        std::fs::write(&outside, "name = 1\n").unwrap();
        assert!(
            load(&outside, &root)
                .unwrap_err()
                .contains("must be a quoted string")
        );
        std::fs::remove_file(&outside).unwrap();
        assert!(load(&outside, &root).unwrap_err().starts_with("profile "));
    }

    #[test]
    fn placeholders_are_filled_with_quoted_paths() {
        let command = fill(
            "conv {dpi} {in} {out} {in}",
            Path::new("/a b/it's.ps"),
            Some(Path::new("/out/page-%d.pnm")),
            36,
        );
        assert_eq!(
            command,
            "conv 36 '/a b/it'\\''s.ps' '/out/page-%d.pnm' '/a b/it'\\''s.ps'"
        );
        assert_eq!(
            fill("x {in} {out}", Path::new("/p"), None, 1),
            "x '/p' {out}"
        );
    }
}
