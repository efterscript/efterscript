// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The scanner scenarios, each naming the `corpus/unit/scanner/*.ps` file it
//! mirrors, plus the property tests: chunked scanning agrees with one-shot
//! scanning at every split point, and generated token streams survive a
//! serialize-scan round trip.

use std::collections::HashMap;

use proptest::prelude::*;
use ps_vm::{
    Atom, ChunkSource, FileSource, MAX_PROC_DEPTH, MAX_STRING_LEN, Memory, Object, Scan, ScanError,
    ScanErrorKind, Scanner, SliceSource, Space, Span, Stream, StringSource, Type, VmError,
    scan_all,
};

// --- helpers ---------------------------------------------------------------

struct Bytes(Vec<u8>, usize);

impl Stream for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        let n = buf.len().min(self.0.len() - self.1);
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }
    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Ok(0)
    }
}

fn objects(m: &mut Memory, input: &[u8]) -> Vec<Object> {
    scan_all(input, m, &mut ())
        .unwrap()
        .into_iter()
        .map(|(o, _)| o)
        .collect()
}

fn error(m: &mut Memory, input: &[u8]) -> ScanError {
    scan_all(input, m, &mut ()).unwrap_err()
}

fn name_text(m: &Memory, o: Object) -> Vec<u8> {
    m.name_text(o.as_name().expect("a name")).to_vec()
}

fn string_text(m: &Memory, o: Object) -> Vec<u8> {
    assert_eq!(o.ty(), Type::String);
    m.string(o).unwrap().to_vec()
}

/// A structural picture of an object, for comparing token streams that were
/// produced by different scans (handles differ; contents must not).
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Int(i32),
    Real(u32),
    Name(Vec<u8>, bool),
    Str(Vec<u8>),
    Proc(Vec<Shape>),
    Other,
}

fn shape(m: &Memory, o: Object) -> Shape {
    match o.ty() {
        Type::Integer => Shape::Int(o.as_i32().unwrap()),
        Type::Real => Shape::Real(o.as_f32().unwrap().to_bits()),
        Type::Name => Shape::Name(name_text(m, o), o.is_executable()),
        Type::String => Shape::Str(string_text(m, o)),
        Type::Array => {
            assert!(o.is_executable());
            Shape::Proc(m.array(o).unwrap().iter().map(|&e| shape(m, e)).collect())
        }
        _ => Shape::Other,
    }
}

fn shapes(m: &mut Memory, input: &[u8]) -> Result<Vec<Shape>, ScanErrorKind> {
    let tokens = scan_all(input, m, &mut ()).map_err(|e| e.kind)?;
    Ok(tokens.iter().map(|&(o, _)| shape(m, o)).collect())
}

// --- token classes -----------------------------------------------------------

// numbers.ps
#[test]
fn numbers() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"123 -7 16#ff 8#17 2#101 1.5 .5 1. -1e3 1E-3");
    let ints: Vec<_> = t[..5].iter().map(|o| o.as_i32().unwrap()).collect();
    assert_eq!(ints, [123, -7, 255, 15, 5]);
    let reals: Vec<_> = t[5..].iter().map(|o| o.as_f32().unwrap()).collect();
    assert_eq!(reals, [1.5, 0.5, 1.0, -1000.0, 0.001]);
    assert!(t.iter().all(|o| o.is_literal()));
}

// number-like-names.ps
#[test]
fn number_like_names() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"123abc - . 1e 16# 37#1");
    assert_eq!(t.len(), 6);
    for o in &t {
        assert_eq!(o.ty(), Type::Name);
        assert!(o.is_executable());
    }
    assert_eq!(name_text(&m, t[0]), b"123abc");
    assert_eq!(name_text(&m, t[5]), b"37#1");
}

// integer-overflow-real.ps
#[test]
fn integer_overflow_becomes_real() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"2147483648 -2147483648 -2147483649 16#ffffffff");
    assert_eq!(t[0].as_f32(), Some(2147483648.0));
    assert_eq!(t[1].as_i32(), Some(i32::MIN));
    assert_eq!(t[2].as_f32(), Some(-2147483649.0));
    assert_eq!(t[3].as_i32(), Some(-1));
}

