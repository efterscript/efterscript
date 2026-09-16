// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Function dictionaries (PLRM3 §3.10.1) as the boundary carries them:
//! each type's entries and defaults, the dimensional checks, the sample
//! source read in full, and the errors. A program leaves the dictionary
//! on the operand stack and the reader is given that object; `shfill`
//! is what will read one in a job.

use ps_vm::ops::function::{read_function, read_function_or_array};
use ps_vm::{Config, FunctionSpec, Interp, Io, Outcome, SliceSource, VmError};

/// Runs `program`, which leaves the operand on the stack, and reads it
/// as a function of the given arity.
fn function(program: &str, inputs: usize, outputs: Option<usize>) -> Result<FunctionSpec, VmError> {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    assert_eq!(outcome, Outcome::Ok, "{program}");
    let object = interp.peek(0).expect("the operand");
    read_function(&mut interp, object, inputs, outputs)
}

fn functions(program: &str, inputs: usize, outputs: usize) -> Result<Vec<FunctionSpec>, VmError> {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    assert_eq!(outcome, Outcome::Ok, "{program}");
    let object = interp.peek(0).expect("the operand");
    read_function_or_array(&mut interp, object, inputs, outputs)
}

const SAMPLED: &str = "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 \
     /DataSource <00 55 AA FF> >>";

#[test]
fn a_sampled_function_with_its_defaults() {
    let f = function(SAMPLED, 1, Some(1)).unwrap();
    assert_eq!(
        f,
        FunctionSpec::Sampled {
            domain: vec![0.0, 1.0],
            range: vec![0.0, 1.0],
            size: vec![4],
            bits: 8,
            order: 1,
            encode: vec![0.0, 3.0],
            decode: vec![0.0, 1.0],
            samples: vec![0x00, 0x55, 0xAA, 0xFF],
        }
    );
    assert_eq!(f.inputs(), 1);
    assert_eq!(f.outputs(), 1);
    assert_eq!(f.function_type(), 0);
    assert_eq!(f.domain(), &[0.0, 1.0]);
    // Every entry given: two inputs, three outputs, four-bit samples,
    // a longer source than needed, cubic order.
    let f = function(
        "<< /FunctionType 0 /Domain [-1 1 0 2] /Range [0 1 0 1 0 1] /Size [2 3] /BitsPerSample 4 \
         /Order 3 /Encode [0 1 0 2] /Decode [0 0.5 0 0.5 0 0.5] /DataSource <0123 4567 89AB CDEF 00> >>",
        2,
        None,
    )
    .unwrap();
    let FunctionSpec::Sampled {
        size,
        bits,
        order,
        encode,
        decode,
        samples,
        ..
    } = &f
    else {
        panic!("sampled");
    };
    assert_eq!((size.as_slice(), *bits, *order), (&[2, 3][..], 4, 3));
    assert_eq!(encode, &[0.0, 1.0, 0.0, 2.0]);
    assert_eq!(decode, &[0.0, 0.5, 0.0, 0.5, 0.0, 0.5]);
    // 2 × 3 samples × 3 outputs × 4 bits = 72 bits = 9 bytes.
    assert_eq!(samples.len(), 9);
    assert_eq!(f.inputs(), 2);
    assert_eq!(f.outputs(), 3);
    // Odd bit counts round the byte count up.
    let f = function(
        "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [3] /BitsPerSample 12 \
         /DataSource <0000 0000 00> >>",
        1,
        None,
    )
    .unwrap();
    assert!(matches!(f, FunctionSpec::Sampled { samples, .. } if samples.len() == 5));
}

