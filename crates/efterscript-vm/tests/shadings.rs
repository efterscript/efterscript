// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Shading dictionaries (PLRM3 §4.9.3) as the boundary carries them: the
//! common entries and the colour-space rules, each type's entries, mesh
//! data from a packed string and from an array (re-encoded, and decoded
//! back), the structural walk, and the errors. A program leaves the
//! dictionary on the operand stack and the reader is given that object.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Recording};
use efterscript_vm::ops::shading::{MeshElement, mesh_elements, read_shading};
use efterscript_vm::{
    Bounds, Config, FunctionSpec, Interp, Io, Matrix, Outcome, ShadingKind, ShadingSpec,
    SliceSource, SpaceSpec, VmError,
};

fn shading(program: &str) -> Result<ShadingSpec, VmError> {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    assert_eq!(outcome, Outcome::Ok, "{program}");
    let object = interp.peek(0).expect("the operand");
    read_shading(&mut interp, object)
}

const AXIAL_FN: &str = "<< /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >>";

fn axial(extra: &str) -> String {
    format!(
        "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0] /Function {AXIAL_FN} {extra} >>"
    )
}

#[test]
fn common_entries_and_their_defaults() {
    let s = shading(&axial("")).unwrap();
    assert_eq!(s.space, SpaceSpec::DeviceRGB);
    assert_eq!(s.background, None);
    assert_eq!(s.bbox, None);
    assert!(!s.antialias);
    assert_eq!(s.kind.shading_type(), 2);
    assert_eq!(s.values_per_color(), 1);
    let s = shading(&axial(
        "/Background [1 1 0] /BBox [0 0 50 20] /AntiAlias true",
    ))
    .unwrap();
    assert_eq!(s.background, Some(vec![1.0, 1.0, 0.0]));
    assert_eq!(s.bbox, Some(Bounds::new(0.0, 0.0, 50.0, 20.0)));
    assert!(s.antialias);
    // Every family the boundary carries, the array form included.
    let s = shading(&axial("").replace("/DeviceRGB", "[/DeviceRGB]")).unwrap();
    assert_eq!(s.space, SpaceSpec::DeviceRGB);
    let s = shading(
        &axial("")
            .replace(
                "/ColorSpace /DeviceRGB",
                "/ColorSpace [/Separation /Spot /DeviceGray { 1 exch sub }]",
            )
            .replace("/C0 [1 0 0] /C1 [0 0 1]", "/C0 [1] /C1 [0]"),
    )
    .unwrap();
    assert!(matches!(s.space, SpaceSpec::Separation { .. }));
    // A CIE-based space that collapses is carried calibrated.
    let s = shading(&axial("").replace(
        "/ColorSpace /DeviceRGB",
        "/ColorSpace [/CIEBasedABC << /WhitePoint [0.9505 1 1.089] >>]",
    ))
    .unwrap();
    assert!(matches!(s.space, SpaceSpec::CalRGB { .. }));
    for (dict, error) in [
        // Required entries and their types.
        (
            axial("").replace("/ShadingType 2 ", ""),
            VmError::Undefined,
        ),
        (
            axial("").replace("/ColorSpace /DeviceRGB ", ""),
            VmError::Undefined,
        ),
        (axial("").replace("/ShadingType 2", "/ShadingType 2.0"), VmError::TypeCheck),
        (axial("").replace("/ShadingType 2", "/ShadingType 8"), VmError::RangeCheck),
        (axial("").replace("/ShadingType 2", "/ShadingType 0"), VmError::RangeCheck),
        (axial("").replace("/ColorSpace /DeviceRGB", "/ColorSpace 3"), VmError::TypeCheck),
        (
            axial("").replace("/ColorSpace /DeviceRGB", "/ColorSpace /NoSuchSpace"),
            VmError::Undefined,
        ),
        // The optional ones.
        (axial("/Background 1"), VmError::TypeCheck),
        (axial("/Background [1 1]"), VmError::RangeCheck),
        (axial("/Background [1 1 (0)]"), VmError::TypeCheck),
        (axial("/BBox 1"), VmError::TypeCheck),
        (axial("/BBox [0 0 1]"), VmError::TypeCheck),
        (axial("/BBox [1 1 0 0]"), VmError::RangeCheck),
        (axial("/AntiAlias 1"), VmError::TypeCheck),
        // A pattern space is out of range; a non-collapsing CIE-based
        // space is a limit, as itself or as an Indexed base.
        (
            axial("").replace("/ColorSpace /DeviceRGB", "/ColorSpace /Pattern"),
            VmError::RangeCheck,
        ),
        (
            axial("").replace("/ColorSpace /DeviceRGB", "/ColorSpace [/Pattern /DeviceRGB]"),
            VmError::RangeCheck,
        ),
        (
            axial("").replace(
                "/ColorSpace /DeviceRGB",
                "/ColorSpace [/CIEBasedABC << /WhitePoint [0.9505 1 1.089] /DecodeABC [{dup mul} {dup mul} {dup mul}] /MatrixLMN [1 0 0 0 1 0 0 0 1] /DecodeLMN [{sqrt} {sqrt} {sqrt}] >>]",
            ),
            VmError::LimitCheck,
        ),
    ] {
        assert_eq!(shading(&dict), Err(error), "{dict}");
    }
    assert_eq!(shading("[1 2]"), Err(VmError::TypeCheck));
}

