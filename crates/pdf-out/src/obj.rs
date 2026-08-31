// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Serialization of the basic object types (ISO 32000-1 §7.3): one canonical
//! byte form per value, written immediately into the current object's buffer.
//!
//! Chosen forms: reals carry a leading zero (`0.5`, not `.5`); `#xx` name
//! escapes and hexadecimal strings use uppercase digits; tokens are separated
//! by single spaces, delimiters included.

use std::io::Write as _;

/// An indirect object identifier: id handed out by `Document::alloc`,
/// generation always 0. Only allocation can create one, which is what makes
/// every written reference resolvable at `finish`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ref(pub(crate) u32);

impl Ref {
    pub fn id(self) -> u32 {
        self.0
    }
}

/// Single space before every token except the first in the buffer. All
/// delimiters are themselves tokens, so spacing never depends on context.
fn sep(buf: &mut Vec<u8>) {
    if !buf.is_empty() {
        buf.push(b' ');
    }
}

fn tok(buf: &mut Vec<u8>, t: &[u8]) {
    sep(buf);
    buf.extend_from_slice(t);
}

fn put_int(buf: &mut Vec<u8>, v: i64) {
    sep(buf);
    let _ = write!(buf, "{v}");
}

fn put_real(buf: &mut Vec<u8>, v: f32) {
    sep(buf);
    buf.extend_from_slice(fmt_real(v).as_bytes());
}

fn put_null(buf: &mut Vec<u8>) {
    tok(buf, b"null");
}

fn put_bool(buf: &mut Vec<u8>, v: bool) {
    tok(buf, if v { b"true".as_slice() } else { b"false" });
}

fn put_ref(buf: &mut Vec<u8>, r: Ref) {
    sep(buf);
    let _ = write!(buf, "{} 0 R", r.0);
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// A byte is written verbatim in a name iff it is a regular character:
/// in `!`..=`~` and neither a delimiter nor `#` (per ISO 32000-1 §7.3.5).
/// NUL cannot occur in a name at all — `put_name` reports it so the write
/// fails instead of emitting `#00`, which no reader could take back to a
/// name.
fn name_byte_is_plain(b: u8) -> bool {
    (b'!'..=b'~').contains(&b)
        && !matches!(
            b,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
        )
}

/// Returns false iff the name contains NUL and therefore has no PDF form.
#[must_use]
fn put_name(buf: &mut Vec<u8>, name: &[u8]) -> bool {
    sep(buf);
    buf.push(b'/');
    let mut ok = true;
    for &b in name {
        if b == 0 {
            ok = false;
        } else if name_byte_is_plain(b) {
            buf.push(b);
        } else {
            buf.push(b'#');
            buf.push(HEX[usize::from(b >> 4)]);
            buf.push(HEX[usize::from(b & 0xF)]);
        }
    }
    ok
}

/// Literal form iff every byte is printable ASCII; hexadecimal otherwise.
/// The choice is a property of the bytes, so serialization is deterministic.
fn put_string(buf: &mut Vec<u8>, s: &[u8]) {
    sep(buf);
    if s.iter().all(|b| (0x20..=0x7E).contains(b)) {
        buf.push(b'(');
        for &b in s {
            // Both parentheses are escaped unconditionally so balance never
            // has to be computed.
            if matches!(b, b'(' | b')' | b'\\') {
                buf.push(b'\\');
            }
            buf.push(b);
        }
        buf.push(b')');
    } else {
        buf.push(b'<');
        for &b in s {
            buf.push(HEX[usize::from(b >> 4)]);
            buf.push(HEX[usize::from(b & 0xF)]);
        }
        buf.push(b'>');
    }
}

/// Canonical real syntax: the shortest plain-decimal form of at most six
/// significant digits that round-trips the `f32` (rounded to six when none
/// does, ties away from zero), trailing zeros and dot trimmed, `-0` written
/// as `0`, never an exponent (the syntax has none, per ISO 32000-1 §7.3.3).
///
/// The digits come from Rust's shortest-round-trip float formatting, which is
/// pure code — identical on every platform — so output is byte-deterministic.
pub(crate) fn fmt_real(v: f32) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    // PDF has no syntax for non-finite reals; clamp rather than emit garbage.
    let v = if v.is_nan() {
        return "0".to_string();
    } else {
        v.clamp(f32::MIN, f32::MAX)
    };

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

/// Sink for exactly one value. Every method consumes the sink; dropping one
/// unused writes `null`, so a key or object body can never be left dangling —
/// misuse degrades to valid output instead of a panic or a malformed file.
pub struct Val<'a> {
    buf: &'a mut Vec<u8>,
    bad_name: &'a mut bool,
    written: bool,
}

