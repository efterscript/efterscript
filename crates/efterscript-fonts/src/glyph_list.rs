// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Glyph names to Unicode, following the Adobe Glyph List specification's
//! mapping rules: the list itself, `uniXXXX` and `uXXXX[XX]` names,
//! ligatures joined with `_`, and a suffix after `.` that is dropped.

use std::collections::HashMap;
use std::sync::OnceLock;

fn list() -> &'static HashMap<&'static str, Vec<char>> {
    static LIST: OnceLock<HashMap<&'static str, Vec<char>>> = OnceLock::new();
    LIST.get_or_init(|| {
        let text = include_str!("../data/glyphlist.txt");
        let mut map = HashMap::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.starts_with('#') {
                continue;
            }
            let Some((name, codes)) = line.split_once(';') else {
                continue;
            };
            let chars: Option<Vec<char>> = codes
                .split_whitespace()
                .map(|hex| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
                .collect();
            if let Some(chars) = chars {
                map.insert(name, chars);
            }
        }
        map
    })
}

fn hex_chars(hex: &str, width: usize) -> Option<Vec<char>> {
    if hex.is_empty()
        || !hex.len().is_multiple_of(width)
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        return None;
    }
    hex.as_bytes()
        .chunks(width)
        .map(|group| {
            let value = u32::from_str_radix(std::str::from_utf8(group).ok()?, 16).ok()?;
            char::from_u32(value)
        })
        .collect()
}

fn component(name: &str) -> Option<Vec<char>> {
    if let Some(chars) = list().get(name) {
        return Some(chars.clone());
    }
    if let Some(hex) = name.strip_prefix("uni") {
        return hex_chars(hex, 4);
    }
    if let Some(hex) = name.strip_prefix('u')
        && (4..=6).contains(&hex.len())
    {
        return hex_chars(hex, hex.len());
    }
    None
}

/// The characters a glyph name denotes, or `None` when the name carries no
/// mapping (`.notdef`, private names, malformed `uni` forms).
pub fn unicode(name: &[u8]) -> Option<Vec<char>> {
    let name = std::str::from_utf8(name).ok()?;
    let stem = name.split('.').next().unwrap_or("");
    if stem.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for part in stem.split('_') {
        out.extend(component(part)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(name: &str) -> Option<String> {
        unicode(name.as_bytes()).map(|c| c.into_iter().collect())
    }

    #[test]
    fn list_entries() {
        assert_eq!(map("A"), Some("A".into()));
        assert_eq!(map("eacute"), Some("é".into()));
        assert_eq!(map("fi"), Some("ﬁ".into()));
        assert_eq!(map("alpha"), Some("α".into()));
        assert_eq!(map("space"), Some(" ".into()));
        assert_eq!(map("Euro"), Some("€".into()));
        assert!(list().len() > 4000);
    }

    #[test]
    fn derived_forms() {
        assert_eq!(map("uni0041"), Some("A".into()));
        assert_eq!(map("uni00410042"), Some("AB".into()));
        assert_eq!(map("u1F600"), Some("😀".into()));
        assert_eq!(map("u0041"), Some("A".into()));
        assert_eq!(map("A.sc"), Some("A".into()));
        assert_eq!(map("f_i"), Some("fi".into()));
        assert_eq!(map("uni0066_uni0069.alt"), Some("fi".into()));
    }

    #[test]
    fn unmapped_names() {
        assert_eq!(map(".notdef"), None);
        assert_eq!(map("nosuchglyph"), None);
        assert_eq!(map("uni004"), None);
        assert_eq!(map("uni00g1"), None);
        assert_eq!(map("uni0041 "), None);
        assert_eq!(map("u123"), None);
        assert_eq!(map("u1234567"), None);
        assert_eq!(map("uniD800"), None);
        assert_eq!(map("a61"), None);
        assert_eq!(map(""), None);
        assert_eq!(map("f_nosuch"), None);
        assert_eq!(unicode(&[0xFF]), None);
    }
}
