// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The browser deliverables, both assembled under `target/` from the
//! tracked sources and one WebAssembly build of the session library.
//!
//! `cargo xtask npm-package` builds the session library for
//! `wasm32-unknown-unknown` as a `cdylib` (its C interface is the
//! module's export list; it imports nothing) and assembles the npm
//! package in `target/npm/efterscript/`: the wrapper, its declarations,
//! the README, the manifest with the workspace version stamped in, the
//! module, and the repository's licence, third-party notices, and the
//! licence texts the bundled font data requires.
//!
//! `cargo xtask site` assembles the try-it page in `target/site/`: the
//! page's own files from `site/` and the package beside them under
//! `efterscript/`, so the page runs the very files that are published.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub const TARGET: &str = "wasm32-unknown-unknown";

/// The package's tracked files, copied as they are.
const PACKAGE_FILES: &[&str] = &["efterscript.js", "efterscript.d.ts", "README.md"];

/// The licence texts that ship with the module: the project's and those
/// of the font data compiled into it.
const LICENCE_TEXTS: &[&str] = &[
    "MIT.txt",
    "OFL-1.1.txt",
    "LPPL-1.3c.txt",
    "BSD-3-Clause.txt",
    "LicenseRef-Adobe-AFM.txt",
];

/// The version the tracked manifest carries; replaced on assembly.
const MANIFEST_PLACEHOLDER: &str = "\"version\": \"0.0.0\"";

pub fn npm_package(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask npm-package");
        return ExitCode::from(2);
    }
    match assemble_package() {
        Ok(dir) => {
            println!("npm-package: assembled {}", dir.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("npm-package: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn site(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask site");
        return ExitCode::from(2);
    }
    let assembled = assemble_package().and_then(|package| assemble_site(&package));
    match assembled {
        Ok(dir) => {
            println!("site: assembled {}", dir.display());
            println!(
                "site: serve it with any static server, e.g. `python3 -m http.server -d {}`",
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("site: {e}");
            ExitCode::FAILURE
        }
    }
}

fn assemble_package() -> Result<PathBuf, String> {
    let root = workspace_root();
    let wasm = build_module(&root)?;
    let source = root.join("npm/efterscript");
    let out = target_dir(&root).join("npm/efterscript");
    recreate(&out)?;

    for name in PACKAGE_FILES {
        copy(&source.join(name), &out.join(name))?;
    }
    copy(&root.join("LICENSE"), &out.join("LICENSE"))?;
    copy(
        &root.join("THIRD-PARTY-NOTICES.md"),
        &out.join("THIRD-PARTY-NOTICES.md"),
    )?;
    fs::create_dir_all(out.join("LICENSES")).map_err(|e| format!("LICENSES: {e}"))?;
    for name in LICENCE_TEXTS {
        copy(
            &root.join("LICENSES").join(name),
            &out.join("LICENSES").join(name),
        )?;
    }
    copy(&wasm, &out.join("efterscript.wasm"))?;

    let manifest_path = source.join("package.json");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    if manifest.matches(MANIFEST_PLACEHOLDER).count() != 1 {
        return Err(format!(
            "{} must carry the version as {MANIFEST_PLACEHOLDER} exactly once",
            manifest_path.display()
        ));
    }
    let version = env!("CARGO_PKG_VERSION");
    let stamped = manifest.replace(MANIFEST_PLACEHOLDER, &format!("\"version\": \"{version}\""));
    fs::write(out.join("package.json"), stamped).map_err(|e| format!("package.json: {e}"))?;
    Ok(out)
}

fn assemble_site(package: &Path) -> Result<PathBuf, String> {
    let root = workspace_root();
    let out = target_dir(&root).join("site");
    recreate(&out)?;
    copy_tree(&root.join("site"), &out)?;
    copy_tree(package, &out.join("efterscript"))?;
    // The page is served as plain files; this keeps the host from
    // running its own site generator over them.
    fs::write(out.join(".nojekyll"), "").map_err(|e| format!(".nojekyll: {e}"))?;
    Ok(out)
}

/// Builds the session library as a WebAssembly module and returns its
/// path.
fn build_module(root: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .current_dir(root)
        .args([
            "rustc",
            "--release",
            "-p",
            "efterscript-platen",
            "--target",
            TARGET,
            "--crate-type",
            "cdylib",
        ])
        // Symbol names are a third of a megabyte nobody downloading the
        // page needs.
        .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!(
            "the WebAssembly build failed; is the target installed? `rustup target add {TARGET}`"
        ));
    }
    let wasm = target_dir(root).join(TARGET).join("release/platen.wasm");
    if !wasm.is_file() {
        return Err(format!("{} was not produced", wasm.display()));
    }
    Ok(wasm)
}

fn recreate(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &dest)?;
        } else {
            copy(&path, &dest)?;
        }
    }
    Ok(())
}

fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}
