// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask lint-strings`: with `EFTERSCRIPT_HELLBOX` set, reads the
//! vault's `oracles/denylist.txt` (one string per line, `#` comments and
//! blank lines ignored) and scans every text file `git ls-files` reports
//! for each string, case-insensitively. A hit is printed as
//! `path:line: <masked>` — the string's first character and asterisks,
//! so the lint's own output never carries it — and fails the run.
//! Without the vault, or without the denylist, it skips with a message.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The strings a denylist lists.
pub fn denylist(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// `needle` with everything after its first character hidden.
pub fn mask(needle: &str) -> String {
    let mut chars = needle.chars();
    let mut out: String = chars.next().into_iter().collect();
    out.extend(chars.map(|_| '*'));
    out
}

/// Whether the file is binary: a NUL among its first 8 KB.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub masked: String,
}

/// The hits in one file's text; each needle is its lower-case form
/// with the mask of its listed spelling.
pub fn scan_text(path: &str, text: &str, needles: &[(String, String)]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        for (needle, masked) in needles {
            if lower.contains(needle.as_str()) {
                hits.push(Hit {
                    path: path.to_string(),
                    line: index + 1,
                    masked: masked.clone(),
                });
            }
        }
    }
    hits
}

/// Scans `files` (relative to `root`), skipping binary and missing ones.
pub fn scan_files(root: &Path, files: &[PathBuf], listed: &[String]) -> Result<Vec<Hit>, String> {
    let needles: Vec<(String, String)> =
        listed.iter().map(|s| (s.to_lowercase(), mask(s))).collect();
    let mut hits = Vec::new();
    for file in files {
        let path = root.join(file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if is_binary(&bytes) {
            continue;
        }
        let shown = file.to_string_lossy();
        hits.extend(scan_text(
            &shown,
            &String::from_utf8_lossy(&bytes),
            &needles,
        ));
    }
    Ok(hits)
}

/// The files git tracks under `root`, relative to it.
pub fn tracked_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| PathBuf::from(String::from_utf8_lossy(name).into_owned()))
        .collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Skipped(String),
    Clean { files: usize, strings: usize },
    Hits(Vec<Hit>),
}

/// Lints the tracked files of `root` against the vault's denylist.
pub fn lint(root: &Path, vault: Option<&Path>) -> Result<Outcome, String> {
    let Some(vault) = vault else {
        return Ok(Outcome::Skipped(
            "lint-strings skipped: no vault".to_string(),
        ));
    };
    let path = vault.join("oracles").join("denylist.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Outcome::Skipped(format!(
                "lint-strings skipped: {} does not exist",
                path.display()
            )));
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let listed = denylist(&text);
    let files = tracked_files(root)?;
    let hits = scan_files(root, &files, &listed)?;
    Ok(if hits.is_empty() {
        Outcome::Clean {
            files: files.len(),
            strings: listed.len(),
        }
    } else {
        Outcome::Hits(hits)
    })
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

pub fn run(args: &[String]) -> ExitCode {
    if let Some(arg) = args.first() {
        eprintln!("lint-strings takes no arguments (got `{arg}`)");
        return ExitCode::from(2);
    }
    let vault = std::env::var_os("EFTERSCRIPT_HELLBOX")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    match lint(&workspace_root(), vault.as_deref()) {
        Ok(Outcome::Skipped(message)) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Clean { files, strings }) => {
            println!("lint-strings: {files} files clean of {strings} listed strings");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Hits(hits)) => {
            for hit in &hits {
                println!("{}:{}: {}", hit.path, hit.line, hit.masked);
            }
            println!("lint-strings: {} hits", hits.len());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("lint-strings: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_denylist_ignores_comments_and_blank_lines() {
        assert_eq!(
            denylist("# names\n\n  SecretName \n\nother\n#tail\n"),
            ["SecretName", "other"]
        );
        assert!(denylist("").is_empty());
    }

    #[test]
    fn masks_keep_the_first_character_only() {
        assert_eq!(mask("SecretName"), "S*********");
        assert_eq!(mask("x"), "x");
        assert_eq!(mask(""), "");
    }

    #[test]
    fn a_planted_string_is_found_by_file_and_line_case_insensitively() {
        let dir = std::env::temp_dir().join(format!("efterscript-lint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(
            dir.join("a").join("b.rs"),
            "// nothing\n// the SECRETNAME appears here\nfn x() {}\n",
        )
        .unwrap();
        std::fs::write(dir.join("bin.dat"), b"secretname\0secretname").unwrap();
        std::fs::write(dir.join("clean.txt"), "secret name\n").unwrap();
        let files: Vec<PathBuf> = ["a/b.rs", "bin.dat", "clean.txt", "gone.txt"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let hits = scan_files(&dir, &files, &denylist("SecretName\nother\n")).unwrap();
        assert_eq!(
            hits,
            [Hit {
                path: "a/b.rs".to_string(),
                line: 2,
                masked: "S*********".to_string()
            }]
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(is_binary(b"a\0b"));
        assert!(!is_binary(b"plain"));
    }

    #[test]
    fn without_a_vault_or_a_denylist_the_lint_skips() {
        let root = workspace_root();
        assert_eq!(
            lint(&root, None).unwrap(),
            Outcome::Skipped("lint-strings skipped: no vault".to_string())
        );
        let empty = std::env::temp_dir().join(format!("efterscript-vault-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let outcome = lint(&root, Some(&empty)).unwrap();
        assert!(matches!(outcome, Outcome::Skipped(ref m) if m.contains("does not exist")));
        std::fs::remove_dir_all(&empty).unwrap();
    }

    #[test]
    fn tracked_files_include_this_source() {
        let files = tracked_files(&workspace_root()).unwrap();
        assert!(files.contains(&PathBuf::from("xtask/src/main.rs")));
        assert!(tracked_files(Path::new("/nonexistent/efterscript")).is_err());
    }

    /// The private tier's check: the repository carries none of the
    /// vault's listed strings. Skips with a message without the vault.
    #[test]
    fn the_repository_carries_no_listed_string() {
        let vault = std::env::var_os("EFTERSCRIPT_HELLBOX")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        match lint(&workspace_root(), vault.as_deref()).unwrap() {
            Outcome::Skipped(message) => eprintln!("{message}"),
            Outcome::Clean { .. } => {}
            Outcome::Hits(hits) => panic!(
                "{}",
                hits.iter()
                    .map(|h| format!("{}:{}: {}", h.path, h.line, h.masked))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }
}