#[test]
fn indexed_spaces_are_for_meshes_without_a_function() {
    let indexed = "[/Indexed /DeviceRGB 1 <FF000000FF00>]";
    let mesh = format!(
        "<< /ShadingType 4 /ColorSpace {indexed} /DataSource [0 0 0 0  0 10 0 1  0 0 10 1] >>"
    );
    let s = shading(&mesh).unwrap();
    assert!(matches!(s.space, SpaceSpec::Indexed { .. }));
    assert_eq!(s.values_per_color(), 1);
    assert_eq!(
        shading(&axial("").replace("/DeviceRGB", indexed)),
        Err(VmError::RangeCheck)
    );
    let with_function = mesh.replace(
        "/DataSource",
        "/Function << /FunctionType 2 /Domain [0 1] /N 1 >> /DataSource",
    );
    assert_eq!(shading(&with_function), Err(VmError::RangeCheck));
    let lab_base = "[/Indexed [/CIEBasedABC << /WhitePoint [0.9505 1 1.089] /DecodeLMN [{sqrt} {sqrt} {sqrt}] >>] 1 <FF000000FF00>]";
    assert_eq!(
        shading(&mesh.replace(indexed, lab_base)),
        Err(VmError::LimitCheck)
    );
}

#[test]
fn a_function_based_shading() {
    let f = "<< /FunctionType 2 /Domain [0 1 0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>";
    // A type 2 function takes one input, so the two-input requirement
    // refuses it; a sampled function with two inputs is accepted.
    let dict = format!("<< /ShadingType 1 /ColorSpace /DeviceRGB /Function {f} >>");
    assert_eq!(shading(&dict), Err(VmError::RangeCheck));
    let f = "<< /FunctionType 0 /Domain [0 1 0 1] /Range [0 1 0 1 0 1] /Size [2 2] /BitsPerSample 8 \
             /DataSource <000000 FF0000 00FF00 FFFF00> >>";
    let dict = format!("<< /ShadingType 1 /ColorSpace /DeviceRGB /Function {f} >>");
    let s = shading(&dict).unwrap();
    let ShadingKind::Function {
        domain,
        matrix,
        function,
    } = &s.kind
    else {
        panic!("function-based");
    };
    assert_eq!(domain, &[0.0, 1.0, 0.0, 1.0]);
    assert_eq!(*matrix, Matrix::IDENTITY);
    assert_eq!(function.len(), 1);
    assert_eq!(function[0].inputs(), 2);
    assert_eq!(function[0].outputs(), 3);
    assert_eq!(s.kind.function().len(), 1);
    let dict = format!(
        "<< /ShadingType 1 /ColorSpace /DeviceRGB /Domain [-1 1 -2 2] /Matrix [2 0 0 2 5 5] /Function {f} >>"
    );
    let s = shading(&dict).unwrap();
    assert!(
        matches!(&s.kind, ShadingKind::Function { domain, matrix, .. }
        if *domain == [-1.0, 1.0, -2.0, 2.0] && *matrix == Matrix([2.0, 0.0, 0.0, 2.0, 5.0, 5.0]))
    );
    // An array of one-output functions, one per component.
    let one = f
        .replace("/Range [0 1 0 1 0 1]", "/Range [0 1]")
        .replace("<000000 FF0000 00FF00 FFFF00>", "<00 FF 00 FF>");
    let dict =
        format!("<< /ShadingType 1 /ColorSpace /DeviceRGB /Function [ {one} {one} {one} ] >>");
    assert_eq!(shading(&dict).unwrap().kind.function().len(), 3);
    for (entries, error) in [
        ("", VmError::Undefined),
        (&format!("/Function {f} /Domain [0 1]"), VmError::RangeCheck),
        (
            &format!("/Function {f} /Domain [1 0 0 1]"),
            VmError::RangeCheck,
        ),
        (&format!("/Function {f} /Domain 1"), VmError::TypeCheck),
        (
            &format!("/Function {f} /Matrix [1 0 0 1 0]"),
            VmError::RangeCheck,
        ),
        (&format!("/Function {f} /Matrix 1"), VmError::TypeCheck),
        (&format!("/Function [ {one} {one} ]"), VmError::RangeCheck),
        ("/Function 1", VmError::TypeCheck),
    ] {
        let dict = format!("<< /ShadingType 1 /ColorSpace /DeviceRGB {entries} >>");
        assert_eq!(shading(&dict), Err(error), "{dict}");
    }
}

