// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Interpreter parameters (PLRM3 Appendix C, §8.2): user parameters,
//! system parameters and their passwords, device parameters, and the
//! font-cache and VM operators that read and write the same values.
//!
//! User parameters live in a dictionary in local VM made at
//! construction, so `restore` reverts them to their values at the
//! matching `save`. System parameters live outside VM, in
//! [`SystemParams`], so no `restore` reverts them. No device parameter
//! set is defined. Parameters that govern caches, collection, or
//! halftoning are recorded and have no effect: nothing is cached,
//! collected, or rasterised.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::{Object, Type};
use crate::ops::vm::VM_MAXIMUM;

op_table! { OPS {
    "setuserparams" => setuserparams, [Dict];
    "currentuserparams" => currentuserparams;
    "setsystemparams" => setsystemparams, [Dict];
    "currentsystemparams" => currentsystemparams;
    "setdevparams" => setdevparams, [String, Dict];
    "currentdevparams" => currentdevparams, [String];
    "cachestatus" => cachestatus;
    "setcachelimit" => setcachelimit, [Int];
    "setcacheparams" => setcacheparams;
    "currentcacheparams" => currentcacheparams;
    "setvmthreshold" => setvmthreshold, [Int];
    "vmreclaim" => vmreclaim, [Int];
}}

/// `JobName` is truncated to this many bytes (§C.2, Table C.1).
const JOB_NAME_MAX: usize = 100;

/// What `cachestatus` reports as the font/matrix and glyph limits; the
/// consumptions beside them are zero.
const CACHE_MATRICES_MAX: i32 = 100;
const CACHE_GLYPHS_MAX: i32 = 800;

/// A user parameter's kind, which decides what a request may set it to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum User {
    Bool,
    /// Stored as given; a negative value is stored as 0.
    Count,
    /// Stored as given within an inclusive range, else the nearest end.
    Ranged(i32, i32),
    /// Reported, and kept whatever a request asks.
    Fixed,
    JobName,
}

/// Table C.1 keys with their kinds and defaults. The stack and VM limits
/// are filled from the interpreter at construction.
const USER_PARAMS: [(&str, User, i32); 17] = [
    ("AccurateScreens", User::Bool, 0),
    ("HalftoneMode", User::Ranged(0, 2), 0),
    ("IdiomRecognition", User::Fixed, 0),
    ("JobName", User::JobName, 0),
    ("MaxDictStack", User::Fixed, 0),
    ("MaxExecStack", User::Fixed, 0),
    ("MaxFontItem", User::Count, 12500),
    ("MaxFormItem", User::Count, 100_000),
    ("MaxLocalVM", User::Fixed, 0),
    ("MaxOpStack", User::Fixed, 0),
    ("MaxPatternItem", User::Count, 20000),
    ("MaxScreenItem", User::Count, 65536),
    ("MaxSuperScreen", User::Count, 1016),
    ("MaxUPathItem", User::Count, 5000),
    ("MinFontCompress", User::Count, 100),
    ("VMReclaim", User::Ranged(-2, 0), 0),
    ("VMThreshold", User::Count, 40000),
];

/// The writable integer system parameters and their defaults.
const SYSTEM_LIMITS: [(&str, i32); 11] = [
    ("MaxDisplayAndSourceList", 1_000_000),
    ("MaxDisplayList", 500_000),
    ("MaxFontCache", 400_000),
    ("MaxFormCache", 200_000),
    ("MaxImageBuffer", 500_000),
    ("MaxOutlineCache", 100_000),
    ("MaxPatternCache", 100_000),
    ("MaxScreenStorage", 100_000),
    ("MaxSourceList", 500_000),
    ("MaxStoredScreenCache", 0),
    ("MaxUPathCache", 100_000),
];

/// The read-only current-consumption system parameters, all zero.
const SYSTEM_CURRENT: [&str; 9] = [
    "CurDisplayList",
    "CurFontCache",
    "CurFormCache",
    "CurOutlineCache",
    "CurPatternCache",
    "CurScreenStorage",
    "CurSourceList",
    "CurStoredScreenCache",
    "CurUPathCache",
];

/// The resource-location system parameters (§C.3.6): `%null` names a
/// product with no external resources.
const RESOURCE_DIRS: [(&str, &[u8]); 3] = [
    ("FontResourceDir", b"%null"),
    ("GenericResourceDir", b"%null"),
    ("GenericResourcePathSep", b"/"),
];

