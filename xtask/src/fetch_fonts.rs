// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask fetch-fonts [--check] [--force]`: the intake and audit
//! path for the resident set's outline assets and the shipped CMap
//! resources. Downloads the exact upstream releases (through the system
//! `curl`) into `target/fetch-fonts`, verifies the archives' SHA-256
//! against the constants below, extracts exactly the listed members
//! (system `tar` and `unzip`), derives the metric table of each Type 1
//! program, and compares each file with the committed one and with its
//! entry in `crates/ps-fonts/data/PROVENANCE.md`.
//! The committed files are the source of truth; this tool is the audit
//! trail.
//!
//! - `--check` reports without writing and fails unless every file is
//!   committed, listed, and identical to upstream (or, for a table, to
//!   what the program yields).
//! - Without it, files missing from the tree are written; a committed
//!   file that differs is refused unless `--force`, which overwrites it.
//!   Whenever something was written or a provenance entry is missing or
//!   stale, the table rows to paste are printed.
//! - `--test-assets` instead extracts the OpenType test font from the
//!   TeX Gyre release into `target/test-fonts/`, where the optional CFF
//!   test finds it; nothing is written into the repository.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use ps_fonts::metrics::MetricTable;
use ps_fonts::type1::parse_file;

use crate::sha256::hex_digest;

/// Where a repository file comes from.
struct Source {
    /// The archive member, or the whole download for a plain text.
    member: &'static str,
    /// The destination: relative to `crates/ps-fonts/data`, except for
    /// paths under `LICENSES/`, which are relative to the workspace root.
    dest: &'static str,
}

enum Kind {
    TarGz,
    Zip,
    /// A single text file, downloaded as it is.
    Text,
}

struct Upstream {
    file: &'static str,
    url: &'static str,
    /// The archive's checksum, recorded from the release it names.
    sha256: &'static str,
    kind: Kind,
    files: &'static [Source],
}

const TEX_GYRE_FACES: [&str; 21] = [
    "qagr", "qagri", "qagb", "qagbi", "qbkr", "qbkri", "qbkb", "qbkbi", "qcsr", "qcsri", "qcsb",
    "qcsbi", "qplr", "qplri", "qplb", "qplbi", "qzcmi", "qhvcr", "qhvcri", "qhvcb", "qhvcbi",
];
const TEX_GYRE_MANIFESTS: [&str; 6] = ["Adventor", "Bonum", "Chorus", "Heros", "Pagella", "Schola"];

macro_rules! src {
    ($member:expr, $dest:expr) => {
        Source {
            member: $member,
            dest: $dest,
        }
    };
}

const LIBERATION_FILES: [Source; 13] = [
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSans-Regular.ttf",
        "outlines/liberation/LiberationSans-Regular.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSans-Bold.ttf",
        "outlines/liberation/LiberationSans-Bold.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSans-Italic.ttf",
        "outlines/liberation/LiberationSans-Italic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSans-BoldItalic.ttf",
        "outlines/liberation/LiberationSans-BoldItalic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSerif-Regular.ttf",
        "outlines/liberation/LiberationSerif-Regular.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSerif-Bold.ttf",
        "outlines/liberation/LiberationSerif-Bold.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSerif-Italic.ttf",
        "outlines/liberation/LiberationSerif-Italic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationSerif-BoldItalic.ttf",
        "outlines/liberation/LiberationSerif-BoldItalic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationMono-Regular.ttf",
        "outlines/liberation/LiberationMono-Regular.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationMono-Bold.ttf",
        "outlines/liberation/LiberationMono-Bold.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationMono-Italic.ttf",
        "outlines/liberation/LiberationMono-Italic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LiberationMono-BoldItalic.ttf",
        "outlines/liberation/LiberationMono-BoldItalic.ttf"
    ),
    src!(
        "liberation-fonts-ttf-2.1.5/LICENSE",
        "outlines/liberation/LICENSE"
    ),
];

/// The commit of `adobe-type-tools/cmap-resources` the Identity CMaps
/// and their licence were taken from; nothing else is fetched from it.
const CMAP_RESOURCES_COMMIT: &str = "f5cf3bca7fdfeaceb77aa82847e974f2306c20b4";