#[test]
fn axial_and_radial_shadings() {
    let s = shading(&axial("/Domain [0.2 0.8] /Extend [true false]")).unwrap();
    assert_eq!(
        s.kind,
        ShadingKind::Axial {
            coords: [0.0, 0.0, 100.0, 0.0],
            domain: [0.2, 0.8],
            function: vec![FunctionSpec::Exponential {
                domain: vec![0.0, 1.0],
                range: Vec::new(),
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            }],
            extend: [true, false],
        }
    );
    let radial = axial("/Extend [false true]")
        .replace("/ShadingType 2", "/ShadingType 3")
        .replace("[0 0 100 0]", "[10 10 0 10 10 50]");
    let s = shading(&radial).unwrap();
    assert!(
        matches!(&s.kind, ShadingKind::Radial { coords, domain, extend, .. }
        if *coords == [10.0, 10.0, 0.0, 10.0, 10.0, 50.0] && *domain == [0.0, 1.0] && *extend == [false, true])
    );
    assert_eq!(s.kind.shading_type(), 3);
    // Per-component functions.
    let s = shading(
        &axial("").replace(
            &format!("/Function {AXIAL_FN}"),
            "/Function [ << /FunctionType 2 /Domain [0 1] /N 1 >> << /FunctionType 2 /Domain [0 1] /N 2 >> \
             << /FunctionType 2 /Domain [0 1] /N 3 >> ]",
        ),
    )
    .unwrap();
    assert_eq!(s.kind.function().len(), 3);
    for (dict, error) in [
        (
            axial("").replace("/Coords [0 0 100 0] ", ""),
            VmError::Undefined,
        ),
        (
            axial("").replace(&format!("/Function {AXIAL_FN}"), ""),
            VmError::Undefined,
        ),
        (
            axial("").replace("[0 0 100 0]", "[0 0 100]"),
            VmError::RangeCheck,
        ),
        (axial("").replace("[0 0 100 0]", "(x)"), VmError::TypeCheck),
        (
            axial("").replace("[0 0 100 0]", "[0 0 100 /x]"),
            VmError::TypeCheck,
        ),
        (
            radial.replace("[10 10 0 10 10 50]", "[10 10 0 10 10]"),
            VmError::RangeCheck,
        ),
        (
            radial.replace("[10 10 0 10 10 50]", "[10 10 -1 10 10 50]"),
            VmError::RangeCheck,
        ),
        (
            radial.replace("[10 10 0 10 10 50]", "[10 10 0 10 10 -50]"),
            VmError::RangeCheck,
        ),
        (axial("/Domain [0]"), VmError::RangeCheck),
        (axial("/Domain 1"), VmError::TypeCheck),
        (axial("/Extend [true]"), VmError::RangeCheck),
        (axial("/Extend [true 1]"), VmError::TypeCheck),
        (axial("/Extend true"), VmError::TypeCheck),
        // The function must take one input and give the space's
        // components.
        (
            axial("").replace("/C0 [1 0 0] /C1 [0 0 1]", "/C0 [1] /C1 [0]"),
            VmError::RangeCheck,
        ),
        (
            axial("").replace("/Domain [0 1] /C0", "/Domain [0 1 0 1] /C0"),
            VmError::RangeCheck,
        ),
    ] {
        assert_eq!(shading(&dict), Err(error), "{dict}");
    }
}