/// System parameters and the two passwords, held outside VM so no
/// `restore` reverts them (§C.1.2).
#[derive(Clone, Debug)]
pub struct SystemParams {
    limits: [i32; SYSTEM_LIMITS.len()],
    /// `None` reports the `product` in `systemdict`.
    printer_name: Option<Vec<u8>>,
    startup_mode: i32,
    factory_defaults: bool,
    resource_dirs: [Vec<u8>; RESOURCE_DIRS.len()],
    system_password: Vec<u8>,
    start_job_password: Vec<u8>,
}

impl SystemParams {
    /// Both passwords are the configured server password, written as
    /// `cvs` writes an integer.
    pub(crate) fn new(server_password: i32) -> Self {
        let password = server_password.to_string().into_bytes();
        SystemParams {
            limits: SYSTEM_LIMITS.map(|(_, value)| value),
            printer_name: None,
            startup_mode: 0,
            factory_defaults: false,
            resource_dirs: RESOURCE_DIRS.map(|(_, value)| value.to_vec()),
            system_password: password.clone(),
            start_job_password: password,
        }
    }

    fn limit(&self, key: &str) -> i32 {
        SYSTEM_LIMITS
            .iter()
            .position(|(k, _)| *k == key)
            .map_or(0, |at| self.limits[at])
    }

    /// Whether `password` equals `StartJobPassword` or
    /// `SystemParamsPassword` (§C.3.1).
    pub(crate) fn start_job_allowed(&self, password: &[u8]) -> bool {
        password == self.start_job_password || password == self.system_password
    }
}

/// A password operand: a string, or an integer as `cvs` would write it.
/// Anything else is `typecheck`. A string stops at its first null byte.
pub(crate) fn password_bytes(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    match object.ty() {
        Type::Integer => Ok(object
            .as_i32()
            .expect("an integer")
            .to_string()
            .into_bytes()),
        Type::String => Ok(until_null(
            i.mem.string(object).ok_or(VmError::InvalidAccess)?,
        )),
        _ => Err(VmError::TypeCheck),
    }
}

fn until_null(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].to_vec()
}

/// Runs `f` with local allocation, restoring the caller's mode.
fn in_local<T>(i: &mut Interp, f: impl FnOnce(&mut Interp) -> T) -> T {
    let mode = i.mem.current_global();
    i.mem.set_global(false);
    let result = f(i);
    i.mem.set_global(mode);
    result
}

fn name(i: &mut Interp, text: &str) -> Result<Object, VmError> {
    i.mem
        .intern(text.as_bytes())
        .map_err(|_| VmError::LimitCheck)
}

/// The text of a request key, or `None` for a key that is not a name
/// (ignored, like an unknown name).
fn key_text(i: &Interp, key: Object) -> Option<String> {
    let atom = key.as_name()?;
    Some(String::from_utf8_lossy(i.mem.name_text(atom)).into_owned())
}

// --- user parameters -------------------------------------------------------

/// Fills the user-parameter dictionary with the defaults; called once at
/// construction, before any `save`.
pub(crate) fn seed(i: &mut Interp) -> Result<(), VmError> {
    let limits = i.limits();
    let fixed = |key: &str| -> i32 {
        let as_i32 = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        match key {
            "MaxDictStack" => as_i32(limits.dict),
            "MaxExecStack" => as_i32(limits.exec),
            "MaxOpStack" => as_i32(limits.operand),
            "MaxLocalVM" => VM_MAXIMUM,
            _ => 0,
        }
    };
    let dict = i.user_params;
    for (key, kind, default) in USER_PARAMS {
        let value = match kind {
            User::Bool => Object::boolean(default != 0),
            User::JobName => in_local(i, |i| i.mem.alloc_string(Vec::new())),
            User::Fixed if key == "IdiomRecognition" => Object::boolean(false),
            User::Fixed => Object::integer(fixed(key)),
            User::Count | User::Ranged(..) => Object::integer(default),
        };
        let key = name(i, key)?;
        i.mem.dict_put(dict, key, value)?;
    }
    Ok(())
}

fn user_kind(key: &str) -> Option<User> {
    USER_PARAMS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, kind, _)| *kind)
}

/// `typecheck` unless `value` suits `kind`.
fn check_user(kind: User, value: Object) -> Result<(), VmError> {
    let ok = match kind {
        User::Bool => value.ty() == Type::Boolean,
        User::Count | User::Ranged(..) => value.ty() == Type::Integer,
        User::JobName => value.ty() == Type::String,
        // Reported only; any value is accepted and ignored.
        User::Fixed => true,
    };
    if ok { Ok(()) } else { Err(VmError::TypeCheck) }
}

