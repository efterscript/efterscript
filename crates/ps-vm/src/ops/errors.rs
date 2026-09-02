// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The default `errordict` entries and `handleerror` (PLRM3 §3.10).
//!
//! `HANDLERS` holds one internal operator per error name, installed in
//! `errordict` at startup; `OPS` holds the public `handleerror`, which runs
//! whatever `errordict` currently defines under that name.

use crate::error::VmError;
use crate::interp::Interp;
use crate::object::Object;

op_table! { OPS {
    "handleerror" => handleerror;
}}

op_table! { internal HANDLERS {
    VmError::DictFull.name() => |i| default_error(i, VmError::DictFull);
    VmError::DictStackOverflow.name() => |i| default_error(i, VmError::DictStackOverflow);
    VmError::DictStackUnderflow.name() => |i| default_error(i, VmError::DictStackUnderflow);
    VmError::ExecStackOverflow.name() => |i| default_error(i, VmError::ExecStackOverflow);
    VmError::InvalidAccess.name() => |i| default_error(i, VmError::InvalidAccess);
    VmError::InvalidExit.name() => |i| default_error(i, VmError::InvalidExit);
    VmError::InvalidFileAccess.name() => |i| default_error(i, VmError::InvalidFileAccess);
    VmError::InvalidRestore.name() => |i| default_error(i, VmError::InvalidRestore);
    VmError::IoError.name() => |i| default_error(i, VmError::IoError);
    VmError::LimitCheck.name() => |i| default_error(i, VmError::LimitCheck);
    VmError::NoCurrentPoint.name() => |i| default_error(i, VmError::NoCurrentPoint);
    VmError::RangeCheck.name() => |i| default_error(i, VmError::RangeCheck);
    VmError::StackOverflow.name() => |i| default_error(i, VmError::StackOverflow);
    VmError::StackUnderflow.name() => |i| default_error(i, VmError::StackUnderflow);
    VmError::SyntaxError.name() => |i| default_error(i, VmError::SyntaxError);
    VmError::TypeCheck.name() => |i| default_error(i, VmError::TypeCheck);
    VmError::Undefined.name() => |i| default_error(i, VmError::Undefined);
    VmError::UndefinedFileName.name() => |i| default_error(i, VmError::UndefinedFileName);
    VmError::UndefinedResult.name() => |i| default_error(i, VmError::UndefinedResult);
    VmError::UnmatchedMark.name() => |i| default_error(i, VmError::UnmatchedMark);
    VmError::VmFull.name() => |i| default_error(i, VmError::VmFull);
    "handleerror" => default_handleerror;
}}

fn default_error(i: &mut Interp, error: VmError) -> Result<(), VmError> {
    let name = i.intern(error.name());
    i.default_error(name);
    Ok(())
}

// The conventional report line goes to the error stream; `newerror` is
// cleared so the same error is not reported twice.
fn default_handleerror(i: &mut Interp) -> Result<(), VmError> {
    if !i.new_error() {
        return Ok(());
    }
    let summary = i.error_summary();
    let line = format!(
        "%%[ Error: {}; OffendingCommand: {} ]%%\n",
        summary.name, summary.command
    );
    i.write_stderr(line.as_bytes())?;
    let key = i.atoms.newerror;
    i.error_put(key, Object::boolean(false));
    Ok(())
}

fn handleerror(i: &mut Interp) -> Result<(), VmError> {
    let key = i.atoms.handleerror;
    match i.errordict_get(key) {
        Some(handler) => i.exec_indirect(handler),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Visibility;

    #[test]
    fn every_error_has_a_default_handler() {
        for error in VmError::ALL {
            assert!(
                HANDLERS
                    .iter()
                    .any(|e| e.visibility == Visibility::Internal && e.name == error.name()),
                "{}",
                error.name()
            );
        }
        assert_eq!(HANDLERS.len(), VmError::ALL.len() + 1);
    }
}