/// A free-form mesh of one triangle packed by hand at eight bits: flag,
/// x, y, and three components per vertex, six bytes each.
const PACKED_TRIANGLE: &str = "<00 00 00 FF0000  00 FF 00 00FF00  00 00 FF 0000FF>";

fn mesh4(entries: &str) -> String {
    format!(
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 /BitsPerComponent 8 \
         /BitsPerFlag 8 /Decode [0 255 0 255 0 1 0 1 0 1] {entries} >>"
    )
}

fn vertex(flag: u8, x: f32, y: f32, color: &[f32]) -> MeshElement {
    MeshElement::Vertex {
        flag,
        x,
        y,
        color: color.to_vec(),
    }
}

#[test]
fn a_packed_triangle_mesh_decodes_to_its_vertices() {
    let s = shading(&mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))).unwrap();
    let ShadingKind::Mesh {
        ty,
        bits_per_coordinate,
        bits_per_component,
        bits_per_flag,
        decode,
        vertices_per_row,
        function,
        data,
    } = &s.kind
    else {
        panic!("mesh");
    };
    assert_eq!(
        (
            *ty,
            *bits_per_coordinate,
            *bits_per_component,
            *bits_per_flag
        ),
        (4, 8, 8, 8)
    );
    assert_eq!(
        decode,
        &[0.0, 255.0, 0.0, 255.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(*vertices_per_row, None);
    assert!(function.is_empty());
    assert_eq!(data.len(), 18);
    assert_eq!(s.values_per_color(), 3);
    assert_eq!(
        mesh_elements(&s).unwrap(),
        vec![
            vertex(0, 0.0, 0.0, &[1.0, 0.0, 0.0]),
            vertex(0, 255.0, 0.0, &[0.0, 1.0, 0.0]),
            vertex(0, 0.0, 255.0, &[0.0, 0.0, 1.0]),
        ]
    );
    // The same data from a reusable stream and from an ordinary filter;
    // a stream is read from its start.
    let s = shading(&mesh4(&format!(
        "/DataSource {PACKED_TRIANGLE} /ReusableStreamDecode filter dup 3 string readstring pop pop"
    )))
    .unwrap();
    assert_eq!(mesh_elements(&s).unwrap().len(), 3);
    let hex = PACKED_TRIANGLE.replace(['<', '>'], "");
    let s = shading(&mesh4(&format!(
        "/DataSource ({hex}>) /ASCIIHexDecode filter"
    )))
    .unwrap();
    assert_eq!(mesh_elements(&s).unwrap().len(), 3);
    // Sub-byte depths pad each vertex to a byte: two-bit flags, four-bit
    // coordinates, one-bit components make 13 bits, so two bytes per
    // vertex; the flag's upper bits are ignored.
    let s = shading(
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 4 /BitsPerComponent 1 \
         /BitsPerFlag 2 /Decode [0 15 0 15 0 1 0 1 0 1] /DataSource <0000 3C00 03E0> >>",
    )
    .unwrap();
    assert_eq!(
        mesh_elements(&s).unwrap(),
        vec![
            vertex(0, 0.0, 0.0, &[0.0, 0.0, 0.0]),
            vertex(0, 15.0, 0.0, &[0.0, 0.0, 0.0]),
            vertex(0, 0.0, 15.0, &[1.0, 0.0, 0.0]),
        ]
    );
    // Mesh elements of a non-mesh shading are none.
    assert_eq!(
        mesh_elements(&shading(&axial("")).unwrap()).unwrap(),
        Vec::new()
    );
}

#[test]
fn an_array_mesh_is_re_encoded_and_decodes_back() {
    let s = shading(
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /DataSource [ \
           0 10.5 -20 1 0 0   0 110.25 -20 0 1 0   0 10.5 80 0 0 1   2 110.25 80 0.5 0.5 0.5 ] >>",
    )
    .unwrap();
    let ShadingKind::Mesh {
        bits_per_coordinate,
        bits_per_component,
        bits_per_flag,
        decode,
        data,
        ..
    } = &s.kind
    else {
        panic!("mesh");
    };
    assert_eq!(
        (*bits_per_coordinate, *bits_per_component, *bits_per_flag),
        (32, 16, 8)
    );
    // Each column's own extent; the constant column stretches by one.
    assert_eq!(
        decode,
        &[10.5, 110.25, -20.0, 80.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(data.len(), 4 * (1 + 8 + 6));
    let expected = [
        vertex(0, 10.5, -20.0, &[1.0, 0.0, 0.0]),
        vertex(0, 110.25, -20.0, &[0.0, 1.0, 0.0]),
        vertex(0, 10.5, 80.0, &[0.0, 0.0, 1.0]),
        vertex(2, 110.25, 80.0, &[0.5, 0.5, 0.5]),
    ];
    let decoded = mesh_elements(&s).unwrap();
    assert_eq!(decoded.len(), 4);
    for (got, want) in decoded.iter().zip(&expected) {
        let (
            MeshElement::Vertex { flag, x, y, color },
            MeshElement::Vertex {
                flag: f,
                x: wx,
                y: wy,
                color: wc,
            },
        ) = (got, want)
        else {
            panic!("vertices");
        };
        assert_eq!(flag, f);
        assert!((x - wx).abs() < 1e-4 && (y - wy).abs() < 1e-4, "{got:?}");
        assert!(
            color.iter().zip(wc).all(|(a, b)| (a - b).abs() < 1e-4),
            "{got:?}"
        );
    }
    // A constant column decodes exactly; a parametric value under a
    // function is clipped to the unit interval and decodes from it.
    let s = shading(
        "<< /ShadingType 4 /ColorSpace /DeviceGray /Function << /FunctionType 2 /Domain [0 1] /N 1 >> \
           /DataSource [ 0 5 5 0  0 5 6 1.5  0 5 7 -1 ] >>",
    )
    .unwrap();
    assert!(matches!(&s.kind, ShadingKind::Mesh { decode, .. }
        if *decode == vec![5.0, 6.0, 5.0, 7.0, 0.0, 1.0]));
    assert_eq!(s.values_per_color(), 1);
    assert_eq!(
        mesh_elements(&s).unwrap(),
        vec![
            vertex(0, 5.0, 5.0, &[0.0]),
            vertex(0, 5.0, 6.0, &[1.0]),
            vertex(0, 5.0, 7.0, &[0.0]),
        ]
    );
    // With an array the bit entries are not needed and not read.
    let s = shading(
        "<< /ShadingType 4 /ColorSpace /DeviceGray /BitsPerFlag 3 /Decode 1 \
           /DataSource [ 0 0 0 0  0 1 0 0  0 0 1 0 ] >>",
    )
    .unwrap();
    assert_eq!(mesh_elements(&s).unwrap().len(), 3);
    // An empty free-form mesh is a whole number of triangles.
    let s = shading("<< /ShadingType 4 /ColorSpace /DeviceGray /DataSource [] >>").unwrap();
    assert_eq!(mesh_elements(&s).unwrap(), Vec::new());
    assert!(matches!(&s.kind, ShadingKind::Mesh { decode, data, .. }
        if *decode == vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0] && data.is_empty()));
}

#[test]
fn free_form_mesh_structure() {
    let tri = "0 0 0 0  0 1 0 0  0 0 1 0";
    for (data, ok) in [
        (tri.to_string(), true),
        // Continuations on either side, and a second triangle.
        (format!("{tri}  1 1 1 0  2 2 2 0  {tri}"), true),
        // The flags of the second and third vertices do not count.
        ("0 0 0 0  3 1 0 0  1 0 1 0".to_string(), true),
        // A partial triangle, a partial vertex, a continuation with
        // nothing to continue, a flag of 3 where it counts.
        ("0 0 0 0  0 1 0 0".to_string(), false),
        (format!("{tri}  0 0 0"), false),
        ("1 0 0 0  0 1 0 0  0 0 1 0".to_string(), false),
        (format!("{tri}  3 1 1 0"), false),
    ] {
        let dict = format!("<< /ShadingType 4 /ColorSpace /DeviceGray /DataSource [ {data} ] >>");
        assert_eq!(shading(&dict).is_ok(), ok, "{dict}");
        if !ok {
            assert_eq!(shading(&dict), Err(VmError::RangeCheck), "{dict}");
        }
    }
    // Flags must be integers 0 to 3, values numbers.
    for data in ["0.0 0 0 0  0 1 0 0  0 0 1 0", "0 (x) 0 0  0 1 0 0  0 0 1 0"] {
        let dict = format!("<< /ShadingType 4 /ColorSpace /DeviceGray /DataSource [ {data} ] >>");
        assert_eq!(shading(&dict), Err(VmError::TypeCheck), "{dict}");
    }
    let dict =
        "<< /ShadingType 4 /ColorSpace /DeviceGray /DataSource [ 4 0 0 0  0 1 0 0  0 0 1 0 ] >>";
    assert_eq!(shading(dict), Err(VmError::RangeCheck));
    // Packed data: a trailing partial vertex, and a partial triangle.
    let dict = mesh4("/DataSource <00 00 00 FF0000  00 FF 00 00FF00  00 00 FF 0000FF  00>");
    assert_eq!(shading(&dict), Err(VmError::RangeCheck));
    let dict = mesh4("/DataSource <00 00 00 FF0000  00 FF 00 00FF00>");
    assert_eq!(shading(&dict), Err(VmError::RangeCheck));
    let dict = mesh4("/DataSource <01 00 00 FF0000  00 FF 00 00FF00  00 00 FF 0000FF>");
    assert_eq!(shading(&dict), Err(VmError::RangeCheck));
}

#[test]
fn lattice_mesh_structure() {
    let row = "0 0 0  1 0 0  2 0 0";
    let dict = format!(
        "<< /ShadingType 5 /ColorSpace /DeviceGray /VerticesPerRow 3 /DataSource [ {row}  {row} ] >>"
    );
    let s = shading(&dict).unwrap();
    assert!(
        matches!(&s.kind, ShadingKind::Mesh { ty: 5, bits_per_flag: 0, vertices_per_row: Some(3), data, .. }
        if data.len() == 6 * (8 + 2))
    );
    assert_eq!(mesh_elements(&s).unwrap().len(), 6);
    assert_eq!(mesh_elements(&s).unwrap()[5], vertex(0, 2.0, 0.0, &[0.0]));
    // Packed: no flags, two rows of two.
    let s = shading(
        "<< /ShadingType 5 /ColorSpace /DeviceGray /VerticesPerRow 2 /BitsPerCoordinate 8 \
         /BitsPerComponent 8 /Decode [0 1 0 1 0 1] /DataSource <000000 FF0000 00FF00 FFFFFF> >>",
    )
    .unwrap();
    assert_eq!(mesh_elements(&s).unwrap()[3], vertex(0, 1.0, 1.0, &[1.0]));
    for (entries, error) in [
        // One row, a partial row, a partial vertex.
        (
            format!("/VerticesPerRow 3 /DataSource [ {row} ]"),
            VmError::RangeCheck,
        ),
        (
            format!("/VerticesPerRow 3 /DataSource [ {row} {row} 0 0 0 ]"),
            VmError::RangeCheck,
        ),
        (
            format!("/VerticesPerRow 3 /DataSource [ {row} {row} 0 ]"),
            VmError::RangeCheck,
        ),
        (
            "/VerticesPerRow 3 /DataSource [ ]".to_string(),
            VmError::RangeCheck,
        ),
        // The row length is required, an integer, at least 2.
        (format!("/DataSource [ {row} {row} ]"), VmError::Undefined),
        (
            format!("/VerticesPerRow 3.0 /DataSource [ {row} {row} ]"),
            VmError::TypeCheck,
        ),
        (
            format!("/VerticesPerRow 1 /DataSource [ {row} {row} ]"),
            VmError::RangeCheck,
        ),
    ] {
        let dict = format!("<< /ShadingType 5 /ColorSpace /DeviceGray {entries} >>");
        assert_eq!(shading(&dict), Err(error), "{dict}");
    }
}

/// Twelve control points along a square.
const COONS_POINTS: &str = "0 0  0 3  0 6  0 10  3 10  6 10  10 10  10 6  10 3  10 0  6 0  3 0";
/// A new patch: a flag of 0, the twelve points, four gray corners.
const COONS: &str =
    "0  0 0  0 3  0 6  0 10  3 10  6 10  10 10  10 6  10 3  10 0  6 0  3 0  0 0.3 0.6 1";
/// Eight points of a patch sharing an edge with the one before.
const COONS_MORE_POINTS: &str = "13 10  16 10  20 10  20 6  20 3  20 0  16 0  13 0";
/// That patch: a flag of 1, the eight points, two corners.
const COONS_MORE: &str = "1  13 10  16 10  20 10  20 6  20 3  20 0  16 0  13 0  0.5 0.5";

#[test]
fn patch_mesh_structure() {
    let dict = format!(
        "<< /ShadingType 6 /ColorSpace /DeviceGray /DataSource [ {COONS} {COONS_MORE} ] >>"
    );
    let s = shading(&dict).unwrap();
    let elements = mesh_elements(&s).unwrap();
    assert_eq!(elements.len(), 2);
    let MeshElement::Patch {
        flag,
        points,
        color,
    } = &elements[0]
    else {
        panic!("patch");
    };
    assert_eq!((*flag, points.len(), color.len()), (0, 12, 4));
    assert!((points[3].1 - 10.0).abs() < 1e-4);
    assert!((color[1][0] - 0.3).abs() < 1e-4);
    let MeshElement::Patch {
        flag,
        points,
        color,
    } = &elements[1]
    else {
        panic!("patch");
    };
    assert_eq!((*flag, points.len(), color.len()), (1, 8, 2));
    // A tensor patch takes sixteen points, then twelve.
    let tensor = format!("0  {COONS_POINTS}  3 3  6 3  6 6  3 6  0 0.3 0.6 1");
    let more = format!("1  {COONS_MORE_POINTS}  13 3  16 3  16 6  13 6  0.5 0.5");
    let dict =
        format!("<< /ShadingType 7 /ColorSpace /DeviceGray /DataSource [ {tensor} {more} ] >>");
    let s = shading(&dict).unwrap();
    let elements = mesh_elements(&s).unwrap();
    assert!(
        matches!(&elements[0], MeshElement::Patch { points, color, .. } if points.len() == 16 && color.len() == 4)
    );
    assert!(
        matches!(&elements[1], MeshElement::Patch { flag: 1, points, color } if points.len() == 12 && color.len() == 2)
    );
    // Packed at eight bits: a flag, 24 coordinates, 4 components.
    let mut hex = String::from("00");
    for (x, y) in [
        (0, 0),
        (0, 3),
        (0, 6),
        (0, 10),
        (3, 10),
        (6, 10),
        (10, 10),
        (10, 6),
        (10, 3),
        (10, 0),
        (6, 0),
        (3, 0),
    ] {
        hex.push_str(&format!("{x:02X}{y:02X}"));
    }
    hex.push_str("00 40 80 FF");
    let dict = format!(
        "<< /ShadingType 6 /ColorSpace /DeviceGray /BitsPerCoordinate 8 /BitsPerComponent 8 /BitsPerFlag 8 \
         /Decode [0 255 0 255 0 1] /DataSource <{hex}> >>"
    );
    let s = shading(&dict).unwrap();
    assert!(
        matches!(&mesh_elements(&s).unwrap()[0], MeshElement::Patch { flag: 0, points, color }
        if points.len() == 12 && points[6] == (10.0, 10.0) && color[3] == vec![1.0])
    );
    for (ty, data) in [
        // No patch at all, a partial patch, a first patch that shares an
        // edge, a partial continuation.
        (6, String::new()),
        (6, COONS[..COONS.len() - 2].to_string()),
        (6, COONS_MORE.to_string()),
        (6, format!("{COONS} {COONS_MORE} 1 13")),
        (6, format!("{COONS} 4 {COONS_MORE}")),
        (7, COONS.to_string()),
    ] {
        let dict =
            format!("<< /ShadingType {ty} /ColorSpace /DeviceGray /DataSource [ {data} ] >>");
        assert_eq!(shading(&dict), Err(VmError::RangeCheck), "{dict}");
    }
}

#[test]
fn packed_mesh_entries_are_checked() {
    let s = shading(&mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))).unwrap();
    assert_eq!(s.kind.shading_type(), 4);
    for (dict, error) in [
        (mesh4(""), VmError::Undefined),
        (mesh4("/DataSource 1"), VmError::TypeCheck),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}")).replace("/BitsPerCoordinate 8 ", ""),
            VmError::Undefined,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}")).replace("/BitsPerComponent 8 ", ""),
            VmError::Undefined,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}")).replace("/BitsPerFlag 8 ", ""),
            VmError::Undefined,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("/Decode [0 255 0 255 0 1 0 1 0 1] ", ""),
            VmError::Undefined,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("/BitsPerCoordinate 8", "/BitsPerCoordinate 3"),
            VmError::RangeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("/BitsPerComponent 8", "/BitsPerComponent 24"),
            VmError::RangeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("/BitsPerFlag 8", "/BitsPerFlag 1"),
            VmError::RangeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("/BitsPerFlag 8", "/BitsPerFlag 8.0"),
            VmError::TypeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("[0 255 0 255 0 1 0 1 0 1]", "[0 255 0 255 0 1]"),
            VmError::RangeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE}"))
                .replace("[0 255 0 255 0 1 0 1 0 1]", "(x)"),
            VmError::TypeCheck,
        ),
        // With a function the decode array has one colour pair.
        (
            mesh4(&format!(
                "/DataSource {PACKED_TRIANGLE} /Function << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>"
            )),
            VmError::RangeCheck,
        ),
        (
            mesh4(&format!("/DataSource {PACKED_TRIANGLE} /Function 1")),
            VmError::TypeCheck,
        ),
        (
            mesh4(&format!(
                "/DataSource {PACKED_TRIANGLE} /ReusableStreamDecode filter dup closefile"
            )),
            VmError::IoError,
        ),
    ] {
        assert_eq!(shading(&dict), Err(error), "{dict}");
    }
    // A function-driven packed mesh: one value per vertex.
    let s = shading(
        "<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 /BitsPerComponent 8 /BitsPerFlag 8 \
         /Decode [0 255 0 255 0 1] /Function << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >> \
         /DataSource <00 00 00 00  00 FF 00 80  00 00 FF FF> >>",
    )
    .unwrap();
    assert_eq!(s.values_per_color(), 1);
    assert_eq!(s.kind.function().len(), 1);
    let elements = mesh_elements(&s).unwrap();
    assert!(
        matches!(&elements[1], MeshElement::Vertex { color, .. } if (color[0] - 128.0 / 255.0).abs() < 1e-6)
    );
}