/// The value `kind` stores for `value`, already checked; `None` when the
/// parameter keeps its value.
fn user_value(i: &mut Interp, kind: User, value: Object) -> Result<Option<Object>, VmError> {
    Ok(match kind {
        User::Fixed => None,
        User::Bool => Some(value),
        User::Count => Some(Object::integer(value.as_i32().expect("checked").max(0))),
        User::Ranged(low, high) => Some(Object::integer(
            value.as_i32().expect("checked").clamp(low, high),
        )),
        User::JobName => {
            let text = until_null(i.mem.string(value).ok_or(VmError::InvalidAccess)?);
            let text = text[..text.len().min(JOB_NAME_MAX)].to_vec();
            // A private copy in local VM: the parameter must not alias
            // the program's string, and the dictionary is local.
            Some(in_local(i, |i| i.mem.alloc_string(text)))
        }
    })
}

/// Applies `(key, value)` pairs to the user parameters: every pair is
/// checked before any is stored, so an error changes nothing.
fn set_user(i: &mut Interp, pairs: &[(Object, Object)]) -> Result<(), VmError> {
    let mut known = Vec::new();
    for &(key, value) in pairs {
        let Some(kind) = key_text(i, key).and_then(|text| user_kind(&text)) else {
            continue;
        };
        check_user(kind, value)?;
        known.push((key, kind, value));
    }
    let dict = i.user_params;
    for (key, kind, value) in known {
        if let Some(stored) = user_value(i, kind, value)? {
            i.mem.dict_put(dict, key, stored)?;
        }
    }
    Ok(())
}

/// One user parameter as an integer, 0 when it is not an integer.
fn user_int(i: &mut Interp, key: &str) -> Result<i32, VmError> {
    let key = name(i, key)?;
    let value = i.mem.dict_get(i.user_params, key)?;
    Ok(value.and_then(Object::as_i32).unwrap_or(0))
}

fn set_user_int(i: &mut Interp, key: &str, value: i32) -> Result<(), VmError> {
    let key = name(i, key)?;
    set_user(i, &[(key, Object::integer(value))])
}

fn setuserparams(i: &mut Interp) -> Result<(), VmError> {
    let pairs = i.mem.dict_entries(i.peek(0)?)?;
    set_user(i, &pairs)?;
    i.pop()?;
    Ok(())
}

/// A new dictionary each time, in the current allocation mode, holding
/// copies of the string values.
fn currentuserparams(i: &mut Interp) -> Result<(), VmError> {
    let entries = i.mem.dict_entries(i.user_params)?;
    let copy = i
        .mem
        .new_dict(u32::try_from(entries.len()).map_err(|_| VmError::LimitCheck)?);
    for (key, value) in entries {
        let value = if value.ty() == Type::String {
            let bytes = i.mem.string(value).unwrap_or_default().to_vec();
            i.mem.alloc_string(bytes)
        } else {
            value
        };
        i.mem.dict_put(copy, key, value)?;
    }
    i.push(copy)
}

// --- system parameters -----------------------------------------------------

/// Whether a `setsystemparams` or `setdevparams` request may change
/// anything (§C.1.2, §C.3.1): the right `Password`, an empty
/// system password, a request holding nothing but `FactoryDefaults`,
/// or the prelude running.
fn permitted(i: &Interp, pairs: &[(Object, Object)]) -> Result<bool, VmError> {
    let params = i.system_params();
    if params.system_password.is_empty() || i.prelude_running() {
        return Ok(true);
    }
    let mut password = None;
    let mut only_factory_defaults = true;
    for &(key, value) in pairs {
        match key_text(i, key).as_deref() {
            Some("Password") => password = Some(value),
            Some("FactoryDefaults") => {}
            _ => only_factory_defaults = false,
        }
    }
    if only_factory_defaults {
        return Ok(true);
    }
    match password {
        Some(value) => Ok(password_bytes(i, value)? == params.system_password),
        None => Ok(false),
    }
}

/// One accepted change to the system parameters.
enum SystemChange {
    Limit(usize, i32),
    PrinterName(Vec<u8>),
    StartupMode(i32),
    FactoryDefaults(bool),
    ResourceDir(usize, Vec<u8>),
    SystemPassword(Vec<u8>),
    StartJobPassword(Vec<u8>),
}

fn string_value(i: &Interp, value: Object) -> Result<Vec<u8>, VmError> {
    if value.ty() != Type::String {
        return Err(VmError::TypeCheck);
    }
    Ok(until_null(
        i.mem.string(value).ok_or(VmError::InvalidAccess)?,
    ))
}