#[test]
fn a_sampled_function_reads_its_file_source_in_full() {
    // A reusable stream is read from its start, however far it was
    // read before.
    let f = function(
        "/s <00 55 AA FF> /ReusableStreamDecode filter def s 2 string readstring pop pop \
         << /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource s >>",
        1,
        None,
    )
    .unwrap();
    assert!(
        matches!(f, FunctionSpec::Sampled { samples, .. } if samples == [0x00, 0x55, 0xAA, 0xFF])
    );
    // An ordinary file is read from where it stands, and only the
    // bytes the table needs.
    let f = function(
        "/f (0055AAFF11) /ASCIIHexDecode filter def f 1 string readstring pop pop \
         << /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [3] /BitsPerSample 8 /DataSource f >>",
        1,
        None,
    )
    .unwrap();
    assert!(matches!(f, FunctionSpec::Sampled { samples, .. } if samples == [0x55, 0xAA, 0xFF]));
    // Short sources are rangecheck; a closed file ioerror.
    let short =
        "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource ";
    assert_eq!(
        function(&format!("{short} <0055AA> >>"), 1, None),
        Err(VmError::RangeCheck)
    );
    assert_eq!(
        function(
            &format!("{short} (0055AA) /ASCIIHexDecode filter >>"),
            1,
            None
        ),
        Err(VmError::RangeCheck)
    );
    assert_eq!(
        function(
            &format!("{short} <0055AAFF> /ReusableStreamDecode filter dup closefile >>"),
            1,
            None
        ),
        Err(VmError::IoError)
    );
    assert_eq!(
        function(&format!("{short} [0 85 170 255] >>"), 1, None),
        Err(VmError::TypeCheck)
    );
}

#[test]
fn sampled_function_errors() {
    for (dict, error) in [
        // Required entries.
        (
            "<< /FunctionType 0 /Domain [0 1] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::Undefined,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::Undefined,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /DataSource <00000000> >>",
            VmError::Undefined,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 >>",
            VmError::Undefined,
        ),
        ("<< /Domain [0 1] >>", VmError::Undefined),
        ("<< /FunctionType 0 >>", VmError::Undefined),
        // Types.
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size 4 /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4.0] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample (8) /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /Order 1.0 /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /Encode 1 /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 (1)] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::TypeCheck,
        ),
        ("<< /FunctionType 0.0 /Domain [0 1] >>", VmError::TypeCheck),
        // Ranges and dimensions.
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [0] /BitsPerSample 8 /DataSource <> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 7 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /Order 2 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /Encode [0 1 0 1] /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4] /BitsPerSample 8 /Decode [0] /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [4 4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [1 0] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1] /Range [1 0] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [0 1 0] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 0 /Domain [] /Range [0 1] /Size [4] /BitsPerSample 8 /DataSource <00000000> >>",
            VmError::RangeCheck,
        ),
        ("<< /FunctionType 1 /Domain [0 1] >>", VmError::RangeCheck),
        ("<< /FunctionType 4 /Domain [0 1] >>", VmError::RangeCheck),
    ] {
        assert_eq!(function(dict, 1, None), Err(error), "{dict}");
    }
    // Not a dictionary.
    assert_eq!(function("[0 1]", 1, None), Err(VmError::TypeCheck));
    assert_eq!(function("42", 1, None), Err(VmError::TypeCheck));
    // The arity the caller expects.
    assert_eq!(function(SAMPLED, 2, None), Err(VmError::RangeCheck));
    assert_eq!(function(SAMPLED, 1, Some(3)), Err(VmError::RangeCheck));
}

#[test]
fn an_exponential_function_with_its_defaults() {
    let f = function("<< /FunctionType 2 /Domain [0 1] /N 1 >>", 1, None).unwrap();
    assert_eq!(
        f,
        FunctionSpec::Exponential {
            domain: vec![0.0, 1.0],
            range: Vec::new(),
            c0: vec![0.0],
            c1: vec![1.0],
            n: 1.0,
        }
    );
    assert_eq!(f.inputs(), 1);
    assert_eq!(f.outputs(), 1);
    assert_eq!(f.function_type(), 2);
    let f = function(
        "<< /FunctionType 2 /Domain [0 1] /Range [0 1 0 1 0 1] /C0 [1 0 0] /C1 [0 0 1] /N 2.5 >>",
        1,
        Some(3),
    )
    .unwrap();
    assert_eq!(
        f,
        FunctionSpec::Exponential {
            domain: vec![0.0, 1.0],
            range: vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            c0: vec![1.0, 0.0, 0.0],
            c1: vec![0.0, 0.0, 1.0],
            n: 2.5,
        }
    );
    assert_eq!(f.outputs(), 3);
    for (dict, error) in [
        ("<< /FunctionType 2 /Domain [0 1] >>", VmError::Undefined),
        (
            "<< /FunctionType 2 /Domain [0 1] /N (1) >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 1 >>",
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 [0 0] /C1 [1] >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 [] /C1 [] >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 2 /Domain [0 1] /N 1 /Range [0 1 0 1] >>",
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 2 /Domain [0 1 0 1] /N 1 >>",
            VmError::RangeCheck,
        ),
    ] {
        assert_eq!(function(dict, 1, None), Err(error), "{dict}");
    }
    assert_eq!(
        function("<< /FunctionType 2 /Domain [0 1] /N 1 >>", 1, Some(3)),
        Err(VmError::RangeCheck)
    );
    assert_eq!(
        function("<< /FunctionType 2 /Domain [0 1] /N 1 >>", 2, None),
        Err(VmError::RangeCheck)
    );
}

