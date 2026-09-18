// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The data files are what `PROVENANCE.md` says they are: every file's
//! SHA-256 matches its entry there — the fetched assets and the metric
//! tables derived from them alike — every entry names an existing file,
//! and each outline set's licence file sits beside its fonts with the
//! plain licence text under `LICENSES/`, as does the CMap directory's.
//! The files promoted from the
//! vault are also compared with the vault's `SHA256SUMS` when
//! `EFTERSCRIPT_HELLBOX` names an existing checkout; that part skips
//! with a message otherwise.

use std::path::{Path, PathBuf};

// SHA-256 (FIPS 180-4), written out here so the check needs no dependency.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while !(message.len() % 64 == 56) {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (k, word) in block.chunks(4).enumerate() {
            w[k] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for k in 16..64 {
            let s0 = w[k - 15].rotate_right(7) ^ w[k - 15].rotate_right(18) ^ (w[k - 15] >> 3);
            let s1 = w[k - 2].rotate_right(17) ^ w[k - 2].rotate_right(19) ^ (w[k - 2] >> 10);
            w[k] = w[k - 16]
                .wrapping_add(s0)
                .wrapping_add(w[k - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for k in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[k])
                .wrapping_add(w[k]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (k, word) in h.iter().enumerate() {
        out[k * 4..k * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn sha256_matches_a_known_vector() {
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// The files promoted from the vault: their path here and in the vault.
const VAULTED: &[(&str, &str)] = &[
    (
        "core14/Courier-Bold.afm",
        "fonts/metrics/core14/Courier-Bold.afm",
    ),
    (
        "core14/Courier-BoldOblique.afm",
        "fonts/metrics/core14/Courier-BoldOblique.afm",
    ),
    (
        "core14/Courier-Oblique.afm",
        "fonts/metrics/core14/Courier-Oblique.afm",
    ),
    ("core14/Courier.afm", "fonts/metrics/core14/Courier.afm"),
    (
        "core14/Helvetica-Bold.afm",
        "fonts/metrics/core14/Helvetica-Bold.afm",
    ),
    (
        "core14/Helvetica-BoldOblique.afm",
        "fonts/metrics/core14/Helvetica-BoldOblique.afm",
    ),
    (
        "core14/Helvetica-Oblique.afm",
        "fonts/metrics/core14/Helvetica-Oblique.afm",
    ),
    ("core14/Helvetica.afm", "fonts/metrics/core14/Helvetica.afm"),
    ("core14/LICENSE", "fonts/metrics/core14/LICENSE"),
    ("core14/Symbol.afm", "fonts/metrics/core14/Symbol.afm"),
    (
        "core14/Times-Bold.afm",
        "fonts/metrics/core14/Times-Bold.afm",
    ),
    (
        "core14/Times-BoldItalic.afm",
        "fonts/metrics/core14/Times-BoldItalic.afm",
    ),
    (
        "core14/Times-Italic.afm",
        "fonts/metrics/core14/Times-Italic.afm",
    ),
    (
        "core14/Times-Roman.afm",
        "fonts/metrics/core14/Times-Roman.afm",
    ),
    (
        "core14/ZapfDingbats.afm",
        "fonts/metrics/core14/ZapfDingbats.afm",
    ),
    ("glyphlist.txt", "fonts/glyphlists/glyphlist.txt"),
];

const LIBERATION: [&str; 12] = [
    "LiberationSans-Regular",
    "LiberationSans-Bold",
    "LiberationSans-Italic",
    "LiberationSans-BoldItalic",
    "LiberationSerif-Regular",
    "LiberationSerif-Bold",
    "LiberationSerif-Italic",
    "LiberationSerif-BoldItalic",
    "LiberationMono-Regular",
    "LiberationMono-Bold",
    "LiberationMono-Italic",
    "LiberationMono-BoldItalic",
];

const TEX_GYRE: [&str; 21] = [
    "qagr", "qagri", "qagb", "qagbi", "qbkr", "qbkri", "qbkb", "qbkbi", "qcsr", "qcsri", "qcsb",
    "qcsbi", "qplr", "qplri", "qplb", "qplbi", "qzcmi", "qhvcr", "qhvcri", "qhvcb", "qhvcbi",
];

const TEX_GYRE_MANIFESTS: [&str; 6] = ["Adventor", "Bonum", "Chorus", "Heros", "Pagella", "Schola"];

/// The outline assets fetched from upstream, by provenance path.
fn fetched() -> Vec<String> {
    let mut files = Vec::new();
    for stem in LIBERATION {
        files.push(format!("outlines/liberation/{stem}.ttf"));
    }
    files.push("outlines/liberation/LICENSE".to_string());
    for stem in TEX_GYRE {
        files.push(format!("outlines/tex-gyre/{stem}.pfb"));
    }
    files.push("outlines/tex-gyre/GUST-FONT-LICENSE.txt".to_string());
    for family in TEX_GYRE_MANIFESTS {
        files.push(format!("outlines/tex-gyre/MANIFEST-TeX-Gyre-{family}.txt"));
    }
    for name in ["Identity-H", "Identity-V", "LICENSE.md"] {
        files.push(format!("cmap/{name}"));
    }
    files.push("LICENSES/OFL-1.1.txt".to_string());
    files.push("LICENSES/LPPL-1.3c.txt".to_string());
    files.push("LICENSES/BSD-3-Clause.txt".to_string());
    files
}

/// The metric tables derived from the TeX Gyre programs at intake, by
/// provenance path.
fn derived() -> Vec<String> {
    TEX_GYRE
        .iter()
        .map(|stem| format!("outlines/tex-gyre/{stem}.metrics"))
        .collect()
}

/// Every file the provenance must list.
fn listed_files() -> Vec<String> {
    let mut files: Vec<String> = VAULTED.iter().map(|(local, _)| local.to_string()).collect();
    files.extend(fetched());
    files.extend(derived());
    files
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
}

/// A provenance path on disk: under the data directory, except the
/// licence texts, which live at the repository root.
fn located(path: &str) -> PathBuf {
    if path.starts_with("LICENSES/") {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    } else {
        data_dir().join(path)
    }
}

fn note() -> String {
    std::fs::read_to_string(data_dir().join("PROVENANCE.md")).unwrap()
}

#[test]
fn promoted_files_match_the_vault_checksums() {
    let Some(vault) = std::env::var_os("EFTERSCRIPT_HELLBOX") else {
        eprintln!("EFTERSCRIPT_HELLBOX is not set; skipping the vault checksum comparison");
        return;
    };
    let vault = Path::new(&vault);
    let Ok(sums) = std::fs::read_to_string(vault.join("SHA256SUMS")) else {
        eprintln!(
            "{} has no SHA256SUMS; skipping the vault checksum comparison",
            vault.display()
        );
        return;
    };
    for &(local, vaulted) in VAULTED {
        let expected = sums
            .lines()
            .find_map(|line| {
                let (sum, path) = line.split_once("  ")?;
                (path.trim() == vaulted).then(|| sum.to_string())
            })
            .unwrap_or_else(|| panic!("{vaulted} is not listed in the vault's SHA256SUMS"));
        let bytes = std::fs::read(located(local)).expect("promoted file exists");
        assert_eq!(
            hex(&sha256(&bytes)),
            expected,
            "{local} differs from the vault"
        );
        let original = std::fs::read(vault.join(vaulted)).expect("vault file exists");
        assert_eq!(
            bytes, original,
            "{local} is not byte-identical to the vault"
        );
    }
}

#[test]
fn provenance_note_lists_every_file_with_its_checksum() {
    let note = note();
    for local in &listed_files() {
        let bytes = std::fs::read(located(local)).unwrap_or_else(|e| panic!("{local}: {e}"));
        let sum = hex(&sha256(&bytes));
        assert!(
            note.contains(&format!("| `{local}` | `{sum}` |")),
            "{local} with {sum} is not recorded in PROVENANCE.md"
        );
    }
}

#[test]
fn every_provenance_entry_names_an_existing_file_and_nothing_is_unlisted() {
    let note = note();
    let mut entries = Vec::new();
    for line in note.lines() {
        let mut cells = line.split('|').map(str::trim).filter(|c| !c.is_empty());
        let Some(path) = cells
            .next()
            .and_then(|c| c.strip_prefix('`'))
            .and_then(|c| c.strip_suffix('`'))
        else {
            continue;
        };
        let Some(sum) = cells
            .next()
            .and_then(|c| c.strip_prefix('`'))
            .and_then(|c| c.strip_suffix('`'))
        else {
            continue;
        };
        if sum.len() != 64 {
            continue;
        }
        let bytes = std::fs::read(located(path))
            .unwrap_or_else(|e| panic!("PROVENANCE.md names {path}, which is unreadable: {e}"));
        assert_eq!(hex(&sha256(&bytes)), sum, "{path}");
        entries.push(path.to_string());
    }
    let mut expected = listed_files();
    expected.sort();
    entries.sort();
    assert_eq!(entries, expected);

    // Every file under the data directory is listed.
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.path().is_dir() {
                walk(&entry.path(), &path, out);
            } else if path != "PROVENANCE.md" {
                out.push(path);
            }
        }
    }
    let mut present = Vec::new();
    walk(&data_dir(), "", &mut present);
    present.sort();
    let mut data_entries: Vec<String> = expected
        .iter()
        .filter(|p| !p.starts_with("LICENSES/"))
        .cloned()
        .collect();
    data_entries.sort();
    assert_eq!(present, data_entries);
}

#[test]
fn licence_files_sit_beside_the_fonts_and_the_texts_are_in_licenses() {
    let liberation =
        std::fs::read_to_string(data_dir().join("outlines/liberation/LICENSE")).unwrap();
    assert!(liberation.contains("SIL OPEN FONT LICENSE Version 1.1"));
    assert!(liberation.contains("Reserved Font Name Liberation"));
    let ofl = std::fs::read_to_string(located("LICENSES/OFL-1.1.txt")).unwrap();
    assert!(ofl.starts_with("SIL OPEN FONT LICENSE"));
    assert!(ofl.contains("Version 1.1 - 26 February 2007"));
    // Liberation's copy re-wraps the licence text after its copyright
    // statements, so the two are compared clause by clause rather than
    // byte for byte.
    for clause in ["PERMISSION & CONDITIONS", "TERMINATION", "DISCLAIMER"] {
        assert!(
            liberation.contains(clause) && ofl.contains(clause),
            "{clause}"
        );
    }

    let gust = std::fs::read_to_string(data_dir().join("outlines/tex-gyre/GUST-FONT-LICENSE.txt"))
        .unwrap();
    assert!(gust.contains("GUST Font License"));
    assert!(gust.contains("LaTeX Project Public License"));
    assert!(gust.contains("version 1.3c"));
    let lppl = std::fs::read_to_string(located("LICENSES/LPPL-1.3c.txt")).unwrap();
    assert!(lppl.starts_with("The LaTeX Project Public License"));
    assert!(lppl.contains("LPPL Version 1.3c"));

    // The CMap resources carry Adobe's BSD-style notice, both in the
    // licence file beside them and in each file's own header.
    let cmap_licence = std::fs::read_to_string(data_dir().join("cmap/LICENSE.md")).unwrap();
    assert!(cmap_licence.starts_with("Copyright 1990-2023 Adobe."));
    let bsd = std::fs::read_to_string(located("LICENSES/BSD-3-Clause.txt")).unwrap();
    assert!(bsd.starts_with("Copyright (c) <year> <owner>"));
    for clause in [
        "Redistributions of source code must retain",
        "Redistributions in binary form must reproduce",
        "endorse or promote products",
    ] {
        assert!(
            cmap_licence.contains(clause) && bsd.contains(clause),
            "{clause}"
        );
    }
    for name in ["Identity-H", "Identity-V"] {
        let text = std::fs::read_to_string(data_dir().join(format!("cmap/{name}"))).unwrap();
        assert!(text.starts_with("%!PS-Adobe-3.0 Resource-CMap"), "{name}");
        assert!(
            text.contains("%%Copyright: Copyright 1990-2019 Adobe."),
            "{name}"
        );
        assert!(text.contains(&format!("/CMapName /{name} def")), "{name}");
        assert_eq!(
            efterscript_fonts::cmap::predefined(name.as_bytes()),
            Some(text.as_str())
        );
    }
    // Each family states its own version; the manifest beside the fonts
    // must be the one for the release the fonts are.
    for (family, stem) in [
        ("Adventor", "qagr"),
        ("Bonum", "qbkr"),
        ("Chorus", "qzcmi"),
        ("Heros", "qhvcr"),
        ("Pagella", "qplr"),
        ("Schola", "qcsr"),
    ] {
        let manifest = std::fs::read_to_string(
            data_dir().join(format!("outlines/tex-gyre/MANIFEST-TeX-Gyre-{family}.txt")),
        )
        .unwrap();
        assert!(manifest.contains(family), "{family}");
        let manifest_version = manifest
            .lines()
            .find_map(|l| l.strip_prefix("Version:"))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| panic!("{family}: no Version:"));
        let bytes =
            std::fs::read(data_dir().join(format!("outlines/tex-gyre/{stem}.pfb"))).unwrap();
        let font = efterscript_fonts::type1::parse_file(&bytes).unwrap();
        let font_version = font
            .program
            .dict()
            .font_info
            .iter()
            .find(|(key, _)| key == b"version")
            .map(|(_, value)| {
                String::from_utf8_lossy(value)
                    .trim_matches(|c| c == '(' || c == ')')
                    .to_string()
            })
            .unwrap_or_else(|| panic!("{family}: no FontInfo version"));
        assert_eq!(
            manifest_version, font_version,
            "{family} manifest names another release"
        );
    }
}