fn int_value(value: Object) -> Result<i32, VmError> {
    value
        .as_i32()
        .filter(|_| value.ty() == Type::Integer)
        .ok_or(VmError::TypeCheck)
}

/// The change a request pair asks for; `None` for read-only, unknown,
/// and non-name keys, which are ignored.
fn system_change(i: &Interp, key: &str, value: Object) -> Result<Option<SystemChange>, VmError> {
    if let Some(at) = SYSTEM_LIMITS.iter().position(|(k, _)| *k == key) {
        return Ok(Some(SystemChange::Limit(at, int_value(value)?.max(0))));
    }
    if let Some(at) = RESOURCE_DIRS.iter().position(|(k, _)| *k == key) {
        return Ok(Some(SystemChange::ResourceDir(at, string_value(i, value)?)));
    }
    Ok(Some(match key {
        "PrinterName" => SystemChange::PrinterName(string_value(i, value)?),
        "StartupMode" => SystemChange::StartupMode(int_value(value)?.max(0)),
        "FactoryDefaults" => {
            if value.ty() != Type::Boolean {
                return Err(VmError::TypeCheck);
            }
            SystemChange::FactoryDefaults(value.as_bool().expect("a boolean"))
        }
        "SystemParamsPassword" => SystemChange::SystemPassword(password_bytes(i, value)?),
        "StartJobPassword" => SystemChange::StartJobPassword(password_bytes(i, value)?),
        _ => return Ok(None),
    }))
}

fn setsystemparams(i: &mut Interp) -> Result<(), VmError> {
    let pairs = i.mem.dict_entries(i.peek(0)?)?;
    if !permitted(i, &pairs)? {
        return Err(VmError::InvalidAccess);
    }
    let mut changes = Vec::new();
    for &(key, value) in &pairs {
        let Some(key) = key_text(i, key) else {
            continue;
        };
        if let Some(change) = system_change(i, &key, value)? {
            changes.push(change);
        }
    }
    let params = i.system_params_mut();
    for change in changes {
        match change {
            SystemChange::Limit(at, value) => params.limits[at] = value,
            SystemChange::PrinterName(name) => {
                params.printer_name = (!name.is_empty()).then_some(name);
            }
            SystemChange::StartupMode(mode) => params.startup_mode = mode,
            SystemChange::FactoryDefaults(on) => params.factory_defaults = on,
            SystemChange::ResourceDir(at, dir) => params.resource_dirs[at] = dir,
            SystemChange::SystemPassword(p) => params.system_password = p,
            SystemChange::StartJobPassword(p) => params.start_job_password = p,
        }
    }
    i.pop()?;
    Ok(())
}

/// A new dictionary each time, in the current allocation mode, holding
/// every system parameter but the two write-only passwords.
fn currentsystemparams(i: &mut Interp) -> Result<(), VmError> {
    let params = i.system_params().clone();
    let systemdict = i.dicts().systemdict;
    let lookup = |i: &mut Interp, key: &str| -> Result<Option<Object>, VmError> {
        let key = name(i, key)?;
        i.mem.dict_get(systemdict, key)
    };
    let revision = lookup(i, "revision")?.unwrap_or(Object::integer(0));
    let printer_name = match &params.printer_name {
        Some(text) => i.mem.alloc_string(text.clone()),
        None => {
            let product = lookup(i, "product")?;
            let text = product
                .and_then(|p| i.mem.string(p).map(<[u8]>::to_vec))
                .unwrap_or_default();
            i.mem.alloc_string(text)
        }
    };
    let real_format = i.mem.alloc_string(b"IEEE".to_vec());
    let page_count = i32::try_from(i.page_count()).unwrap_or(i32::MAX);
    let mut entries: Vec<(&str, Object)> = vec![
        ("ByteOrder", Object::boolean(false)),
        ("BuildTime", Object::integer(0)),
        ("FactoryDefaults", Object::boolean(params.factory_defaults)),
        ("PageCount", Object::integer(page_count)),
        ("PrinterName", printer_name),
        ("RealFormat", real_format),
        ("Revision", revision),
        ("StartupMode", Object::integer(params.startup_mode)),
    ];
    for key in SYSTEM_CURRENT {
        entries.push((key, Object::integer(0)));
    }
    for (at, (key, _)) in SYSTEM_LIMITS.iter().enumerate() {
        entries.push((key, Object::integer(params.limits[at])));
    }
    for (at, (key, _)) in RESOURCE_DIRS.iter().enumerate() {
        let dir = i.mem.alloc_string(params.resource_dirs[at].clone());
        entries.push((key, dir));
    }
    let dict = i
        .mem
        .new_dict(u32::try_from(entries.len()).map_err(|_| VmError::LimitCheck)?);
    for (key, value) in entries {
        let key = name(i, key)?;
        i.mem.dict_put(dict, key, value)?;
    }
    i.push(dict)
}