const PARTS: &str = "[ << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 1 0] /N 1 >> \
       << /FunctionType 2 /Domain [0 1] /C0 [0 1 0] /C1 [0 0 1] /N 1 >> ]";

#[test]
fn a_stitching_function_reads_its_parts() {
    let program = format!(
        "<< /FunctionType 3 /Domain [0 1] /Functions {PARTS} /Bounds [0.4] /Encode [0 1 1 0] >>"
    );
    let f = function(&program, 1, Some(3)).unwrap();
    let FunctionSpec::Stitching {
        domain,
        range,
        functions,
        bounds,
        encode,
    } = &f
    else {
        panic!("stitching");
    };
    assert_eq!(domain, &[0.0, 1.0]);
    assert!(range.is_empty());
    assert_eq!(functions.len(), 2);
    assert_eq!(functions[1].outputs(), 3);
    assert_eq!(bounds, &[0.4]);
    assert_eq!(encode, &[0.0, 1.0, 1.0, 0.0]);
    assert_eq!(f.inputs(), 1);
    assert_eq!(f.outputs(), 3);
    assert_eq!(f.function_type(), 3);
    // One part, no bounds; a range fixes the output count; nesting.
    let f = function(
        "<< /FunctionType 3 /Domain [0 1] /Range [0 1] /Functions [ \
           << /FunctionType 3 /Domain [0 1] /Functions [ << /FunctionType 2 /Domain [0 1] /N 1 >> ] \
              /Bounds [] /Encode [0 1] >> ] /Bounds [] /Encode [1 0] >>",
        1,
        None,
    )
    .unwrap();
    assert_eq!(f.outputs(), 1);
    assert!(matches!(&f, FunctionSpec::Stitching { functions, .. }
        if matches!(functions[0], FunctionSpec::Stitching { .. })));
    // A sampled part beside an exponential one.
    let f = function(
        &format!(
            "<< /FunctionType 3 /Domain [0 2] /Functions [ {SAMPLED} << /FunctionType 2 /Domain [0 1] /N 1 >> ] \
             /Bounds [1] /Encode [0 1 0 1] >>"
        ),
        1,
        Some(1),
    )
    .unwrap();
    assert_eq!(f.outputs(), 1);
}