// names-and-attributes.ps
#[test]
fn names_and_attributes() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"abc /abc [ ] << >>");
    assert_eq!(t.len(), 6);
    assert!(t[0].is_executable());
    assert!(t[1].is_literal());
    assert_eq!(t[0].as_name(), t[1].as_name());
    assert_eq!(name_text(&m, t[0]), b"abc");
    let rest: Vec<_> = t[2..].iter().map(|&o| name_text(&m, o)).collect();
    assert_eq!(
        rest,
        [b"[".to_vec(), b"]".to_vec(), b"<<".to_vec(), b">>".to_vec()]
    );
    assert!(t[2..].iter().all(|o| o.is_executable()));
}

// names-delimit.ps
#[test]
fn names_end_at_delimiters_and_may_be_empty() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"a[b]c{d}/e(f)/ /g%h\n");
    let texts: Vec<_> = t.iter().map(|&o| shape(&m, o)).collect();
    assert_eq!(
        texts,
        [
            Shape::Name(b"a".to_vec(), true),
            Shape::Name(b"[".to_vec(), true),
            Shape::Name(b"b".to_vec(), true),
            Shape::Name(b"]".to_vec(), true),
            Shape::Name(b"c".to_vec(), true),
            Shape::Proc(vec![Shape::Name(b"d".to_vec(), true)]),
            Shape::Name(b"e".to_vec(), false),
            Shape::Str(b"f".to_vec()),
            Shape::Name(b"".to_vec(), false),
            Shape::Name(b"g".to_vec(), false),
        ]
    );
    let e = error(&mut m, b"//");
    assert_eq!(e.kind, ScanErrorKind::Undefined);
    let mut m = Memory::new();
    let long = vec![b'x'; 128];
    assert_eq!(error(&mut m, &long).kind, ScanErrorKind::LimitCheck);
    let mut lit = b"/".to_vec();
    lit.extend_from_slice(&long);
    assert_eq!(error(&mut m, &lit).kind, ScanErrorKind::LimitCheck);
    let ok = vec![b'x'; 127];
    assert_eq!(objects(&mut m, &ok).len(), 1);
}

// strings.ps
#[test]
fn strings() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"(a(b)c) (\\101\\n) <41 4> <~87cURD]i,\"Ebo80~>");
    assert_eq!(t.len(), 4);
    assert_eq!(string_text(&m, t[0]), b"a(b)c");
    assert_eq!(string_text(&m, t[1]), b"A\n");
    assert_eq!(string_text(&m, t[2]), b"A@");
    assert_eq!(string_text(&m, t[3]), b"Hello World!");
    assert!(t.iter().all(|o| o.is_literal()));
}

// string-escapes.ps
#[test]
fn string_escapes() {
    let mut m = Memory::new();
    let t = objects(
        &mut m,
        b"(\\n\\r\\t\\b\\f\\\\\\(\\)) (\\0\\12\\101x\\1234) (\\q\\ ) (a\\\nb\\\r\nc\\\rd) (x\ny\r\nz) (\\400)",
    );
    assert_eq!(string_text(&m, t[0]), b"\n\r\t\x08\x0c\\()");
    assert_eq!(string_text(&m, t[1]), b"\0\n\x41x\x534");
    assert_eq!(string_text(&m, t[2]), b"q ");
    assert_eq!(string_text(&m, t[3]), b"abcd");
    assert_eq!(string_text(&m, t[4]), b"x\ny\r\nz");
    assert_eq!(string_text(&m, t[5]), b"\0");
    let empty = objects(&mut m, b"()")[0];
    assert_eq!(string_text(&m, empty), b"");
    let nested = objects(&mut m, b"(()())")[0];
    assert_eq!(string_text(&m, nested), b"()()");
}

// hex-strings.ps
#[test]
fn hex_strings() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"<> <4> <414243> <4\n1 4\t2 4 3> <aAfF>");
    assert_eq!(string_text(&m, t[0]), b"");
    assert_eq!(string_text(&m, t[1]), b"\x40");
    assert_eq!(string_text(&m, t[2]), b"ABC");
    assert_eq!(string_text(&m, t[3]), b"ABC");
    assert_eq!(string_text(&m, t[4]), b"\xaa\xff");
    assert_eq!(error(&mut m, b"<4g>").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<41").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<").kind, ScanErrorKind::SyntaxError);
}

