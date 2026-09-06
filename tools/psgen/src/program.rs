// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A generated program as the properties and the shrinker see it: a
//! comment header and one statement per line. The `% psgen:` header line
//! names the profile, seed, and index a program came from; a hand-written
//! program has none and is identified by its file name instead.

use std::fmt;

/// The comment lines every generated program starts with.
pub const LICENSE_LINES: [&str; 2] = [
    "% SPDX-FileCopyrightText: 2026 EfterScript contributors",
    "% SPDX-License-Identifier: MIT",
];

/// Where a program came from, as its header records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub profile: String,
    pub seed: u64,
    pub index: u64,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "profile={} seed={} index={}",
            self.profile, self.seed, self.index
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The leading `%` lines, `%!PS` included, verbatim.
    pub header: Vec<String>,
    /// One statement per line, in order.
    pub statements: Vec<String>,
}

impl Program {
    /// Splits text into its comment header and its statements: the
    /// header is every leading `%` line; after it, each non-blank line is
    /// one statement (a later comment line is kept as a statement so
    /// nothing is lost).
    pub fn parse(text: &str) -> Program {
        let mut header = Vec::new();
        let mut statements = Vec::new();
        let mut in_header = true;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if in_header && line.starts_with('%') {
                header.push(line.to_string());
                continue;
            }
            in_header = false;
            if !line.trim().is_empty() {
                statements.push(line.to_string());
            }
        }
        Program { header, statements }
    }

    /// The program's text, one line per header entry and statement.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in self.header.iter().chain(&self.statements) {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// The origin the `% psgen:` header records, if any.
    pub fn origin(&self) -> Option<Origin> {
        self.header.iter().find_map(|line| parse_origin(line))
    }

    /// A copy with the given statements and the same header.
    pub fn with_statements(&self, statements: Vec<String>) -> Program {
        Program {
            header: self.header.clone(),
            statements,
        }
    }
}

/// The `% psgen: profile=<p> seed=<n> index=<i>` line, or none.
fn parse_origin(line: &str) -> Option<Origin> {
    let rest = line.strip_prefix("% psgen:")?.trim();
    if rest.starts_with("shrunk") {
        return None;
    }
    let mut profile = None;
    let mut seed = None;
    let mut index = None;
    for field in rest.split_whitespace() {
        let (key, value) = field.split_once('=')?;
        match key {
            "profile" => profile = Some(value.to_string()),
            "seed" => seed = value.parse().ok(),
            "index" => index = value.parse().ok(),
            _ => {}
        }
    }
    Some(Origin {
        profile: profile?,
        seed: seed?,
        index: index?,
    })
}

/// The header of a generated program.
pub fn header(origin: &Origin) -> Vec<String> {
    let mut lines = vec!["%!PS".to_string()];
    lines.extend(LICENSE_LINES.iter().map(|l| l.to_string()));
    lines.push(format!("% psgen: {origin}"));
    lines
}

/// The last token of a statement, which names its operator for the
/// statements the relations classify.
pub fn last_token(statement: &str) -> &str {
    statement.split_whitespace().next_back().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_round_trip() {
        let text = "%!PS\n% SPDX-License-Identifier: MIT\n% psgen: profile=core seed=5 index=2\n1 2 add =\n\n  \npstack\n";
        let program = Program::parse(text);
        assert_eq!(program.header.len(), 3);
        assert_eq!(program.statements, ["1 2 add =", "pstack"]);
        assert_eq!(
            program.origin(),
            Some(Origin {
                profile: "core".to_string(),
                seed: 5,
                index: 2
            })
        );
        assert_eq!(
            program.render(),
            "%!PS\n% SPDX-License-Identifier: MIT\n% psgen: profile=core seed=5 index=2\n1 2 add =\npstack\n"
        );
        let plain = Program::parse("1 =\n% late comment\n2 =\n");
        assert!(plain.header.is_empty());
        assert_eq!(plain.statements.len(), 3);
        assert_eq!(plain.origin(), None);
        assert_eq!(
            Program::parse("% psgen: shrunk from profile=core seed=1 index=2 property=x\n1")
                .origin(),
            None
        );
    }

    #[test]
    fn headers_name_their_origin() {
        let origin = Origin {
            profile: "graphics".to_string(),
            seed: 9,
            index: 0,
        };
        let lines = header(&origin);
        assert_eq!(lines[0], "%!PS");
        assert_eq!(lines[1], LICENSE_LINES[0]);
        assert_eq!(lines[3], "% psgen: profile=graphics seed=9 index=0");
        assert_eq!(last_token("0 0 10 10 rectfill"), "rectfill");
        assert_eq!(last_token(""), "");
    }
}
