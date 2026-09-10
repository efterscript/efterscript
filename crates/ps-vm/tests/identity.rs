// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The printer-identity mechanism: `statusdict` and `serverdict`,
//! identity seeding, the prelude and its failure, `exitserver`, the
//! implicit resource categories, and the screen and transfer setters
//! without a backend. The printed-output scenarios are corpus files
//! under `corpus/unit/identity`.

use ps_vm::{Capture, Config, Interp, Io, MarkValue, Outcome, PreludeError, SliceSource};

fn configured(identity: Vec<(&str, MarkValue)>, prelude: Option<&str>) -> (Interp, Capture) {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        identity: identity
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        prelude: prelude.map(|p| p.as_bytes().to_vec()),
        ..Default::default()
    };
    (Interp::try_with_config(config).expect("prelude runs"), out)
}

fn run(interp: &mut Interp, program: &str) -> Outcome {
    interp.run(&mut SliceSource::new(program.as_bytes()))
}

fn string(text: &str) -> MarkValue {
    MarkValue::String(text.as_bytes().to_vec())
}

// --- seeding and the prelude ---------------------------------------------------

#[test]
fn seeded_identity_is_read_back() {
    let (mut interp, out) = configured(
        vec![
            ("product", string("Fictional Press")),
            ("manualfeed", MarkValue::Bool(false)),
            (
                "sizes",
                MarkValue::Array(vec![MarkValue::Int(612), MarkValue::Int(792)]),
            ),
            (
                "tray",
                MarkValue::Dict(vec![(b"name".to_vec(), MarkValue::Name(b"Upper".to_vec()))]),
            ),
        ],
        None,
    );
    let outcome = run(
        &mut interp,
        "statusdict /product get = statusdict /manualfeed get = \
         statusdict /sizes get 1 get = statusdict /tray get /name get =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "Fictional Press\nfalse\n792\nUpper\n");
    assert_eq!(
        interp.statusdict_entries(),
        vec![
            ("product".to_string(), "(Fictional Press)".to_string()),
            (
                "version".to_string(),
                format!("({})", env!("CARGO_PKG_VERSION"))
            ),
            ("revision".to_string(), "0".to_string()),
            ("manualfeed".to_string(), "false".to_string()),
            ("sizes".to_string(), "[612 792]".to_string()),
            ("tray".to_string(), "-dict-".to_string()),
        ]
    );
    assert!(!interp.prelude_ran());
    assert!(!interp.server_level());
}

#[test]
fn the_prelude_defines_the_device() {
    let prelude = "statusdict begin /waittimeout 300 def \
                   /setpage { pop 2 array astore << /PageSize 3 -1 roll >> setpagedevice } def \
                   end /hostname (press) def";
    let (mut interp, out) = configured(Vec::new(), Some(prelude));
    assert!(interp.prelude_ran());
    assert!(!interp.server_level());
    let outcome = run(
        &mut interp,
        "statusdict /waittimeout get = 612 1008 0 statusdict /setpage get exec \
         currentpagedevice /PageSize get 1 get = hostname =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "300\n1008\npress\n");
    assert_eq!(
        interp.statusdict_entries()[3],
        ("waittimeout".to_string(), "300".to_string())
    );
}

#[test]
fn the_prelude_runs_at_the_server_level_and_its_output_is_kept() {
    let (mut interp, out) = configured(Vec::new(), Some("(ready) = /lvl 1 def"));
    assert_eq!(out.text(), "ready\n");
    // The prelude's definition went into userdict, where the job finds it.
    assert_eq!(run(&mut interp, "lvl ="), Outcome::Ok);
    assert_eq!(out.text(), "ready\n1\n");
}

#[test]
fn a_failing_prelude_fails_construction() {
    let failed = |prelude: &str| {
        let config = Config {
            prelude: Some(prelude.as_bytes().to_vec()),
            ..Default::default()
        };
        Interp::try_with_config(config).err().expect("fails")
    };
    assert_eq!(
        failed("1 0 div"),
        PreludeError {
            name: "undefinedresult".to_string(),
            offending: "div".to_string(),
        }
    );
    assert_eq!(
        failed("nosuchname"),
        PreludeError {
            name: "undefined".to_string(),
            offending: "nosuchname".to_string(),
        }
    );
    assert_eq!(
        failed("1 0 div").to_string(),
        "prelude failed: undefinedresult in div"
    );
    // A caught error is no failure, and a quit ends the prelude alone.
    let (mut interp, _) = configured(Vec::new(), Some("{ 1 0 div } stopped pop quit"));
    assert!(!interp.has_quit());
    assert_eq!(run(&mut interp, "1"), Outcome::Ok);
}

#[test]
fn an_identity_value_the_vm_cannot_hold_fails_construction() {
    let config = Config {
        identity: vec![("x".repeat(200), MarkValue::Int(1))],
        ..Default::default()
    };
    let error = Interp::try_with_config(config).err().expect("fails");
    assert_eq!(error.name, "limitcheck");
    assert_eq!(error.offending, "x".repeat(200));
}

#[test]
#[should_panic(expected = "prelude failed: undefinedresult in div")]
fn with_config_panics_on_a_failing_prelude() {
    let _ = Interp::with_config(Config {
        prelude: Some(b"1 0 div".to_vec()),
        ..Default::default()
    });
}

// --- exitserver ------------------------------------------------------------------

#[test]
fn exitserver_checks_the_configured_password() {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        server_password: 4321,
        ..Default::default()
    });
    let outcome = run(&mut interp, "serverdict begin 0 exitserver");
    assert!(matches!(&outcome, Outcome::Error(e) if e.name == "invalidaccess"));
    assert!(!interp.server_level());
    let outcome = run(&mut interp, "serverdict begin 4321 exitserver /x 1 def");
    assert_eq!(outcome, Outcome::Ok);
    assert!(interp.server_level());
    // The dictionary stack is back at the permanent dictionaries, so the
    // definition after exitserver landed in userdict.
    assert_eq!(interp.dstack().len(), 3);
    assert_eq!(run(&mut interp, "userdict /x known"), Outcome::Ok);
    assert_eq!(interp.ostack().last().and_then(|o| o.as_bool()), Some(true));
}

