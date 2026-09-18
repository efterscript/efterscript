// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Canonical real syntax for the dump: the shortest plain-decimal form of
//! at most six significant digits that round-trips the `f32` (rounded to
//! six when none does, ties away from zero), trailing zeros and dot
//! trimmed, `-0` written as `0`, never an exponent, non-finite values
//! clamped. This is the same algorithm as the PDF writer's, kept separate
//! because the graphics layer must not depend on it; an equivalence test
//! in this crate's test suite holds the two together.

/// Formats `v` in the project's canonical real syntax.
pub fn fmt_real(v: f32) -> String {
    if v == 0.0 || v.is_nan() {
        return "0".to_string();
    }
    let v = v.clamp(f32::MIN, f32::MAX);

    let sci = format!("{:e}", v.abs());
    let (mant, exp) = sci.split_once('e').expect("float format has an exponent");
    let mut exp: i32 = exp.parse().expect("float exponent is an integer");
    let mut digits: Vec<u8> = mant.bytes().filter(|&b| b != b'.').collect();

    if digits.len() > 6 {
        let round_up = digits[6] >= b'5';
        digits.truncate(6);
        if round_up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, b'1');
                    digits.truncate(6);
                    exp += 1;
                    break;
                }
                i -= 1;
                if digits[i] == b'9' {
                    digits[i] = b'0';
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }

    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    let n = digits.len() as i32;
    let digits = std::str::from_utf8(&digits).expect("decimal digits are ASCII");
    if exp >= n - 1 {
        out.push_str(digits);
        for _ in 0..(exp - (n - 1)) {
            out.push('0');
        }
    } else if exp >= 0 {
        let split = (exp + 1) as usize;
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    } else {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(digits);
    }
    out
}

/// Space-separated reals.
pub fn fmt_reals(values: &[f32]) -> String {
    values
        .iter()
        .map(|&v| fmt_real(v))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_forms() {
        assert_eq!(fmt_real(1.0), "1");
        assert_eq!(fmt_real(0.5), "0.5");
        assert_eq!(fmt_real(-0.0), "0");
        assert_eq!(fmt_real(72.09), "72.09");
        assert_eq!(fmt_real(0.0001), "0.0001");
        assert_eq!(fmt_real(-1.5), "-1.5");
        assert_eq!(fmt_real(612.0), "612");
        assert_eq!(fmt_real(f32::NAN), "0");
        assert_eq!(fmt_real(0.1234567), "0.123457");
        assert_eq!(fmt_real(9_999_999.0), "10000000");
        assert!(!fmt_real(f32::INFINITY).contains(['e', 'E', 'i', 'n']));
        assert_eq!(fmt_reals(&[1.0, 0.25, -3.0]), "1 0.25 -3");
    }
}