const UPSTREAMS: [Upstream; 8] = [
    Upstream {
        file: "liberation-fonts-ttf-2.1.5.tar.gz",
        url: "https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz",
        sha256: "7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0",
        kind: Kind::TarGz,
        files: &LIBERATION_FILES,
    },
    Upstream {
        file: "tex-gyre.zip",
        url: "https://mirrors.ctan.org/fonts/tex-gyre.zip",
        sha256: "1773c470f9e388e087b68e3426e115af2cd236845a7e05ceb25b2a503409a7a3",
        kind: Kind::Zip,
        // The Type 1 programs, licence, and manifests are listed by
        // `tex_gyre_files`; the array form cannot be built in a const.
        files: &[],
    },
    Upstream {
        file: "OFL-1.1.txt",
        url: "https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/OFL-1.1.txt",
        sha256: "8eea8287e5876b539670cadb82e99f9a7afddec6f6730811be1daf25d2e9bcfd",
        kind: Kind::Text,
        files: &[src!("OFL-1.1.txt", "LICENSES/OFL-1.1.txt")],
    },
    Upstream {
        file: "LPPL-1.3c.txt",
        url: "https://www.latex-project.org/lppl/lppl-1-3c.txt",
        sha256: "3d262cdf34dafa6955f703c634a8c238ec44109bc8dd6ef34fb7aa54809f7e66",
        kind: Kind::Text,
        files: &[src!("LPPL-1.3c.txt", "LICENSES/LPPL-1.3c.txt")],
    },
    Upstream {
        file: "BSD-3-Clause.txt",
        url: "https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/BSD-3-Clause.txt",
        sha256: "5a93d5831e1297ab10fe643e1a631e83be392896da14ee2951285a79012df69d",
        kind: Kind::Text,
        files: &[src!("BSD-3-Clause.txt", "LICENSES/BSD-3-Clause.txt")],
    },
    Upstream {
        file: "Identity-H",
        url: "https://raw.githubusercontent.com/adobe-type-tools/cmap-resources/f5cf3bca7fdfeaceb77aa82847e974f2306c20b4/Adobe-Identity-0/CMap/Identity-H",
        sha256: "a06aff40c5e4393829d572b3771e5cafcf450ec4fa6ef3df5ae4024f16fc6efa",
        kind: Kind::Text,
        files: &[src!("Identity-H", "cmap/Identity-H")],
    },
    Upstream {
        file: "Identity-V",
        url: "https://raw.githubusercontent.com/adobe-type-tools/cmap-resources/f5cf3bca7fdfeaceb77aa82847e974f2306c20b4/Adobe-Identity-0/CMap/Identity-V",
        sha256: "c03430489caf73dc71c723d9ae0413a31132f6ac9f16149d9a1d19e5d913af0f",
        kind: Kind::Text,
        files: &[src!("Identity-V", "cmap/Identity-V")],
    },
    Upstream {
        file: "cmap-LICENSE.md",
        url: "https://raw.githubusercontent.com/adobe-type-tools/cmap-resources/f5cf3bca7fdfeaceb77aa82847e974f2306c20b4/LICENSE.md",
        sha256: "742665db9c8e1bc72603c6d319ca3e90b83bd6d95202f0cd4ef11068a07a9c29",
        kind: Kind::Text,
        files: &[src!("cmap-LICENSE.md", "cmap/LICENSE.md")],
    },
];

/// The OpenType file the optional CFF test reads, as an archive member
/// and as its name under `target/test-fonts`.
pub const TEST_OPENTYPE_MEMBER: &str = "tex-gyre/opentype/texgyrepagella-regular.otf";
pub const TEST_OPENTYPE_FILE: &str = "texgyrepagella-regular.otf";

/// The archive member holding a TeX Gyre face's Type 1 program.
fn tex_gyre_program(face: &str) -> String {
    format!("tex-gyre/type1/{face}.pfb")
}

/// The metric table derived from a face's program, by destination.
fn tex_gyre_table(face: &str) -> String {
    format!("outlines/tex-gyre/{face}.metrics")
}