impl<'a> Val<'a> {
    pub(crate) fn new(buf: &'a mut Vec<u8>, bad_name: &'a mut bool) -> Self {
        Val {
            buf,
            bad_name,
            written: false,
        }
    }

    pub fn int(mut self, v: i64) {
        self.written = true;
        put_int(self.buf, v);
    }

    pub fn real(mut self, v: f32) {
        self.written = true;
        put_real(self.buf, v);
    }

    pub fn boolean(mut self, v: bool) {
        self.written = true;
        put_bool(self.buf, v);
    }

    pub fn null(mut self) {
        self.written = true;
        put_null(self.buf);
    }

    pub fn name(self, n: &str) {
        self.name_bytes(n.as_bytes());
    }

    pub fn name_bytes(mut self, n: &[u8]) {
        self.written = true;
        if !put_name(self.buf, n) {
            *self.bad_name = true;
        }
    }

    pub fn string(mut self, s: &[u8]) {
        self.written = true;
        put_string(self.buf, s);
    }

    pub fn reference(mut self, r: Ref) {
        self.written = true;
        put_ref(self.buf, r);
    }

    pub fn dict(mut self, f: impl FnOnce(&mut DictBuilder<'_>)) {
        self.written = true;
        tok(self.buf, b"<<");
        let mut d = DictBuilder {
            buf: &mut *self.buf,
            bad_name: &mut *self.bad_name,
        };
        f(&mut d);
        tok(self.buf, b">>");
    }

    pub fn array(mut self, f: impl FnOnce(&mut ArrayBuilder<'_>)) {
        self.written = true;
        tok(self.buf, b"[");
        let mut a = ArrayBuilder {
            buf: &mut *self.buf,
            bad_name: &mut *self.bad_name,
        };
        f(&mut a);
        tok(self.buf, b"]");
    }
}

impl Drop for Val<'_> {
    fn drop(&mut self) {
        if !self.written {
            put_null(self.buf);
        }
    }
}

/// Writes dictionary entries in call order; `<<` and `>>` are emitted by the
/// enclosing [`Val::dict`], so an unclosed dictionary cannot be expressed.
pub struct DictBuilder<'a> {
    buf: &'a mut Vec<u8>,
    bad_name: &'a mut bool,
}

impl DictBuilder<'_> {
    pub(crate) fn new<'a>(buf: &'a mut Vec<u8>, bad_name: &'a mut bool) -> DictBuilder<'a> {
        DictBuilder { buf, bad_name }
    }

    pub fn key(&mut self, name: &str) -> Val<'_> {
        self.key_bytes(name.as_bytes())
    }

    pub fn key_bytes(&mut self, name: &[u8]) -> Val<'_> {
        if !put_name(self.buf, name) {
            *self.bad_name = true;
        }
        Val::new(self.buf, self.bad_name)
    }
}

/// Writes array elements in call order; `[` and `]` are emitted by the
/// enclosing [`Val::array`], so an unclosed array cannot be expressed.
pub struct ArrayBuilder<'a> {
    buf: &'a mut Vec<u8>,
    bad_name: &'a mut bool,
}