// --- implicit categories ------------------------------------------------------------

#[test]
fn every_claimed_font_type_defines() {
    let (mut interp, out) = configured(Vec::new(), None);
    let program = "\
        /Enc 256 array def 0 1 255 { Enc exch /.notdef put } for \
        /T1 << /FontType 1 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding Enc \
               /CharStrings 1 dict /Private 1 dict >> definefont /FontType get = \
        /T2 << /FontType 2 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding Enc >> \
               definefont /FontType get = \
        /T3 << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding Enc \
               /BuildGlyph { pop pop } >> definefont /FontType get = \
        /T42 << /FontType 42 /FontMatrix [1 0 0 1 0 0] /Encoding Enc \
               /CharStrings 1 dict /sfnts [ <00010000000100> ] >> definefont /FontType get = \
        /T0 << /FontType 0 /FMapType 9 /FontMatrix [1 0 0 1 0 0] /CMap /Identity-H \
               /Encoding [0] /FDepVector [ /Helvetica findfont ] >> definefont /FontType get = \
        (*) { = } 8 string /FontType resourceforall";
    assert_eq!(run(&mut interp, program), Outcome::Ok);
    assert_eq!(out.text(), "1\n2\n3\n42\n0\n0\n1\n2\n3\n42\n");
}

#[test]
fn every_listed_category_resolves() {
    let (mut interp, out) = configured(Vec::new(), None);
    let program = "\
        (*) { cvn dup /Category findresource eq { (ok) = } { (bad) = } ifelse } \
        32 string /Category resourceforall \
        (*) { cvn /Category resourcestatus pop pop pop } 32 string /Category resourceforall \
        count = \
        /Font /Category findresource == \
        /Category /Category findresource == \
        /Generic /Category resourcestatus = = = \
        /NoSuch /Category resourcestatus = \
        /NoSuch /Category findresource";
    let outcome = run(&mut interp, program);
    assert!(matches!(&outcome, Outcome::Error(e) if e.name == "undefined"));
    assert_eq!(
        out.text(),
        "ok\n".repeat(12) + "0\n/Font\n/Category\ntrue\n0\n0\nfalse\n"
    );
}

#[test]
fn implicit_members_cannot_be_changed_and_other_key_types_are_absent() {
    let (mut interp, out) = configured(Vec::new(), None);
    let program = "\
        { 42 /FontType undefineresource } stopped pop $error /errorname get = \
        { /Foo 1 dict /Generic defineresource } stopped pop $error /errorname get = \
        (42) /FontType resourcestatus = \
        /DeviceRGB /FontType resourcestatus = \
        (*) { = } 8 string /Filter resourceforall \
        (*) { = } 8 string /Generic resourceforall \
        (Device*) { = } 32 string /ColorSpaceFamily resourceforall";
    assert_eq!(run(&mut interp, program), Outcome::Ok);
    assert_eq!(
        out.text(),
        "invalidaccess\ninvalidaccess\nfalse\nfalse\nDeviceCMYK\nDeviceGray\nDeviceN\nDeviceRGB\n"
    );
}

// --- screens and transfers without a backend ---------------------------------------

#[test]
fn screens_and_transfers_are_kept_without_a_backend() {
    let (mut interp, out) = configured(Vec::new(), None);
    let program = "\
        currentscreen pop pop = \
        currenttransfer == \
        /spot { pop } def 100 30 /spot load setscreen \
        currentscreen /spot load eq = = = \
        currentcolorscreen pop pop = 9 { pop } repeat \
        { 1 exch sub } settransfer currenttransfer == \
        { } { } { } { 0.5 mul } setcolortransfer currentcolortransfer == pop pop pop \
        currenttransfer == \
        [ 1 0 0 1 0 0 ] 8 8 { } framedevice \
        { 60 45 (x) setscreen } stopped pop $error /errorname get =";
    assert_eq!(run(&mut interp, program), Outcome::Ok);
    assert_eq!(
        out.text(),
        "60.0\n{}\ntrue\n30.0\n100.0\n100.0\n{1 exch sub}\n{0.5 mul}\n{0.5 mul}\ntypecheck\n"
    );
    assert_eq!(interp.screens()[3].frequency, 100.0);
    // Three `{ }` literals are three procedures; the gray one is another.
    let transfers = interp.transfers();
    assert_ne!(transfers[0], transfers[3]);
    assert!(interp.graphics_proc(transfers[3]).is_some());
}
