// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Statement-level delta debugging: ddmin over the statement list, then
//! single-statement removal until no statement can go, under a predicate
//! that says whether a candidate still fails.

use crate::program::Program;

/// Reduces `program` while `fails` holds; the predicate is trusted to
/// hold for the input. Returns the input unchanged if it does not.
pub fn shrink(program: &Program, fails: &mut dyn FnMut(&Program) -> bool) -> Program {
    let mut statements = program.statements.clone();
    if !fails(program) {
        return program.clone();
    }
    let mut n = 2;
    while statements.len() >= 2 {
        let len = statements.len();
        let chunk = len.div_ceil(n);
        let mut reduced = false;
        let mut start = 0;
        while start < len {
            let end = (start + chunk).min(len);
            let mut candidate = statements[..start].to_vec();
            candidate.extend_from_slice(&statements[end..]);
            if fails(&program.with_statements(candidate.clone())) {
                statements = candidate;
                n = (n - 1).max(2);
                reduced = true;
                break;
            }
            start = end;
        }
        if reduced {
            continue;
        }
        if n >= len {
            break;
        }
        n = (n * 2).min(len);
    }
    // Single removals to a fixed point.
    let mut i = 0;
    while i < statements.len() {
        let mut candidate = statements.clone();
        candidate.remove(i);
        if fails(&program.with_statements(candidate.clone())) {
            statements = candidate;
            i = 0;
        } else {
            i += 1;
        }
    }
    program.with_statements(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(statements: &[&str]) -> Program {
        Program {
            header: vec!["%!PS".to_string()],
            statements: statements.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_planted_statement_is_isolated() {
        let mut statements: Vec<String> = (0..50).map(|i| format!("{i} pop")).collect();
        statements.insert(23, "BAD".to_string());
        let p = Program {
            header: vec!["%!PS".to_string()],
            statements,
        };
        let mut calls = 0;
        let min = shrink(&p, &mut |q| {
            calls += 1;
            q.statements.iter().any(|s| s == "BAD")
        });
        assert_eq!(min.statements, ["BAD"]);
        assert_eq!(min.header, p.header);
        assert!(calls < 200, "{calls}");
    }

    #[test]
    fn dependent_statements_are_kept() {
        let p = program(&["a", "b", "c", "d", "e", "f"]);
        let min = shrink(&p, &mut |q| {
            q.statements.contains(&"b".to_string()) && q.statements.contains(&"e".to_string())
        });
        assert_eq!(min.statements, ["b", "e"]);
        let unchanged = shrink(&p, &mut |_| false);
        assert_eq!(unchanged, p);
        let none = shrink(&program(&["x", "y"]), &mut |_| true);
        assert!(none.statements.is_empty());
        let one = shrink(&program(&["x"]), &mut |q| q.statements.len() == 1);
        assert_eq!(one.statements, ["x"]);
    }
}