#[test]
fn stitching_function_errors() {
    let with =
        |entries: &str| format!("<< /FunctionType 3 /Domain [0 1] /Functions {PARTS} {entries} >>");
    for (dict, error) in [
        // Required entries.
        (with("/Encode [0 1 0 1]"), VmError::Undefined),
        (with("/Bounds [0.5]"), VmError::Undefined),
        (
            "<< /FunctionType 3 /Domain [0 1] /Bounds [] /Encode [0 1] >>".to_string(),
            VmError::Undefined,
        ),
        // Types.
        (with("/Bounds 0.5 /Encode [0 1 0 1]"), VmError::TypeCheck),
        (with("/Bounds [0.5] /Encode (x)"), VmError::TypeCheck),
        (
            "<< /FunctionType 3 /Domain [0 1] /Functions 1 /Bounds [] /Encode [0 1] >>".to_string(),
            VmError::TypeCheck,
        ),
        (
            "<< /FunctionType 3 /Domain [0 1] /Functions [1] /Bounds [] /Encode [0 1] >>".to_string(),
            VmError::TypeCheck,
        ),
        // Bounds: count, order, within the domain.
        (with("/Bounds [] /Encode [0 1 0 1]"), VmError::RangeCheck),
        (with("/Bounds [0.7 0.3] /Encode [0 1 0 1]"), VmError::RangeCheck),
        (with("/Bounds [1.5] /Encode [0 1 0 1]"), VmError::RangeCheck),
        (with("/Bounds [0] /Encode [0 1 0 1]"), VmError::RangeCheck),
        (with("/Bounds [1] /Encode [0 1 0 1]"), VmError::RangeCheck),
        (with("/Bounds [0.5] /Encode [0 1]"), VmError::RangeCheck),
        (
            "<< /FunctionType 3 /Domain [0 1] /Functions [] /Bounds [] /Encode [] >>".to_string(),
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 3 /Domain [0 1 0 1] /Functions [ << /FunctionType 2 /Domain [0 1] /N 1 >> ] \
             /Bounds [] /Encode [0 1] >>"
                .to_string(),
            VmError::RangeCheck,
        ),
        // The parts must agree on their outputs, with the range, and
        // take one input.
        (
            "<< /FunctionType 3 /Domain [0 1] /Functions [ << /FunctionType 2 /Domain [0 1] /N 1 >> \
             << /FunctionType 2 /Domain [0 1] /C0 [0 0] /C1 [1 1] /N 1 >> ] /Bounds [0.5] /Encode [0 1 0 1] >>"
                .to_string(),
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 3 /Domain [0 1] /Range [0 1 0 1] /Functions [ << /FunctionType 2 /Domain [0 1] /N 1 >> ] \
             /Bounds [] /Encode [0 1] >>"
                .to_string(),
            VmError::RangeCheck,
        ),
        (
            "<< /FunctionType 3 /Domain [0 1] /Functions [ << /FunctionType 0 /Domain [0 1 0 1] /Range [0 1] \
             /Size [1 1] /BitsPerSample 8 /DataSource <00> >> ] /Bounds [] /Encode [0 1] >>"
                .to_string(),
            VmError::RangeCheck,
        ),
    ] {
        assert_eq!(function(&dict, 1, None), Err(error), "{dict}");
    }
    assert_eq!(
        function(&with("/Bounds [0.5] /Encode [0 1 0 1]"), 1, Some(1)),
        Err(VmError::RangeCheck)
    );
    // Nesting has a limit.
    let mut deep = "<< /FunctionType 2 /Domain [0 1] /N 1 >>".to_string();
    for _ in 0..12 {
        deep = format!(
            "<< /FunctionType 3 /Domain [0 1] /Functions [ {deep} ] /Bounds [] /Encode [0 1] >>"
        );
    }
    assert_eq!(function(&deep, 1, None), Err(VmError::LimitCheck));
}

#[test]
fn a_shading_function_may_be_an_array_of_one_output_functions() {
    let parts = functions(
        PARTS
            .replace("/C0 [1 0 0] /C1 [0 1 0]", "/C0 [1] /C1 [0]")
            .replace("/C0 [0 1 0] /C1 [0 0 1]", "/C0 [0] /C1 [1]")
            .as_str(),
        1,
        2,
    )
    .unwrap();
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|f| f.outputs() == 1));
    let one = functions(
        "<< /FunctionType 2 /Domain [0 1] /C0 [0 0] /C1 [1 1] /N 1 >>",
        1,
        2,
    )
    .unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].outputs(), 2);
    // The count must match the components, each part have one output,
    // and the operand be a dictionary or an array.
    assert_eq!(functions(PARTS, 1, 2), Err(VmError::RangeCheck));
    assert_eq!(
        functions("[ << /FunctionType 2 /Domain [0 1] /N 1 >> ]", 1, 2),
        Err(VmError::RangeCheck)
    );
    assert_eq!(functions("(x)", 1, 1), Err(VmError::TypeCheck));
    assert_eq!(functions("[ 1 ]", 1, 1), Err(VmError::TypeCheck));
}