// ascii85-strings.ps
#[test]
fn ascii85_strings() {
    let mut m = Memory::new();
    let t = objects(
        &mut m,
        b"<~~> <~z~> <~87cURDZ~> <~87 cU\nRD]i~> <~!!~> <~s8W-!~>",
    );
    assert_eq!(string_text(&m, t[0]), b"");
    assert_eq!(string_text(&m, t[1]), b"\0\0\0\0");
    assert_eq!(string_text(&m, t[2]), b"Hello");
    assert_eq!(string_text(&m, t[3]), b"Hello ");
    assert_eq!(string_text(&m, t[4]), b"\0");
    assert_eq!(string_text(&m, t[5]), b"\xff\xff\xff\xff");
    assert_eq!(
        error(&mut m, b"<~s8W-\"~>").kind,
        ScanErrorKind::SyntaxError
    );
    assert_eq!(error(&mut m, b"<~a~>").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<~v~>").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<~az~>").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<~~x").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<~87cU").kind, ScanErrorKind::SyntaxError);
    assert_eq!(error(&mut m, b"<~87cU~").kind, ScanErrorKind::SyntaxError);
}

// string-length-limit.ps
#[test]
fn string_length_limit() {
    let mut m = Memory::new();
    let mut ok = vec![b'('];
    ok.extend(std::iter::repeat_n(b'x', MAX_STRING_LEN));
    ok.push(b')');
    assert_eq!(
        objects(&mut m, &ok)[0].length(),
        Some(MAX_STRING_LEN as u32)
    );
    let mut long = vec![b'('];
    long.extend(std::iter::repeat_n(b'x', MAX_STRING_LEN + 1));
    long.push(b')');
    assert_eq!(error(&mut m, &long).kind, ScanErrorKind::LimitCheck);
}

// procedures.ps
#[test]
fn procedures() {
    let mut m = Memory::new();
    let t = objects(&mut m, b"{1 {2} add}");
    assert_eq!(t.len(), 1);
    let outer = t[0];
    assert_eq!(outer.ty(), Type::Array);
    assert!(outer.is_executable());
    let items = m.array(outer).unwrap().to_vec();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_i32(), Some(1));
    let inner = items[1];
    assert_eq!(inner.ty(), Type::Array);
    assert!(inner.is_executable());
    let inner_items = m.array(inner).unwrap();
    assert_eq!(inner_items.len(), 1);
    assert_eq!(inner_items[0].as_i32(), Some(2));
    assert_eq!(name_text(&m, items[2]), b"add");
    let empty = objects(&mut m, b"{}")[0];
    assert!(m.array(empty).unwrap().is_empty());
    assert_eq!(
        shapes(&mut m, b"{}{(a)}").unwrap(),
        [
            Shape::Proc(vec![]),
            Shape::Proc(vec![Shape::Str(b"a".to_vec())])
        ]
    );
}

// procedure-depth-limit.ps
#[test]
fn procedure_depth_limit() {
    let mut m = Memory::new();
    let mut deep = vec![b'{'; MAX_PROC_DEPTH];
    deep.extend(vec![b'}'; MAX_PROC_DEPTH]);
    assert_eq!(objects(&mut m, &deep).len(), 1);
    let mut too_deep = vec![b'{'; MAX_PROC_DEPTH + 1];
    too_deep.extend(vec![b'}'; MAX_PROC_DEPTH + 1]);
    let e = error(&mut m, &too_deep);
    assert_eq!(e.kind, ScanErrorKind::LimitCheck);
    assert_eq!(e.span, Span::new(MAX_PROC_DEPTH, MAX_PROC_DEPTH + 1));
}

// --- exact position ------------------------------------------------------

fn one(scanner: &mut Scanner, source: &mut SliceSource, m: &mut Memory) -> (Object, Span) {
    match scanner.next(source, m, &mut ()).unwrap() {
        Scan::Token { object, span } => (object, span),
        other => panic!("expected a token, got {other:?}"),
    }
}

// Covered in Rust only: the corpus cannot hold the raw bytes meaningfully.
#[test]
fn binary_data_after_an_operator_name() {
    let mut m = Memory::new();
    let input = b"eexec\r\n\x80\x01";
    let mut source = SliceSource::new(input);
    let mut scanner = Scanner::new();
    let (o, span) = one(&mut scanner, &mut source, &mut m);
    assert_eq!(name_text(&m, o), b"eexec");
    assert_eq!(span, Span::new(0, 5));
    assert_eq!(source.remaining(), b"\n\x80\x01");
}

