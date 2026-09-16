// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `rand`, `srand`, `rrand`, and the two clocks `usertime` and `realtime`
//! (PLRM3 §8.2).
//!
//! The generator is a mixed linear congruential recurrence modulo 2^31
//! with parameters of the project's own. The modulus is a power of two,
//! so the recurrence has the full period 2^31 exactly when the increment
//! is odd and the multiplier is one more than a multiple of four
//! (Hull–Dobell); the multiplier is further chosen five more than a
//! multiple of eight, so the recurrence's potency is as high as the
//! modulus allows, and it lies well inside the modulus. The low bits of
//! such a recurrence cycle quickly (bit k has period 2^(k+1)), so the
//! value handed to the program is the state folded once with its own
//! high half, an invertible mixing that gives every output bit at least
//! the period of bit 15; the fold keeps the output a permutation of the
//! states, hence uniform over 0 to 2^31 − 1 across a period. `rrand`
//! returns the raw state and `srand` installs one, so the sequence
//! after `srand` of an `rrand` value continues where it was read.
//!
//! `usertime` is derived from the interpreter's step counter, which is
//! deterministic; `realtime` asks the embedder's clock and falls back to
//! `usertime` without one.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::Object;

op_table! { OPS {
    "rand" => rand;
    "srand" => srand, [Int];
    "rrand" => rrand;
    "usertime" => usertime;
    "realtime" => realtime;
}}

/// The state and the results are 31-bit non-negative integers.
pub(crate) const STATE_MASK: u32 = 0x7FFF_FFFF;

/// One more than a multiple of four (full period) and five more than a
/// multiple of eight (potency); about 0.46 of the modulus.
const MULTIPLIER: u32 = 0x3A47_0C25;
/// Odd, as the full period requires.
const INCREMENT: u32 = 0x1B4F_2C63;
/// The state a fresh interpreter starts from, so a program that never
/// calls `srand` sees the same sequence in every run.
pub(crate) const INITIAL_STATE: i32 = 0x1D2B_4A99;

/// Interpreter steps per millisecond of `usertime`: a release build of
/// this interpreter executes about this many objects per millisecond
/// on the development machine (a loop of `pop`s and dictionary stores
/// measured at some 10 600), so the clock ticks at roughly the rate of
/// one counting real milliseconds there; it is a fixed scale, not a
/// measurement, and the same in every build.
const STEPS_PER_MS: u64 = 10_000;

/// The next state of the recurrence, modulo 2^`bits`. Tests reduce the
/// modulus to walk a whole period; the operators use all 31 bits.
pub(crate) fn step(state: u32, bits: u32) -> u32 {
    let next = MULTIPLIER.wrapping_mul(state).wrapping_add(INCREMENT);
    next & ((1u32 << bits) - 1)
}

/// The value `rand` returns for a state.
pub(crate) fn output(state: u32) -> i32 {
    ((state ^ (state >> 15)) & STATE_MASK) as i32
}

/// The `usertime` reading for a step count: milliseconds of execution,
/// wrapping to the most negative integer past the largest.
pub(crate) fn usertime_of(steps: u64) -> i32 {
    // Truncating to 32 bits is the wrap the entry describes.
    ((steps / STEPS_PER_MS) as u32) as i32
}

fn rand(i: &mut Interp) -> Result<(), VmError> {
    let state = step(i.random_state() as u32, 31);
    i.set_random_state(state as i32);
    i.push(Object::integer(output(state)))
}

fn srand(i: &mut Interp) -> Result<(), VmError> {
    let seed = i.pop()?.as_i32().expect("integer");
    i.set_random_state((seed as u32 & STATE_MASK) as i32);
    Ok(())
}

fn rrand(i: &mut Interp) -> Result<(), VmError> {
    let state = i.random_state();
    i.push(Object::integer(state))
}

fn usertime(i: &mut Interp) -> Result<(), VmError> {
    let now = usertime_of(i.steps());
    i.push(Object::integer(now))
}