// --- device parameters -----------------------------------------------------

/// No parameter set is defined, so every device name is `undefined`
/// once the operands' types have been checked.
fn setdevparams(_: &mut Interp) -> Result<(), VmError> {
    Err(VmError::Undefined)
}

fn currentdevparams(_: &mut Interp) -> Result<(), VmError> {
    Err(VmError::Undefined)
}

// --- font-cache and VM operators -------------------------------------------

/// `bsize bmax msize mmax csize cmax blimit`: nothing is cached, so the
/// consumptions are zero; `bmax` is `MaxFontCache` and `blimit`
/// `MaxFontItem`.
fn cachestatus(i: &mut Interp) -> Result<(), VmError> {
    let bmax = i.system_params().limit("MaxFontCache");
    let blimit = user_int(i, "MaxFontItem")?;
    for value in [0, bmax, 0, CACHE_MATRICES_MAX, 0, CACHE_GLYPHS_MAX, blimit] {
        i.push(Object::integer(value))?;
    }
    Ok(())
}

fn setcachelimit(i: &mut Interp) -> Result<(), VmError> {
    let limit = i.peek(0)?.as_i32().expect("signature");
    set_user_int(i, "MaxFontItem", limit)?;
    i.pop()?;
    Ok(())
}

/// `mark size lower upper setcacheparams`: the topmost operands are
/// used, fewer leaving the rest unchanged. `lower` and `upper` are
/// `MinFontCompress` and `MaxFontItem`; `size` names `MaxFontCache`, a
/// system parameter, and is not applied, so a program needs no password
/// to call the operator.
fn setcacheparams(i: &mut Interp) -> Result<(), VmError> {
    let stack = i.ostack();
    let mark = stack
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let operands = stack[mark + 1..].to_vec();
    if operands.iter().any(|o| o.ty() != Type::Integer) {
        return Err(VmError::TypeCheck);
    }
    let mut top = operands.iter().rev().map(|o| o.as_i32().expect("checked"));
    let upper = top.next();
    let lower = top.next();
    if let Some(upper) = upper {
        set_user_int(i, "MaxFontItem", upper)?;
    }
    if let Some(lower) = lower {
        set_user_int(i, "MinFontCompress", lower)?;
    }
    for _ in mark..i.ostack().len() {
        i.pop()?;
    }
    Ok(())
}

fn currentcacheparams(i: &mut Interp) -> Result<(), VmError> {
    let size = i.system_params().limit("MaxFontCache");
    let lower = user_int(i, "MinFontCompress")?;
    let upper = user_int(i, "MaxFontItem")?;
    i.push(Object::mark())?;
    for value in [size, lower, upper] {
        i.push(Object::integer(value))?;
    }
    Ok(())
}

fn setvmthreshold(i: &mut Interp) -> Result<(), VmError> {
    let threshold = i.peek(0)?.as_i32().expect("signature");
    set_user_int(i, "VMThreshold", threshold)?;
    i.pop()?;
    Ok(())
}

/// −2, −1, and 0 set `VMReclaim`; 1 and 2 ask for a collection, and
/// there is no collector, so they do nothing.
fn vmreclaim(i: &mut Interp) -> Result<(), VmError> {
    match i.peek(0)?.as_i32().expect("signature") {
        code @ -2..=0 => set_user_int(i, "VMReclaim", code)?,
        1 | 2 => {}
        _ => return Err(VmError::RangeCheck),
    }
    i.pop()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_start_as_the_server_password() {
        let params = SystemParams::new(0);
        assert!(params.start_job_allowed(b"0"));
        assert!(!params.start_job_allowed(b"1"));
        let params = SystemParams::new(-12);
        assert!(params.start_job_allowed(b"-12"));
    }

    #[test]
    fn nulls_end_a_string_value() {
        assert_eq!(until_null(b"ab\0cd"), b"ab");
        assert_eq!(until_null(b"abc"), b"abc");
    }

    #[test]
    fn every_user_key_is_listed_once() {
        let mut keys: Vec<_> = USER_PARAMS.iter().map(|(k, _, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), USER_PARAMS.len());
    }
}