// delimiter-left-in-place.ps
#[test]
fn delimiter_left_in_place() {
    let mut m = Memory::new();
    let input = b"abc(x)";
    let mut source = SliceSource::new(input);
    let mut scanner = Scanner::new();
    let (o, span) = one(&mut scanner, &mut source, &mut m);
    assert_eq!(name_text(&m, o), b"abc");
    assert_eq!(span, Span::new(0, 3));
    assert_eq!(source.remaining(), b"(x)");
    let (s, span) = one(&mut scanner, &mut source, &mut m);
    assert_eq!(string_text(&m, s), b"x");
    assert_eq!(span, Span::new(3, 6));
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::End)
    ));
}

#[test]
fn self_delimiting_tokens_consume_no_trailing_whitespace() {
    let mut m = Memory::new();
    for (input, rest) in [
        (&b"(a) x"[..], &b" x"[..]),
        (b"<41> x", b" x"),
        (b"<~~> x", b" x"),
        (b"{1} x", b" x"),
        (b"[ x", b" x"),
        (b"] x", b" x"),
        (b"<< x", b" x"),
        (b">> x", b" x"),
        (b"/a x", b"x"),
        (b"12 x", b"x"),
        (b"12\rx", b"x"),
        (b"12\n\nx", b"\nx"),
        (b"12\x00x", b"x"),
        (b"12\x0cx", b"x"),
        (b"12%c\nx", b"%c\nx"),
        (b"12/x", b"/x"),
        (b"a{", b"{"),
    ] {
        let mut source = SliceSource::new(input);
        let mut scanner = Scanner::new();
        one(&mut scanner, &mut source, &mut m);
        assert_eq!(source.remaining(), rest, "after {input:?}");
    }
}

#[test]
fn spans_cover_the_token_only() {
    let mut m = Memory::new();
    let input = b"  12 /ab (c)\n{ 1 }<41>\t<<>>";
    let spans: Vec<_> = scan_all(input, &mut m, &mut ())
        .unwrap()
        .into_iter()
        .map(|(_, s)| (s.start, s.end))
        .collect();
    assert_eq!(
        spans,
        [
            (2, 4),
            (5, 8),
            (9, 12),
            (13, 18),
            (18, 22),
            (23, 25),
            (25, 27)
        ]
    );
}

#[test]
fn file_source_leaves_the_file_positioned_after_the_token() {
    let mut m = Memory::new();
    let f = m.open_stream(Box::new(Bytes(b"12 abc(raw)".to_vec(), 0)));
    let mut source = FileSource::new(f).unwrap();
    let mut scanner = Scanner::new();
    let a = match scanner.next(&mut source, &mut m, &mut ()).unwrap() {
        Scan::Token { object, span } => {
            assert_eq!(span, Span::new(0, 2));
            object
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(a.as_i32(), Some(12));
    let b = match scanner.next(&mut source, &mut m, &mut ()).unwrap() {
        Scan::Token { object, span } => {
            assert_eq!(span, Span::new(3, 6));
            object
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(name_text(&m, b), b"abc");
    let mut buf = [0u8; 8];
    assert_eq!(m.file_read(f, &mut buf), Ok(5));
    assert_eq!(&buf[..5], b"(raw)");
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::End)
    ));
    m.close_file(f).unwrap();
    let e = scanner.next(&mut source, &mut m, &mut ()).unwrap_err();
    assert_eq!(e.kind, ScanErrorKind::Vm(VmError::IoError));
}

#[test]
fn string_source_supports_token_on_strings() {
    let mut m = Memory::new();
    let s = m.alloc_string(b" 1 (a) b".to_vec());
    let mut source = StringSource::new(s).unwrap();
    let mut scanner = Scanner::new();
    let t = scanner.next(&mut source, &mut m, &mut ()).unwrap();
    assert!(matches!(t, Scan::Token { object, .. } if object.as_i32() == Some(1)));
    assert_eq!(m.string(source.remainder()), Some(&b"(a) b"[..]));
    let t = scanner.next(&mut source, &mut m, &mut ()).unwrap();
    assert!(matches!(t, Scan::Token { object, .. } if object.ty() == Type::String));
    assert_eq!(m.string(source.remainder()), Some(&b" b"[..]));
    scanner.next(&mut source, &mut m, &mut ()).unwrap();
    assert_eq!(m.string(source.remainder()), Some(&b""[..]));
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::End)
    ));
}