impl ArrayBuilder<'_> {
    pub fn item(&mut self) -> Val<'_> {
        Val::new(self.buf, self.bad_name)
    }

    pub fn int(&mut self, v: i64) -> &mut Self {
        self.item().int(v);
        self
    }

    pub fn real(&mut self, v: f32) -> &mut Self {
        self.item().real(v);
        self
    }

    pub fn boolean(&mut self, v: bool) -> &mut Self {
        self.item().boolean(v);
        self
    }

    pub fn null(&mut self) -> &mut Self {
        self.item().null();
        self
    }

    pub fn name(&mut self, n: &str) -> &mut Self {
        self.item().name(n);
        self
    }

    pub fn string(&mut self, s: &[u8]) -> &mut Self {
        self.item().string(s);
        self
    }

    pub fn reference(&mut self, r: Ref) -> &mut Self {
        self.item().reference(r);
        self
    }

    pub fn dict(&mut self, f: impl FnOnce(&mut DictBuilder<'_>)) -> &mut Self {
        self.item().dict(f);
        self
    }

    pub fn array(&mut self, f: impl FnOnce(&mut ArrayBuilder<'_>)) -> &mut Self {
        self.item().array(f);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::prelude::*;

    fn value(f: impl FnOnce(Val<'_>)) -> String {
        let mut buf = Vec::new();
        let mut bad = false;
        f(Val::new(&mut buf, &mut bad));
        assert!(!bad, "value helper used with a NUL-bearing name");
        String::from_utf8(buf).expect("serialized values are ASCII")
    }

    #[test]
    fn real_scenario_values() {
        assert_eq!(fmt_real(1.0), "1");
        assert_eq!(fmt_real(0.5), "0.5");
        assert_eq!(fmt_real(-0.0), "0");
        assert_eq!(fmt_real(72.09), "72.09");
        assert_eq!(fmt_real(0.0001), "0.0001");
    }

    #[test]
    fn real_edges() {
        assert_eq!(fmt_real(-1.5), "-1.5");
        assert_eq!(fmt_real(612.0), "612");
        assert_eq!(fmt_real(f32::NAN), "0");
        assert!(!fmt_real(f32::INFINITY).contains(['e', 'E', 'i', 'n']));
        assert!(!fmt_real(f32::MIN_POSITIVE).contains(['e', 'E']));
        // Seven shortest digits force rounding to six.
        assert_eq!(fmt_real(0.1234567), "0.123457");
        assert_eq!(fmt_real(9_999_999.0), "10000000");
    }

    #[test]
    fn integers_booleans_null() {
        assert_eq!(value(|v| v.int(0)), "0");
        assert_eq!(value(|v| v.int(-42)), "-42");
        assert_eq!(value(|v| v.boolean(true)), "true");
        assert_eq!(value(|v| v.boolean(false)), "false");
        assert_eq!(value(|v| v.null()), "null");
    }

    #[test]
    fn name_escaping_scenario() {
        assert_eq!(value(|v| v.name("A B#/x")), "/A#20B#23#2Fx");
    }

    #[test]
    fn name_escapes_nonprintable_and_delimiters() {
        assert_eq!(value(|v| v.name_bytes(b"a(b)c%")), "/a#28b#29c#25");
        assert_eq!(value(|v| v.name_bytes(b"\x07\xFF")), "/#07#FF");
        assert_eq!(value(|v| v.name("Type")), "/Type");
    }

    #[test]
    fn string_scenario() {
        assert_eq!(value(|v| v.string(b"abc(1)")), r"(abc\(1\))");
        assert_eq!(value(|v| v.string(b"\xFF\x00")), "<FF00>");
        assert_eq!(value(|v| v.string(b"")), "()");
        assert_eq!(value(|v| v.string(b"a\\b")), r"(a\\b)");
        assert_eq!(value(|v| v.string(b"line\n")), "<6C696E650A>");
    }

    #[test]
    fn containers_and_spacing() {
        let got = value(|v| {
            v.dict(|d| {
                d.key("Type").name("Page");
                d.key("Kids").array(|a| {
                    a.reference(Ref(3)).int(7);
                });
                d.key("Empty").dict(|_| {});
            });
        });
        assert_eq!(got, "<< /Type /Page /Kids [ 3 0 R 7 ] /Empty << >> >>");
    }

    #[test]
    fn dropped_sink_writes_null() {
        let got = value(|v| {
            v.dict(|d| {
                let _ = d.key("Unset");
            });
        });
        assert_eq!(got, "<< /Unset null >>");
    }

    proptest! {
        #[test]
        fn real_never_uses_exponent(v in proptest::num::f32::ANY) {
            let s = fmt_real(v);
            prop_assert!(!s.contains(['e', 'E']), "{s}");
        }

        #[test]
        fn real_is_canonical(v in proptest::num::f32::ANY) {
            let s = fmt_real(v);
            prop_assert!(s == "0" || !s.ends_with('0') || !s.contains('.'));
            prop_assert!(!s.ends_with('.'));
            prop_assert_ne!(s.as_str(), "-0");
            // Stable under re-parsing: the canonical form of the parsed-back
            // value is the same string.
            let back: f32 = s.parse().unwrap();
            prop_assert_eq!(fmt_real(back), s);
        }

        #[test]
        fn real_parses_back_within_six_digits(v in -1.0e6f32..1.0e6) {
            let back: f32 = fmt_real(v).parse().unwrap();
            let tol = 1.0e-5 * f64::from(v.abs()).max(1.0e-30);
            prop_assert!((f64::from(back) - f64::from(v)).abs() <= tol);
        }

        #[test]
        fn six_digit_decimals_round_trip_exactly(
            sig in 1i64..=999_999,
            scale in -6i32..=6,
            neg: bool,
        ) {
            // A value born from a decimal of at most six significant digits
            // must serialize to a form that parses back to the same f32.
            let text = format!("{}{sig}e{scale}", if neg { "-" } else { "" });
            let v: f32 = text.parse().unwrap();
            let back: f32 = fmt_real(v).parse().unwrap();
            prop_assert_eq!(back.to_bits(), v.to_bits());
        }
    }
}