/// The TeX Gyre members: `type1/<face>.pfb` for the twenty-one faces,
/// the licence, and the manifests of the six families used.
fn tex_gyre_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    for face in TEX_GYRE_FACES {
        files.push((
            tex_gyre_program(face),
            format!("outlines/tex-gyre/{face}.pfb"),
        ));
    }
    files.push((
        "tex-gyre/doc/GUST-FONT-LICENSE.txt".to_string(),
        "outlines/tex-gyre/GUST-FONT-LICENSE.txt".to_string(),
    ));
    for family in TEX_GYRE_MANIFESTS {
        files.push((
            format!("tex-gyre/doc/MANIFEST-TeX-Gyre-{family}.txt"),
            format!("outlines/tex-gyre/MANIFEST-TeX-Gyre-{family}.txt"),
        ));
    }
    files
}

fn files_of(upstream: &Upstream) -> Vec<(String, String)> {
    if upstream.file == "tex-gyre.zip" {
        tex_gyre_files()
    } else {
        upstream
            .files
            .iter()
            .map(|s| (s.member.to_string(), s.dest.to_string()))
            .collect()
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

/// The repository path of a destination (see [`Source::dest`]).
fn dest_path(root: &Path, dest: &str) -> PathBuf {
    if dest.starts_with("LICENSES/") {
        root.join(dest)
    } else {
        root.join("crates/ps-fonts/data").join(dest)
    }
}

/// The `| \`path\` | \`sha256\` |` rows of the provenance note.
fn provenance(root: &Path) -> BTreeMap<String, String> {
    let note = std::fs::read_to_string(root.join("crates/ps-fonts/data/PROVENANCE.md"))
        .unwrap_or_default();
    note.lines()
        .filter_map(|line| {
            let mut cells = line.split('|').map(str::trim).filter(|c| !c.is_empty());
            let path = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
            let sum = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
            (sum.len() == 64).then(|| (path.to_string(), sum.to_string()))
        })
        .collect()
}

fn run_tool(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed ({status})"))
    }
}

/// Downloads `upstream` into `dir` unless a copy with the right checksum
/// is already there; verifies the checksum either way.
fn download(upstream: &Upstream, dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(upstream.file);
    let cached = std::fs::read(&path)
        .ok()
        .is_some_and(|bytes| hex_digest(&bytes) == upstream.sha256);
    if cached {
        println!("cached    {}", upstream.file);
    } else {
        println!("download  {}", upstream.url);
        run_tool(
            "curl",
            &[
                "-sSL",
                "--fail",
                "-o",
                path.to_str().ok_or("path is not UTF-8")?,
                upstream.url,
            ],
        )?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let actual = hex_digest(&bytes);
        if actual != upstream.sha256 {
            return Err(format!(
                "{} has SHA-256 {actual}, expected {}",
                upstream.file, upstream.sha256
            ));
        }
    }
    println!("verified  {} sha256={}", upstream.file, upstream.sha256);
    Ok(path)
}

/// Extracts the listed members of `archive` into `dir`; a plain text is
/// its own single member.
fn extract(
    upstream: &Upstream,
    archive: &Path,
    dir: &Path,
    members: &[String],
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let archive = archive.to_str().ok_or("path is not UTF-8")?;
    let dir_str = dir.to_str().ok_or("path is not UTF-8")?;
    let members: Vec<&str> = members.iter().map(String::as_str).collect();
    match upstream.kind {
        Kind::TarGz => {
            let mut args = vec!["xzf", archive, "-C", dir_str];
            args.extend(members);
            run_tool("tar", &args)
        }
        Kind::Zip => {
            let mut args = vec!["-q", "-o", archive];
            args.extend(members);
            args.extend(["-d", dir_str]);
            run_tool("unzip", &args)
        }
        Kind::Text => std::fs::copy(archive, dir.join(upstream.file))
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", archive)),
    }
}

/// The metric table of the program in `bytes`, rendered as the file to
/// commit; every advance that had to be rounded is printed.
fn derive_table(face: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let source = format!("{face}.pfb");
    let font = parse_file(bytes).map_err(|e| format!("{source}: {e}"))?;
    let derived = MetricTable::derive(&font).map_err(|e| format!("{source}: {e}"))?;
    for (name, advance) in &derived.rounded {
        println!(
            "rounded   {}: /{name} {advance} to {}",
            tex_gyre_table(face),
            advance.round()
        );
    }
    Ok(derived.table.render(&source).into_bytes())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Committed, identical to upstream, listed with that checksum.
    Ok,
    /// Not in the tree.
    Missing,
    /// Committed but not identical to upstream.
    Differs,
    /// Identical to upstream but the provenance entry is absent or stale.
    Unlisted,
}

struct Audit {
    dest: String,
    status: Status,
    /// What the file should hold: the upstream member or the derived table.
    bytes: Vec<u8>,
    upstream: String,
    committed: Option<String>,
}

fn audit(root: &Path, listed: &BTreeMap<String, String>, dest: &str, bytes: Vec<u8>) -> Audit {
    let upstream = hex_digest(&bytes);
    let committed = std::fs::read(dest_path(root, dest))
        .ok()
        .map(|b| hex_digest(&b));
    let status = match &committed {
        None => Status::Missing,
        Some(c) if *c != upstream => Status::Differs,
        Some(_) if listed.get(dest) != Some(&upstream) => Status::Unlisted,
        Some(_) => Status::Ok,
    };
    Audit {
        dest: dest.to_string(),
        status,
        bytes,
        upstream,
        committed,
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let force = args.iter().any(|a| a == "--force");
    let test_assets = args.iter().any(|a| a == "--test-assets");
    if let Some(unknown) = args
        .iter()
        .find(|a| !matches!(a.as_str(), "--check" | "--force" | "--test-assets"))
    {
        eprintln!("fetch-fonts: unknown argument `{unknown}`");
        eprintln!("usage: cargo xtask fetch-fonts [--check] [--force] [--test-assets]");
        return ExitCode::from(2);
    }
    let result = if test_assets {
        fetch_test_assets().map(|()| true)
    } else {
        fetch(check, force)
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("fetch-fonts: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Extracts the OpenType test font into `target/test-fonts/`.
fn fetch_test_assets() -> Result<(), String> {
    let root = workspace_root();
    let work = root.join("target").join("fetch-fonts");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let upstream = UPSTREAMS
        .iter()
        .find(|u| u.file == "tex-gyre.zip")
        .expect("the TeX Gyre release is listed");
    let archive = download(upstream, &work)?;
    let extracted = work.join("extract");
    extract(
        upstream,
        &archive,
        &extracted,
        &[TEST_OPENTYPE_MEMBER.to_string()],
    )?;
    let dir = root.join("target").join("test-fonts");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dest = dir.join(TEST_OPENTYPE_FILE);
    std::fs::copy(extracted.join(TEST_OPENTYPE_MEMBER), &dest)
        .map_err(|e| format!("{TEST_OPENTYPE_MEMBER}: {e}"))?;
    println!("wrote     {}", dest.display());
    Ok(())
}

fn fetch(check: bool, force: bool) -> Result<bool, String> {
    let root = workspace_root();
    let work = root.join("target").join("fetch-fonts");
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let listed = provenance(&root);
    let extracted = work.join("extract");
    println!("cmap      adobe-type-tools/cmap-resources at {CMAP_RESOURCES_COMMIT}");
    let mut audits = Vec::new();
    for upstream in &UPSTREAMS {
        let archive = download(upstream, &work)?;
        let files = files_of(upstream);
        let members: Vec<String> = files.iter().map(|(m, _)| m.clone()).collect();
        extract(upstream, &archive, &extracted, &members)?;
        for (member, dest) in &files {
            let path = extracted.join(member);
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("{member} not extracted from {}: {e}", upstream.file))?;
            audits.push(audit(&root, &listed, dest, bytes));
        }
    }
    for face in TEX_GYRE_FACES {
        let member = tex_gyre_program(face);
        let bytes = std::fs::read(extracted.join(&member))
            .map_err(|e| format!("{member} not extracted: {e}"))?;
        let table = derive_table(face, &bytes)?;
        audits.push(audit(&root, &listed, &tex_gyre_table(face), table));
    }
    let mut rows = Vec::new();
    let mut failed = false;
    for a in &audits {
        let label = match a.status {
            Status::Ok => "ok",
            Status::Missing => "missing",
            Status::Differs => "differs",
            Status::Unlisted => "unlisted",
        };
        match (a.status, check, force) {
            (Status::Ok, _, _) => println!("{label:<9} {}", a.dest),
            (_, true, _) => {
                println!("{label:<9} {} (upstream {})", a.dest, a.upstream);
                failed = true;
            }
            (Status::Missing, false, _) | (Status::Differs, false, true) => {
                let path = dest_path(&root, &a.dest);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(&path, &a.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("wrote     {}", a.dest);
                rows.push(a);
            }
            (Status::Differs, false, false) => {
                println!(
                    "refused   {} (committed {}, upstream {}; pass --force to overwrite)",
                    a.dest,
                    a.committed.as_deref().unwrap_or("-"),
                    a.upstream
                );
                failed = true;
            }
            (Status::Unlisted, false, _) => {
                println!("{label:<9} {}", a.dest);
                rows.push(a);
            }
        }
    }
    if !rows.is_empty() {
        println!();
        println!("provenance rows for crates/ps-fonts/data/PROVENANCE.md:");
        for a in rows {
            println!("| `{}` | `{}` |", a.dest, a.upstream);
        }
    }
    Ok(!failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_destination_is_unique_and_the_tex_gyre_list_is_complete() {
        let mut all: Vec<(String, String)> = UPSTREAMS.iter().flat_map(files_of).collect();
        all.extend(TEX_GYRE_FACES.map(|face| (tex_gyre_program(face), tex_gyre_table(face))));
        let mut dests: Vec<&str> = all.iter().map(|(_, d)| d.as_str()).collect();
        dests.sort_unstable();
        let count = dests.len();
        dests.dedup();
        assert_eq!(dests.len(), count);
        assert_eq!(count, 13 + 21 * 2 + 1 + 6 + 3 + 3);
        assert!(
            all.iter()
                .any(|(m, d)| m == "Identity-V" && d == "cmap/Identity-V")
        );
        assert!(
            UPSTREAMS
                .iter()
                .filter(|u| u.url.contains("cmap-resources"))
                .all(|u| u.url.contains(CMAP_RESOURCES_COMMIT))
        );
        assert!(all.iter().any(|(m, d)| m == "tex-gyre/type1/qzcmi.pfb"
            && d == "outlines/tex-gyre/qzcmi.pfb"));
        assert_eq!(tex_gyre_table("qzcmi"), "outlines/tex-gyre/qzcmi.metrics");
        assert!(!all.iter().any(|(m, _)| m.ends_with(".afm")));
    }

    #[test]
    fn provenance_rows_are_read_from_the_table() {
        let root = workspace_root();
        let rows = provenance(&root);
        assert_eq!(
            rows.get("glyphlist.txt").map(String::as_str),
            Some("a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789")
        );
        assert!(dest_path(&root, "LICENSES/OFL-1.1.txt").ends_with("LICENSES/OFL-1.1.txt"));
        assert!(
            dest_path(&root, "outlines/liberation/LICENSE")
                .ends_with("crates/ps-fonts/data/outlines/liberation/LICENSE")
        );
    }

    #[test]
    fn a_derived_table_is_the_programs_own_metrics() {
        use ps_fonts::testing::{Type1Font, rectangle};
        let font = Type1Font::new("Syn").glyph("a", 600, &rectangle(0.0, 0.0, 500.0, 500.0));
        let table = derive_table("syn", &font.pfb()).unwrap();
        let text = String::from_utf8(table).unwrap();
        assert!(text.starts_with("metrics/1\n"));
        assert!(text.contains("from syn.pfb\n"));
        assert!(text.ends_with("w /.notdef 0\nw /a 600\n"));
        assert!(derive_table("bad", b"not a font").is_err());
    }
}