// --- incremental input -----------------------------------------------------

fn chunked(m: &mut Memory, chunks: &[&[u8]]) -> Vec<Result<Scan, ScanError>> {
    let mut source = ChunkSource::new();
    let mut scanner = Scanner::new();
    let mut out = Vec::new();
    for chunk in chunks {
        source.append(chunk);
        loop {
            match scanner.next(&mut source, m, &mut ()) {
                Ok(Scan::NeedMore) => {
                    out.push(Ok(Scan::NeedMore));
                    break;
                }
                Ok(Scan::End) => unreachable!("End before finish"),
                other => out.push(other),
            }
        }
    }
    source.finish();
    loop {
        match scanner.next(&mut source, m, &mut ()) {
            Ok(Scan::End) => break,
            Ok(Scan::NeedMore) => unreachable!("NeedMore after finish"),
            other => {
                let stop = other.is_err();
                out.push(other);
                if stop {
                    break;
                }
            }
        }
    }
    out
}

// split-inside-string.ps
#[test]
fn split_inside_a_string() {
    let mut m = Memory::new();
    let out = chunked(&mut m, &[b"(hel", b"lo world)"]);
    assert!(matches!(out[0], Ok(Scan::NeedMore)));
    match out[1] {
        Ok(Scan::Token { object, span }) => {
            assert_eq!(string_text(&m, object), b"hello world");
            assert_eq!(span, Span::new(0, 13));
        }
        ref other => panic!("{other:?}"),
    }
    assert!(matches!(out[2], Ok(Scan::NeedMore)));
    assert_eq!(out.len(), 3);
}

#[test]
fn chunk_source_holds_only_the_unread_tail() {
    let mut m = Memory::new();
    let mut source = ChunkSource::new();
    let mut scanner = Scanner::new();
    source.append(b"12 34 (ab");
    let mut count = 0;
    while let Scan::Token { .. } = scanner.next(&mut source, &mut m, &mut ()).unwrap() {
        count += 1;
    }
    assert_eq!(count, 2);
    assert!(scanner.is_mid_token());
    source.append(b"c)");
    assert_eq!(source.buffered(), 2);
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::Token { .. })
    ));
    assert!(!scanner.is_mid_token());
    source.finish();
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::End)
    ));
}

#[test]
fn end_inside_procedure_is_syntaxerror_only_when_no_more_may_come() {
    let mut m = Memory::new();
    let mut source = ChunkSource::new();
    let mut scanner = Scanner::new();
    source.append(b"{ 1 ");
    assert!(matches!(
        scanner.next(&mut source, &mut m, &mut ()),
        Ok(Scan::NeedMore)
    ));
    assert_eq!(scanner.depth(), 1);
    source.finish();
    let e = scanner.next(&mut source, &mut m, &mut ()).unwrap_err();
    assert_eq!(e.kind, ScanErrorKind::SyntaxError);
    assert_eq!(e.span, Span::new(0, 4));
    assert_eq!(scanner.depth(), 0);
}

// --- errors ------------------------------------------------------------------

// unmatched-close-brace.ps
#[test]
fn unmatched_close_brace() {
    let mut m = Memory::new();
    let e = error(&mut m, b"}");
    assert_eq!(e.kind, ScanErrorKind::SyntaxError);
    assert_eq!(e.kind.name(), "syntaxerror");
    assert_eq!(e.span, Span::new(0, 1));
    assert_eq!(e.byte, Some(b'}'));
    let e = error(&mut m, b"1 2 }");
    assert_eq!(e.span, Span::new(4, 5));
}

// unterminated-string.ps
#[test]
fn unterminated_string_at_end_of_input() {
    let mut m = Memory::new();
    let e = error(&mut m, b"(abc");
    assert_eq!(e.kind, ScanErrorKind::SyntaxError);
    assert_eq!(e.span, Span::new(0, 4));
    assert_eq!(e.byte, None);
    for input in [
        &b"(a\\"[..],
        b"(a(b)",
        b"<4",
        b"<~87",
        b"{ 1",
        b">",
        b"> 1",
        b")",
    ] {
        assert_eq!(
            error(&mut m, input).kind,
            ScanErrorKind::SyntaxError,
            "{input:?}"
        );
    }
}

