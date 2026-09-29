// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Interpreter parameters where a corpus file cannot reach: the prelude
//! changing system parameters without a password, the passwords seeded
//! from the configured server password, and `status` through a file
//! capability. The printed-output scenarios are corpus files under
//! `corpus/unit/params`.

use efterscript_vm::{
    Capabilities, Capture, Config, FileCapability, FileStatus, Interp, Io, Outcome, SliceSource,
    Stream, VmError,
};

fn configured(prelude: Option<&str>, server_password: i32) -> (Interp, Capture) {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        prelude: prelude.map(|p| p.as_bytes().to_vec()),
        server_password,
        ..Default::default()
    };
    (Interp::try_with_config(config).expect("prelude runs"), out)
}

fn run(interp: &mut Interp, program: &str) -> Outcome {
    interp.run(&mut SliceSource::new(program.as_bytes()))
}

#[test]
fn the_prelude_sets_system_parameters_without_a_password() {
    let (mut interp, out) = configured(
        Some("<< /PrinterName (Front Office) /MaxFontCache 123 >> setsystemparams"),
        0,
    );
    let outcome = run(
        &mut interp,
        "currentsystemparams dup /PrinterName get = /MaxFontCache get =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "Front Office\n123\n");
    // The job itself still needs the password.
    let outcome = run(&mut interp, "<< /MaxFontCache 1 >> setsystemparams");
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "invalidaccess"));
}

#[test]
fn printer_name_defaults_to_the_product() {
    let (mut interp, out) = configured(None, 0);
    let outcome = run(
        &mut interp,
        "currentsystemparams /PrinterName get product eq = \
         << /Password 0 /PrinterName (Here) >> setsystemparams \
         currentsystemparams /PrinterName get = \
         << /Password 0 /PrinterName () >> setsystemparams \
         currentsystemparams /PrinterName get product eq =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "true\nHere\ntrue\n");
}

#[test]
fn the_passwords_follow_the_configured_server_password() {
    let (mut interp, out) = configured(None, 4321);
    let outcome = run(
        &mut interp,
        "{ << /Password 0 /MaxFontCache 1 >> setsystemparams } stopped = \
         << /Password (4321) /MaxFontCache 1 >> setsystemparams \
         serverdict begin 4321 exitserver (in) =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "true\nin\n");
}

#[test]
fn an_empty_password_disables_checking() {
    let (mut interp, out) = configured(None, 0);
    let outcome = run(
        &mut interp,
        "<< /Password 0 /SystemParamsPassword () >> setsystemparams \
         << /MaxFontCache 77 >> setsystemparams \
         currentsystemparams /MaxFontCache get =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "77\n");
}

#[test]
fn factory_defaults_alone_needs_no_password() {
    let (mut interp, _) = configured(None, 0);
    let outcome = run(&mut interp, "<< /FactoryDefaults true >> setsystemparams");
    assert_eq!(outcome, Outcome::Ok);
}

struct OneFile;

impl FileCapability for OneFile {
    fn open(&mut self, _: &[u8], _: &[u8]) -> Result<Box<dyn Stream>, VmError> {
        Err(VmError::UndefinedFileName)
    }

    fn status(&mut self, name: &[u8]) -> Option<FileStatus> {
        (name == b"fonts/Known").then_some(FileStatus {
            pages: 2,
            bytes: 2000,
            referenced: 7,
            created: 5,
        })
    }
}

#[test]
fn status_asks_the_file_capability() {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        capabilities: Capabilities {
            file: Some(Box::new(OneFile)),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = run(
        &mut interp,
        "(fonts/Known) status = = = = = (fonts/Other) status =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "true\n5\n7\n2000\n2\nfalse\n");
}
