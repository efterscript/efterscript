// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `statusdict`, `serverdict`, and the device operators a printer's setup
//! code reaches first: `exitserver` (PLRM3 §3.7.7) and `framedevice`. The
//! dictionaries hold this interpreter's own identity and nothing else
//! until the embedder seeds them; a host that presents the interpreter as
//! a device supplies the entries, as data or through a prelude.

use crate::error::VmError;
use crate::graphics::MarkValue;
use crate::interp::Interp;
use crate::ops::distiller::object;
use crate::ops::output::{brief, full};

op_table! { OPS {
    "framedevice" => framedevice, [Array, Int, Int, Array];
}}

op_table! { server SERVER_OPS {
    "exitserver" => exitserver, [Int];
}}

/// The interpreter's own identity: what `statusdict` holds by default.
pub fn default_identity() -> Vec<(String, MarkValue)> {
    vec![
        (
            "product".to_string(),
            MarkValue::String(b"EfterScript".to_vec()),
        ),
        (
            "version".to_string(),
            MarkValue::String(env!("CARGO_PKG_VERSION").as_bytes().to_vec()),
        ),
        ("revision".to_string(), MarkValue::Int(0)),
    ]
}

/// Fills the fresh `statusdict` with the default identity.
pub(crate) fn seed(i: &mut Interp) -> Result<(), VmError> {
    seed_identity(i, &default_identity()).map_err(|(e, _)| e)
}

/// The identity keys `systemdict` defines as well (PLRM3 §8.2 entries
/// `product`, `version`, `revision`, `serialnumber`): an entry under one
/// of these is written to both dictionaries, so the operators and the
/// `statusdict` lookups a driver makes answer alike.
const SYSTEMDICT_IDENTITY: [&str; 4] = ["product", "version", "revision", "serialnumber"];

/// Writes `entries` into `statusdict`, in the current (local) VM, and
/// the identity keys into `systemdict` too; a value the VM cannot hold,
/// or a `serialnumber` that is not an integer, names the key it failed
/// on.
pub(crate) fn seed_identity(
    i: &mut Interp,
    entries: &[(String, MarkValue)],
) -> Result<(), (VmError, String)> {
    let dict = i.dicts().statusdict;
    let systemdict = i.dicts().systemdict;
    for (key, value) in entries {
        if key == "serialnumber" && !matches!(value, MarkValue::Int(_)) {
            return Err((VmError::TypeCheck, key.clone()));
        }
        let stored = i
            .mem
            .intern(key.as_bytes())
            .map_err(|_| VmError::LimitCheck)
            .and_then(|key| {
                let value = object(i, value, 0)?;
                i.mem.dict_put(dict, key, value)?;
                Ok((key, value))
            });
        let (name, value) = stored.map_err(|e| (e, key.clone()))?;
        if SYSTEMDICT_IDENTITY.contains(&key.as_str()) {
            // systemdict is read-only and global; these objects predate
            // every save, so the raw insert is safe (as for the
            // graphics operators).
            i.mem
                .dict_mut(systemdict)
                .expect("systemdict exists")
                .insert(name, value);
        }
    }
    Ok(())
}

/// The `statusdict` entries in insertion order: keys as `=` writes them,
/// values as `==` does.
pub(crate) fn entries(i: &Interp) -> Vec<(String, String)> {
    let dict = i.dicts().statusdict;
    i.mem
        .dict_entries(dict)
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| {
            let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
            (text(brief(i, key)), text(full(i, value)))
        })
        .collect()
}

/// `password exitserver`: `invalidaccess` unless the password is the
/// configured one; then execution continues at the server level, so what
/// follows persists for the interpreter's life.
fn exitserver(i: &mut Interp) -> Result<(), VmError> {
    let password = i.peek(0)?.as_i32().ok_or(VmError::TypeCheck)?;
    if password != i.server_password() {
        return Err(VmError::InvalidAccess);
    }
    i.pop()?;
    i.enter_server_level();
    Ok(())
}

/// `matrix width height proc framedevice`: a Level 1 raster-device setup
/// with no meaning for a device that rasterises nothing; the operands
/// are checked and consumed.
fn framedevice(i: &mut Interp) -> Result<(), VmError> {
    let procedure = i.peek(0)?;
    if !procedure.is_executable() {
        return Err(VmError::TypeCheck);
    }
    for n in [1, 2] {
        if i.peek(n)?.as_i32().is_some_and(|v| v < 0) {
            return Err(VmError::RangeCheck);
        }
    }
    crate::ops::graphics::read_matrix(i, i.peek(3)?)?;
    for _ in 0..4 {
        i.pop()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_identity_is_this_interpreter() {
        let identity = default_identity();
        assert_eq!(identity[0].0, "product");
        assert_eq!(identity[0].1, MarkValue::String(b"EfterScript".to_vec()));
        assert_eq!(identity[1].0, "version");
        assert_eq!(identity[2], ("revision".to_string(), MarkValue::Int(0)));
    }

    #[test]
    fn framedevice_wants_its_operand_shapes() {
        let mut interp = Interp::new();
        let outcome = interp.run(&mut crate::SliceSource::new(
            b"[1 0 0 1 0 0] 8 8 { } framedevice count",
        ));
        assert_eq!(outcome, crate::Outcome::Ok);
        assert_eq!(interp.ostack()[0].as_i32(), Some(0));
        let outcome = interp.run(&mut crate::SliceSource::new(
            b"[1 0 0 1 0] 8 8 { } framedevice",
        ));
        assert!(matches!(outcome, crate::Outcome::Error(e) if e.name == "rangecheck"));
        let outcome = interp.run(&mut crate::SliceSource::new(
            b"[1 0 0 1 0 0] -1 8 { } framedevice",
        ));
        assert!(matches!(outcome, crate::Outcome::Error(e) if e.name == "rangecheck"));
        let outcome = interp.run(&mut crate::SliceSource::new(
            b"[1 0 0 1 0 0] 8 8 [ ] framedevice",
        ));
        assert!(matches!(outcome, crate::Outcome::Error(e) if e.name == "typecheck"));
    }
}