// undefined-immediate-name.ps
#[test]
fn undefined_immediate_name() {
    let mut m = Memory::new();
    let e = error(&mut m, b"//nosuchname");
    assert_eq!(e.kind, ScanErrorKind::Undefined);
    assert_eq!(e.span, Span::new(0, 12));
}

#[test]
fn immediate_names_substitute_the_resolved_value() {
    let mut m = Memory::new();
    let mut defs = HashMap::new();
    defs.insert(m.names_mut().intern(b"x").unwrap(), Object::integer(42));
    let mut resolver = |name: Atom, _: &mut Memory| defs.get(&name).copied();
    let tokens = scan_all(b"//x {//x /x} x", &mut m, &mut resolver).unwrap();
    assert_eq!(tokens[0].0.as_i32(), Some(42));
    assert_eq!(tokens[0].1, Span::new(0, 3));
    let body = m.array(tokens[1].0).unwrap();
    assert_eq!(body[0].as_i32(), Some(42));
    assert!(body[1].is_literal());
    assert_eq!(tokens[2].0.ty(), Type::Name);
    assert!(tokens[2].0.is_executable());
}

// binary-lead-byte.ps
#[test]
fn binary_lead_byte() {
    let mut m = Memory::new();
    let e = error(&mut m, b"\x80");
    assert_eq!(e.kind, ScanErrorKind::BinaryEncoding);
    assert_eq!(e.span, Span::new(0, 1));
    assert_eq!(e.byte, Some(0x80));
    assert_eq!(error(&mut m, b"1 \x9f").kind, ScanErrorKind::BinaryEncoding);
    let t = objects(&mut m, b"\xa0 a\x80b \xff");
    assert_eq!(name_text(&m, t[0]), b"\xa0");
    assert_eq!(name_text(&m, t[1]), b"a\x80b");
    assert_eq!(name_text(&m, t[2]), b"\xff");
}

#[test]
fn errors_leave_the_scanner_reusable() {
    let mut m = Memory::new();
    let input = b"{ ) 1";
    let mut source = SliceSource::new(input);
    let mut scanner = Scanner::new();
    let e = scanner.next(&mut source, &mut m, &mut ()).unwrap_err();
    assert_eq!(e.kind, ScanErrorKind::SyntaxError);
    assert_eq!(scanner.depth(), 0);
    let (o, _) = one(&mut scanner, &mut source, &mut m);
    assert_eq!(o.as_i32(), Some(1));
}

// --- allocation through the VM -----------------------------------------------

// global-allocation-mode.ps
#[test]
fn global_allocation_mode() {
    let mut m = Memory::new();
    m.set_global(true);
    let t = objects(&mut m, b"{(a)}");
    assert_eq!(t[0].space(), Some(Space::Global));
    let s = m.array(t[0]).unwrap()[0];
    assert_eq!(s.ty(), Type::String);
    assert_eq!(s.space(), Some(Space::Global));
    m.set_global(false);
    let t = objects(&mut m, b"{(a)} <41>");
    assert_eq!(t[0].space(), Some(Space::Local));
    assert_eq!(m.array(t[0]).unwrap()[0].space(), Some(Space::Local));
    assert_eq!(t[1].space(), Some(Space::Local));
}

// immediate-local-into-global.ps
#[test]
fn local_value_into_global_procedure_is_invalidaccess() {
    let mut m = Memory::new();
    let local = m.alloc_string(b"local".to_vec());
    let mut resolver = move |_: Atom, _: &mut Memory| Some(local);
    m.set_global(true);
    let e = scan_all(b"{ //v }", &mut m, &mut resolver).unwrap_err();
    assert_eq!(e.kind, ScanErrorKind::Vm(VmError::InvalidAccess));
    assert_eq!(e.kind.name(), "invalidaccess");
    assert_eq!(e.span, Span::new(2, 5));
    let top = scan_all(b"//v", &mut m, &mut resolver).unwrap();
    assert!(top[0].0.eq(local));
    m.set_global(false);
    let t = scan_all(b"{ //v }", &mut m, &mut resolver).unwrap();
    assert!(m.array(t[0].0).unwrap()[0].eq(local));
}

// --- DSC observer ------------------------------------------------------------

