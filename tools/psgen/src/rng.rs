// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The generator's own random source: xorshift64, seeded from a seed and
//! a program index, integer arithmetic only, so a program's text depends
//! on nothing but those two numbers.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A source for program `index` of run `seed`. The two are mixed
    /// through a multiplicative hash so neighbouring indices do not share
    /// prefixes of their streams.
    pub fn new(seed: u64, index: u64) -> Self {
        let mut state = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(index.wrapping_mul(0xBF58_476D_1CE4_E5B9))
            .wrapping_add(0x94D0_49BB_1331_11EB);
        if state == 0 {
            state = 0x2545_F491_4F6C_DD1D;
        }
        let mut rng = Rng { state };
        for _ in 0..4 {
            rng.next_u64();
        }
        rng
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// A value in `0..n`; `n` must be positive.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "empty range");
        (self.next_u64() % n as u64) as usize
    }

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo <= hi, "empty range");
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as i64
    }

    /// True with probability `share`, given in thousandths.
    pub fn chance(&mut self, per_mille: u32) -> bool {
        (self.next_u64() % 1000) < u64::from(per_mille)
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_reproducible_and_seed_sensitive() {
        let a: Vec<u64> = (0..8).map(|_| Rng::new(7, 3).next_u64()).collect();
        let b: Vec<u64> = (0..8).map(|_| Rng::new(7, 3).next_u64()).collect();
        assert_eq!(a, b);
        let mut x = Rng::new(7, 3);
        let mut y = Rng::new(7, 4);
        let mut z = Rng::new(8, 3);
        let first: Vec<u64> = (0..4).map(|_| x.next_u64()).collect();
        assert_ne!(first, (0..4).map(|_| y.next_u64()).collect::<Vec<_>>());
        assert_ne!(first, (0..4).map(|_| z.next_u64()).collect::<Vec<_>>());
        assert!(Rng::new(0, 0).next_u64() != 0);
    }

    #[test]
    fn ranges_stay_inside_their_bounds() {
        let mut rng = Rng::new(1, 1);
        for _ in 0..1000 {
            let v = rng.range(-5, 5);
            assert!((-5..=5).contains(&v));
            assert!(rng.below(3) < 3);
        }
        assert_eq!(rng.range(4, 4), 4);
        assert!(!rng.chance(0));
        assert!(rng.chance(1000));
    }
}
