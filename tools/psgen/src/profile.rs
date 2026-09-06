// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Profiles: which operator groups a program draws on and how large it
//! may grow. Two exist, `core` (the language without graphics) and
//! `graphics` (a page's worth of drawing over a small core).

/// Size bounds of a profile. Numeric literals are integers in
/// `-int_max..=int_max` and reals with two decimals below `int_max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// Top-level statements per program, inclusive range.
    pub statements: (usize, usize),
    /// Statements per nested block, inclusive range.
    pub block: (usize, usize),
    /// Maximum nesting depth of compound statements.
    pub depth: usize,
    /// Largest `repeat` count and largest number of `for` iterations.
    pub loop_count: i64,
    pub int_max: i64,
    /// Longest string literal.
    pub string_len: usize,
    /// Longest array literal and `array` operand.
    pub array_len: usize,
    /// Most inputs a generated procedure declares.
    pub proc_inputs: usize,
    /// Statements per procedure body, inclusive range.
    pub proc_body: (usize, usize),
    /// The model stops pushing beyond this many operands.
    pub max_stack: usize,
    /// Largest coordinate a graphics literal uses.
    pub coord_max: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Core,
    Graphics,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    pub kind: Kind,
    pub bounds: Bounds,
    /// The ill-typed share, in thousandths of the eligible statements.
    pub ill_typed: u32,
    /// The execution budget the runner gives a program.
    pub budget: u64,
}

pub const CORE: Profile = Profile {
    name: "core",
    kind: Kind::Core,
    bounds: Bounds {
        statements: (12, 40),
        block: (1, 5),
        depth: 3,
        loop_count: 5,
        int_max: 1000,
        string_len: 8,
        array_len: 6,
        proc_inputs: 2,
        proc_body: (1, 5),
        max_stack: 60,
        coord_max: 0,
    },
    ill_typed: 50,
    budget: 200_000,
};

pub const GRAPHICS: Profile = Profile {
    name: "graphics",
    kind: Kind::Graphics,
    bounds: Bounds {
        statements: (15, 50),
        block: (1, 5),
        depth: 3,
        loop_count: 4,
        int_max: 1000,
        string_len: 8,
        array_len: 6,
        proc_inputs: 2,
        proc_body: (1, 4),
        max_stack: 60,
        coord_max: 500,
    },
    ill_typed: 50,
    budget: 200_000,
};

pub const ALL: [&Profile; 2] = [&CORE, &GRAPHICS];

impl Profile {
    pub fn named(name: &str) -> Option<&'static Profile> {
        ALL.into_iter().find(|p| p.name == name)
    }

    /// A copy with another ill-typed share, in thousandths.
    pub fn with_ill_typed(&self, per_mille: u32) -> Profile {
        Profile {
            ill_typed: per_mille,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_found_by_name() {
        assert_eq!(Profile::named("core"), Some(&CORE));
        assert_eq!(
            Profile::named("graphics").map(|p| p.kind),
            Some(Kind::Graphics)
        );
        assert_eq!(Profile::named("images"), None);
        assert_eq!(CORE.with_ill_typed(0).ill_typed, 0);
        assert_eq!(CORE.with_ill_typed(0).bounds, CORE.bounds);
    }
}