// dsc-page-comment.ps
#[test]
fn page_comment_reaches_the_observer() {
    let mut m = Memory::new();
    let mut seen: Vec<(Vec<u8>, Span)> = Vec::new();
    let mut scanner = Scanner::with_dsc_observer(|text, span| seen.push((text.to_vec(), span)));
    let mut source = SliceSource::new(b"%%Page: 1 1\n0");
    let (o, span) = one(&mut scanner, &mut source, &mut m);
    assert_eq!(o.as_i32(), Some(0));
    assert_eq!(span, Span::new(12, 13));
    drop(scanner);
    assert_eq!(seen, [(b"%%Page: 1 1".to_vec(), Span::new(0, 11))]);
}

#[test]
fn only_line_initial_dsc_comments_are_offered() {
    let mut m = Memory::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let input = b"%!PS-Adobe-3.0\r\n% plain\n1 %%not dsc\n%%Trailer\r%%EOF";
    let mut source = SliceSource::new(input);
    let mut scanner = Scanner::with_dsc_observer(|text, _| seen.push(text.to_vec()));
    let mut count = 0;
    while let Scan::Token { .. } = scanner.next(&mut source, &mut m, &mut ()).unwrap() {
        count += 1;
    }
    drop(scanner);
    assert_eq!(count, 1);
    assert_eq!(
        seen,
        [
            b"%!PS-Adobe-3.0".to_vec(),
            b"%%Trailer".to_vec(),
            b"%%EOF".to_vec()
        ]
    );
    let mut scanner = Scanner::new();
    scanner.set_dsc_observer(None);
    let mut source = SliceSource::new(input);
    let mut count = 0;
    while let Scan::Token { .. } = scanner.next(&mut source, &mut m, &mut ()).unwrap() {
        count += 1;
    }
    assert_eq!(count, 1);
}

// --- property tests ----------------------------------------------------------

fn token_text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any::<i32>().prop_map(|i| i.to_string()),
        2 => (-1.0e6f32..1.0e6).prop_map(|r| format!("{r:?}")),
        2 => "[a-zA-Z@#*+_.0-9-]{1,10}",
        1 => "/[a-zA-Z0-9]{0,8}",
        2 => "\\([^\\\\()]{0,12}\\)",
        1 => "\\(\\\\[nrtbf()\\\\0-7]{1,3}\\)",
        1 => "<[0-9a-fA-F ]{0,10}>",
        1 => "<~[!-u]{0,10}~>",
        1 => Just("<<".to_string()),
        1 => Just(">>".to_string()),
        1 => Just("[".to_string()),
        1 => Just("]".to_string()),
        1 => "%[ -~]{0,10}\n",
        1 => "%%[ -~]{0,10}\r\n",
    ]
}

fn program() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        (
            token_text(),
            prop_oneof![Just(" "), Just("\n"), Just("\r\n"), Just("\t"), Just("")],
        ),
        0..12,
    )
    .prop_map(|parts| {
        let mut depth = 0usize;
        let mut text = String::new();
        for (i, (token, sep)) in parts.iter().enumerate() {
            if i % 3 == 0 {
                text.push('{');
                depth += 1;
            }
            text.push_str(token);
            text.push_str(sep);
            if i % 4 == 3 && depth > 0 {
                text.push('}');
                depth -= 1;
            }
        }
        for _ in 0..depth {
            text.push('}');
        }
        text.into_bytes()
    })
}