// --- shfill and smoothness at the boundary ----------------------------------------

/// Runs `program` against the recording backend: the outcome, the
/// output, and the calls.
fn with_backend(program: &str) -> (Outcome, String, Vec<Call>) {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    let calls = log.borrow().clone();
    (outcome, out.text(), calls)
}

#[test]
fn shfill_hands_the_checked_shading_over_and_touches_nothing_else() {
    let program = format!(
        "3 4 moveto 0.25 setgray {} shfill currentpoint = = currentgray =",
        axial("/Background [1 1 0]")
    );
    let (outcome, output, calls) = with_backend(&program);
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "4.0\n3.0\n0.25\n");
    let at = calls
        .iter()
        .position(|c| matches!(c, Call::Shade(_)))
        .expect("the backend is asked to shade");
    let Call::Shade(spec) = &calls[at] else {
        unreachable!("found above");
    };
    // The background travels with the value: the backend keeps one
    // shading for both uses and knows this one ignores it.
    assert_eq!(spec.background, Some(vec![1.0, 1.0, 0.0]));
    assert_eq!(spec.kind.shading_type(), 2);
    assert!(
        !calls[at + 1..]
            .iter()
            .any(|c| matches!(c, Call::NewPath | Call::Fill | Call::Color(_))),
        "{calls:?}"
    );
    // An operand that is not a dictionary is typecheck, and a shading
    // the reader refuses fails with the reader's error, the operand
    // still on the stack.
    let (outcome, _, _) = with_backend("5 shfill");
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "typecheck"));
    let (outcome, output, _) = with_backend(
        "{ << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 1] >> shfill } stopped \
         { $error /errorname get = count = } if",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "rangecheck\n1\n");
    // Inside an uncoloured cell the operator is undefined, as image is.
    let (outcome, _, _) = with_backend(&format!(
        "<< /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /PaintProc {{ pop {} shfill }} >> matrix makepattern \
         [/Pattern /DeviceGray] setcolorspace 0.5 exch setcolor 0 0 5 5 rectfill",
        axial("")
    ));
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "undefined"));
}

#[test]
fn smoothness_is_clamped_and_read_back() {
    let (outcome, output, calls) = with_backend(
        "currentsmoothness = 0.3 setsmoothness currentsmoothness = \
         5 setsmoothness currentsmoothness = -2 setsmoothness currentsmoothness = \
         (x) setsmoothness",
    );
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "typecheck"));
    assert_eq!(output, "0.02\n0.3\n1.0\n0.0\n");
    let set: Vec<f32> = calls
        .iter()
        .filter_map(|c| match c {
            Call::Smoothness(value) => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(set, [0.3, 1.0, 0.0]);
}
