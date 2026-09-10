// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Halftone screens and transfer functions (PLRM3 §7.4): `setscreen`,
//! `setcolorscreen`, `settransfer`, `setcolortransfer` and their getters.
//! The values are recorded in the graphics state, so `gsave`/`grestore`
//! and the getters behave as setup code expects, and never applied: a
//! device that rasterises nothing has nothing to halftone. The procedures
//! are kept by the VM and referred to by [`ProcRef`]. Defined with or
//! without a graphics backend.

use crate::error::VmError;
use crate::graphics::{ProcRef, Screen};
use crate::interp::Interp;
use crate::object::{Object, Type};

op_table! { OPS {
    "setscreen" => setscreen, [Num, Num, Any];
    "currentscreen" => currentscreen;
    "setcolorscreen" => setcolorscreen,
        [Num, Num, Any, Num, Num, Any, Num, Num, Any, Num, Num, Any];
    "currentcolorscreen" => currentcolorscreen;
    "settransfer" => settransfer, [Any];
    "currenttransfer" => currenttransfer;
    "setcolortransfer" => setcolortransfer, [Any, Any, Any, Any];
    "currentcolortransfer" => currentcolortransfer;
}}

/// A spot function: a procedure, or (Level 2) a halftone dictionary.
fn spot_at(i: &mut Interp, n: usize) -> Result<ProcRef, VmError> {
    let object = i.peek(n)?;
    match object.ty() {
        Type::Array | Type::PackedArray | Type::Dict => i.graphics_proc_ref(object),
        _ => Err(VmError::TypeCheck),
    }
}

fn transfer_at(i: &mut Interp, n: usize) -> Result<ProcRef, VmError> {
    let object = i.peek(n)?;
    match object.ty() {
        Type::Array | Type::PackedArray => i.graphics_proc_ref(object),
        _ => Err(VmError::TypeCheck),
    }
}

fn num_at(i: &Interp, n: usize) -> Result<f32, VmError> {
    i.peek(n)?.as_number().ok_or(VmError::TypeCheck)
}

/// The screen whose three operands end `n` below the top.
fn screen_at(i: &mut Interp, n: usize) -> Result<Screen, VmError> {
    Ok(Screen {
        frequency: num_at(i, n + 2)?,
        angle: num_at(i, n + 1)?,
        spot: spot_at(i, n)?,
    })
}

fn drop(i: &mut Interp, count: usize) -> Result<(), VmError> {
    for _ in 0..count {
        i.pop()?;
    }
    Ok(())
}

fn push_screen(i: &mut Interp, screen: Screen) -> Result<(), VmError> {
    i.push(Object::real(screen.frequency))?;
    i.push(Object::real(screen.angle))?;
    push_proc(i, screen.spot)
}

fn push_proc(i: &mut Interp, id: ProcRef) -> Result<(), VmError> {
    let procedure = i.graphics_proc(id).ok_or(VmError::Undefined)?;
    i.push(procedure)
}

fn setscreen(i: &mut Interp) -> Result<(), VmError> {
    let screen = screen_at(i, 0)?;
    i.set_screens([screen; 4])?;
    drop(i, 3)
}

/// The gray screen, the one `setscreen` last set.
fn currentscreen(i: &mut Interp) -> Result<(), VmError> {
    let screen = i.screens()[3];
    push_screen(i, screen)
}

fn setcolorscreen(i: &mut Interp) -> Result<(), VmError> {
    let screens = [
        screen_at(i, 9)?,
        screen_at(i, 6)?,
        screen_at(i, 3)?,
        screen_at(i, 0)?,
    ];
    i.set_screens(screens)?;
    drop(i, 12)
}

fn currentcolorscreen(i: &mut Interp) -> Result<(), VmError> {
    for screen in i.screens() {
        push_screen(i, screen)?;
    }
    Ok(())
}

fn settransfer(i: &mut Interp) -> Result<(), VmError> {
    let transfer = transfer_at(i, 0)?;
    i.set_transfers([transfer; 4])?;
    drop(i, 1)
}

fn currenttransfer(i: &mut Interp) -> Result<(), VmError> {
    let transfer = i.transfers()[3];
    push_proc(i, transfer)
}

fn setcolortransfer(i: &mut Interp) -> Result<(), VmError> {
    let transfers = [
        transfer_at(i, 3)?,
        transfer_at(i, 2)?,
        transfer_at(i, 1)?,
        transfer_at(i, 0)?,
    ];
    i.set_transfers(transfers)?;
    drop(i, 4)
}

fn currentcolortransfer(i: &mut Interp) -> Result<(), VmError> {
    for transfer in i.transfers() {
        push_proc(i, transfer)?;
    }
    Ok(())
}