fn realtime(i: &mut Interp) -> Result<(), VmError> {
    let now = match i.clock_mut() {
        Some(clock) => clock.realtime_ms(),
        None => usertime_of(i.steps()),
    };
    i.push(Object::integer(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parameters_satisfy_the_full_period_conditions() {
        assert_eq!(INCREMENT % 2, 1);
        assert_eq!(MULTIPLIER % 4, 1);
        assert_eq!(MULTIPLIER % 8, 5);
        assert_eq!(INITIAL_STATE as u32 & !STATE_MASK, 0);
    }

    /// The recurrence visits every residue before returning to its
    /// start, for two reduced moduli; the conditions that give the full
    /// period modulo 2^k are the same for every k, and the parameters
    /// pass them for 31.
    #[test]
    fn the_recurrence_has_the_full_period_on_reduced_moduli() {
        for bits in [12, 20] {
            let modulus = 1u64 << bits;
            let mut state = 0u32;
            let mut count = 0u64;
            loop {
                state = step(state, bits);
                count += 1;
                if state == 0 {
                    break;
                }
                assert!(count <= modulus, "cycle longer than the modulus");
            }
            assert_eq!(count, modulus, "{bits} bits");
        }
    }

    #[test]
    fn outputs_stay_in_range_and_the_low_bits_are_not_degenerate() {
        let mut state = INITIAL_STATE as u32;
        let mut low_bit = Vec::with_capacity(64);
        let mut buckets = [0u32; 16];
        let draws = 65_536;
        for n in 0..draws {
            state = step(state, 31);
            let value = output(state);
            assert!(value >= 0);
            if n < 64 {
                low_bit.push(value & 1);
            }
            buckets[(value & 15) as usize] += 1;
        }
        // The raw recurrence would alternate its lowest bit; the fold
        // must break that pattern.
        assert!(
            low_bit.windows(2).any(|w| w[0] == w[1]),
            "the low bit alternates: {low_bit:?}"
        );
        // Every low nibble appears, and none is far from its share.
        let expected = draws / 16;
        for (nibble, &count) in buckets.iter().enumerate() {
            let deviation = (count as i64 - expected as i64).unsigned_abs();
            assert!(
                deviation * 20 < expected as u64,
                "nibble {nibble}: {count} of {draws}"
            );
        }
    }

    #[test]
    fn successive_outputs_are_weakly_correlated() {
        let mut state = INITIAL_STATE as u32;
        let draws = 100_000;
        let scale = f64::from(STATE_MASK);
        let mut previous = None;
        let (mut sum_x, mut sum_y, mut sum_xy, mut sum_xx, mut sum_yy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for _ in 0..draws {
            state = step(state, 31);
            let y = f64::from(output(state)) / scale;
            if let Some(x) = previous {
                sum_x += x;
                sum_y += y;
                sum_xy += x * y;
                sum_xx += x * x;
                sum_yy += y * y;
            }
            previous = Some(y);
        }
        let n = f64::from(draws - 1);
        let cov = sum_xy / n - (sum_x / n) * (sum_y / n);
        let var_x = sum_xx / n - (sum_x / n).powi(2);
        let var_y = sum_yy / n - (sum_y / n).powi(2);
        let correlation = cov / (var_x * var_y).sqrt();
        assert!(correlation.abs() < 0.02, "lag-1 correlation {correlation}");
    }

    #[test]
    fn usertime_counts_milliseconds_and_wraps_to_the_most_negative_integer() {
        assert_eq!(usertime_of(0), 0);
        assert_eq!(usertime_of(STEPS_PER_MS - 1), 0);
        assert_eq!(usertime_of(STEPS_PER_MS), 1);
        assert_eq!(usertime_of(STEPS_PER_MS * 1_000), 1_000);
        let largest = STEPS_PER_MS * u64::try_from(i32::MAX).expect("positive");
        assert_eq!(usertime_of(largest), i32::MAX);
        assert_eq!(usertime_of(largest + STEPS_PER_MS), i32::MIN);
        assert_eq!(usertime_of(largest + 2 * STEPS_PER_MS), i32::MIN + 1);
    }
}