fn chunked_shapes(m: &mut Memory, input: &[u8], split: usize) -> Result<Vec<Shape>, ScanErrorKind> {
    let mut source = ChunkSource::new();
    let mut scanner = Scanner::new();
    let mut objects = Vec::new();
    for chunk in [&input[..split], &input[split..]] {
        source.append(chunk);
        loop {
            match scanner.next(&mut source, m, &mut ()) {
                Ok(Scan::Token { object, .. }) => objects.push(object),
                Ok(Scan::NeedMore) => break,
                Ok(Scan::End) => panic!("End before finish"),
                Err(e) => return Err(e.kind),
            }
        }
    }
    source.finish();
    loop {
        match scanner.next(&mut source, m, &mut ()) {
            Ok(Scan::Token { object, .. }) => objects.push(object),
            Ok(Scan::End) => break,
            Ok(Scan::NeedMore) => panic!("NeedMore after finish"),
            Err(e) => return Err(e.kind),
        }
    }
    Ok(objects.iter().map(|&o| shape(m, o)).collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_split_point_matches_one_shot_scanning(input in program()) {
        let mut m = Memory::new();
        let expected = shapes(&mut m, &input);
        for split in 0..=input.len() {
            prop_assert_eq!(chunked_shapes(&mut m, &input, split), expected.clone(), "split at {}", split);
        }
    }

    #[test]
    fn every_split_point_matches_for_arbitrary_bytes(input in prop::collection::vec(any::<u8>(), 0..24)) {
        let mut m = Memory::new();
        let expected = shapes(&mut m, &input);
        for split in 0..=input.len() {
            prop_assert_eq!(chunked_shapes(&mut m, &input, split), expected.clone(), "split at {}", split);
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(input in prop::collection::vec(any::<u8>(), 0..200)) {
        let mut m = Memory::new();
        let _ = scan_all(&input, &mut m, &mut ());
        m.set_global(true);
        let _ = scan_all(&input, &mut m, &mut ());
    }
}

// --- round trip: generated token values → text → scan ------------------------

#[derive(Debug, Clone)]
enum Value {
    Int(i32),
    Real(f32),
    Name(Vec<u8>, bool),
    Str(Vec<u8>),
    Proc(Vec<Value>),
}

fn name_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop_oneof![
            9 => (b'a'..=b'z').prop_map(|b| b),
            2 => (b'0'..=b'9').prop_map(|b| b),
            1 => prop::sample::select(vec![b'#', b'@', b'!', b'*', b'_', b'-', b'.', b'?', 0xA0u8, 0xFFu8]),
        ],
        1..12,
    )
    .prop_filter("must not scan as a number", |bytes| {
        ps_vm::scanner::parse_number(bytes).is_ok_and(|n| n.is_none())
    })
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        3 => any::<i32>().prop_map(Value::Int),
        3 => any::<f32>().prop_filter("finite", |r| r.is_finite()).prop_map(Value::Real),
        3 => (name_bytes(), any::<bool>()).prop_map(|(n, x)| Value::Name(n, x)),
        3 => prop::collection::vec(any::<u8>(), 0..24).prop_map(Value::Str),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop::collection::vec(inner, 0..6).prop_map(Value::Proc)
    })
}

fn serialize(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Value::Real(r) => {
            let mut text = format!("{r:?}");
            if !text.contains(['.', 'e', 'E']) {
                text.push_str(".0");
            }
            out.extend_from_slice(text.as_bytes());
        }
        Value::Name(name, executable) => {
            if !executable {
                out.push(b'/');
            }
            out.extend_from_slice(name);
        }
        Value::Str(bytes) => {
            if bytes.len() % 2 == 0 {
                out.push(b'(');
                for &b in bytes {
                    match b {
                        b'(' | b')' | b'\\' => {
                            out.push(b'\\');
                            out.push(b);
                        }
                        b'\r' => out.extend_from_slice(b"\\r"),
                        b'\n' => out.extend_from_slice(b"\\n"),
                        0..=31 | 127..=255 => {
                            out.extend_from_slice(format!("\\{b:03o}").as_bytes())
                        }
                        _ => out.push(b),
                    }
                }
                out.push(b')');
            } else {
                out.push(b'<');
                for &b in bytes {
                    out.extend_from_slice(format!("{b:02x}").as_bytes());
                }
                out.push(b'>');
            }
        }
        Value::Proc(items) => {
            out.push(b'{');
            for item in items {
                serialize(item, out);
                out.push(b' ');
            }
            out.push(b'}');
        }
    }
}

fn expected_shape(value: &Value) -> Shape {
    match value {
        Value::Int(i) => Shape::Int(*i),
        Value::Real(r) => Shape::Real(r.to_bits()),
        Value::Name(n, x) => Shape::Name(n.clone(), *x),
        Value::Str(b) => Shape::Str(b.clone()),
        Value::Proc(items) => Shape::Proc(items.iter().map(expected_shape).collect()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn serialized_values_scan_back_to_themselves(values in prop::collection::vec(value(), 0..8)) {
        let mut text = Vec::new();
        for v in &values {
            serialize(v, &mut text);
            text.push(b'\n');
        }
        let mut m = Memory::new();
        let scanned = shapes(&mut m, &text);
        let expected: Vec<_> = values.iter().map(expected_shape).collect();
        prop_assert_eq!(scanned, Ok(expected), "text: {:?}", String::from_utf8_lossy(&text));
    }
}
