// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The execution loop and the error machinery.
//!
//! The loop acts on the top frame of the execution stack and never calls
//! itself: control operators push frames, and `exit`, `stop`, and errors
//! unwind by truncating the stack.

use super::{ErrorSummary, Frame, Interp, LoopFrame, MAX_NESTED_ERROR_HANDLERS, Marker, Outcome};
use super::{SourceFrame, SourceSlot, lookup_in};
use crate::error::VmError;
use crate::memory::Memory;
use crate::names::Atom;
use crate::object::{Object, Type};
use crate::ops::{self, Num};
use crate::scanner::{Scan, ScanErrorKind, Scanner};
use crate::source::{FileSource, Source, StringSource};

fn scan_error(kind: ScanErrorKind) -> VmError {
    match kind {
        ScanErrorKind::SyntaxError | ScanErrorKind::BinaryEncoding => VmError::SyntaxError,
        ScanErrorKind::LimitCheck => VmError::LimitCheck,
        ScanErrorKind::Undefined => VmError::Undefined,
        ScanErrorKind::Vm(e) => e,
    }
}

enum LoopStep {
    Finished,
    Iterate {
        body: Object,
        value: Option<Object>,
        operator: &'static str,
    },
}

impl Interp {
    /// Executes `source` as a job: a run boundary and a `Source` frame are
    /// pushed and the loop runs until the boundary is reached again.
    pub fn run(&mut self, source: &mut dyn Source) -> Outcome {
        self.push_frame_unchecked(Frame::Marker(Marker::RunBoundary));
        self.push_frame_unchecked(Frame::Source(Box::new(SourceFrame {
            slot: SourceSlot::Run,
            scanner: Scanner::new(),
        })));
        self.execute_until_boundary(source)
    }

    /// Continues a suspended run once `source` has more bytes.
    pub fn resume(&mut self, source: &mut dyn Source) -> Outcome {
        self.execute_until_boundary(source)
    }

    fn execute_until_boundary(&mut self, source: &mut dyn Source) -> Outcome {
        #[cfg(debug_assertions)]
        {
            self.host_depth += 1;
            self.max_host_depth = self.max_host_depth.max(self.host_depth);
        }
        let outcome = loop {
            match self.estack.last_mut() {
                None => break Outcome::Ok,
                Some(Frame::Object(object)) => {
                    let object = *object;
                    self.pop_frame();
                    self.execute(object, false);
                }
                Some(Frame::Proc { array, next }) => {
                    let (array, index) = (*array, *next);
                    let element = self
                        .mem
                        .array(array)
                        .map(|items| items.get(index as usize).copied());
                    match element {
                        None => {
                            self.pop_frame();
                            self.raise(VmError::InvalidAccess, array);
                        }
                        Some(None) => {
                            self.pop_frame();
                        }
                        Some(Some(object)) => {
                            if let Some(Frame::Proc { next, .. }) = self.estack.last_mut() {
                                *next = index + 1;
                            }
                            self.execute(object, true);
                        }
                    }
                }
                Some(Frame::Source(frame)) => {
                    let SourceFrame { slot, scanner } = &mut **frame;
                    let dstack = &self.dstack;
                    let mut resolver =
                        |atom: Atom, mem: &mut Memory| lookup_in(dstack, mem, Object::name(atom));
                    let result = match slot {
                        SourceSlot::Run => scanner.next(source, &mut self.mem, &mut resolver),
                        SourceSlot::File { source, .. } => {
                            scanner.next(source, &mut self.mem, &mut resolver)
                        }
                        SourceSlot::String(source) => {
                            scanner.next(source, &mut self.mem, &mut resolver)
                        }
                    };
                    let command = slot.object().unwrap_or(Object::null());
                    match result {
                        Ok(Scan::Token { object, .. }) => self.execute(object, true),
                        Ok(Scan::End) => {
                            self.pop_frame();
                        }
                        Ok(Scan::NeedMore) => break Outcome::Suspended,
                        Err(e) => self.raise(scan_error(e.kind), command),
                    }
                }
                Some(Frame::Loop(_)) => self.advance_loop(),
                Some(Frame::Stopped) => {
                    self.pop_frame();
                    if let Err(e) = self.push(Object::boolean(false)) {
                        let command = self.operator("stopped").unwrap_or(Object::null());
                        self.raise(e, command);
                    }
                }
                Some(Frame::Marker(Marker::RunBoundary)) => {
                    if self.stopped && self.pending_error.is_none() && self.new_error() {
                        self.pending_error = Some(self.error_summary());
                        if let Some(handler) = self.errordict_get(self.atoms.handleerror) {
                            self.push_frame_unchecked(Frame::Object(handler));
                            continue;
                        }
                    }
                    self.pop_frame();
                    self.stopped = false;
                    break match self.pending_error.take() {
                        Some(summary) => {
                            self.error_put(self.atoms.newerror, Object::boolean(false));
                            Outcome::Error(summary)
                        }
                        None => Outcome::Ok,
                    };
                }
                Some(Frame::Marker(_)) => {
                    self.pop_frame();
                }
            }
        };
        #[cfg(debug_assertions)]
        {
            self.host_depth -= 1;
        }
        outcome
    }

    // `direct` is true for an object met in the token stream or as a
    // procedure element, where executable arrays and strings are pushed
    // rather than run (PLRM3 §3.5.5).
    fn execute(&mut self, object: Object, direct: bool) {
        let mut object = object;
        let mut direct = direct;
        // The name a procedure was reached through, reported as the
        // offending command if the procedure cannot be started.
        let mut origin = object;
        loop {
            if object.is_literal() {
                if let Err(e) = self.push(object) {
                    self.raise(e, object);
                }
                return;
            }
            match object.ty() {
                Type::Name => match self.lookup(object) {
                    None => {
                        self.raise(VmError::Undefined, object);
                        return;
                    }
                    Some(value) if value.is_literal() => {
                        if let Err(e) = self.push(value) {
                            self.raise(e, object);
                        }
                        return;
                    }
                    Some(value) if value.ty() == Type::Name => {
                        if let Err(e) = self.push_frame(Frame::Object(value)) {
                            self.raise(e, object);
                        }
                        return;
                    }
                    Some(value) => {
                        origin = object;
                        object = value;
                        direct = false;
                    }
                },
                Type::Operator => {
                    let index = object.as_operator().expect("operator");
                    if let Err(e) = self.call_operator(index) {
                        self.raise(e, object);
                    }
                    return;
                }
                Type::Array | Type::PackedArray if !direct => {
                    if let Err(e) = self.push_proc(object) {
                        self.raise(e, origin);
                    }
                    return;
                }
                Type::String if !direct => {
                    let slot = SourceSlot::String(StringSource::new(object).expect("string"));
                    self.push_source(slot, origin);
                    return;
                }
                Type::File if !direct => {
                    let source = FileSource::new(object).expect("file");
                    self.push_source(SourceSlot::File { object, source }, origin);
                    return;
                }
                Type::Null => return,
                _ => {
                    if let Err(e) = self.push(object) {
                        self.raise(e, object);
                    }
                    return;
                }
            }
        }
    }

    fn push_source(&mut self, slot: SourceSlot, command: Object) {
        let frame = Frame::Source(Box::new(SourceFrame {
            slot,
            scanner: Scanner::new(),
        }));
        if let Err(e) = self.push_frame(frame) {
            self.raise(e, command);
        }
    }

    fn call_operator(&mut self, index: u32) -> Result<(), VmError> {
        let entry = self
            .ops
            .get(index as usize)
            .copied()
            .ok_or(VmError::Undefined)?;
        ops::check_sig(self, entry.sig)?;
        (entry.func)(self)
    }

    fn advance_loop(&mut self) {
        let Some(Frame::Loop(frame)) = self.estack.last_mut() else {
            return;
        };
        let step = match frame {
            LoopFrame::For {
                body,
                current,
                increment,
                limit,
                done,
            } => {
                let past = *done
                    || match (*current, *increment, *limit) {
                        (Num::Int(c), Num::Int(i), Num::Int(l)) => {
                            if i >= 0 {
                                c > l
                            } else {
                                c < l
                            }
                        }
                        (c, i, l) => {
                            if i.as_f32() >= 0.0 {
                                c.as_f32() > l.as_f32()
                            } else {
                                c.as_f32() < l.as_f32()
                            }
                        }
                    };
                if past {
                    LoopStep::Finished
                } else {
                    let value = current.to_object();
                    match (*current, *increment) {
                        (Num::Int(c), Num::Int(i)) => match c.checked_add(i) {
                            Some(n) => *current = Num::Int(n),
                            None => *done = true,
                        },
                        (c, i) => *current = Num::Real(c.as_f32() + i.as_f32()),
                    }
                    LoopStep::Iterate {
                        body: *body,
                        value: Some(value),
                        operator: "for",
                    }
                }
            }
            LoopFrame::Repeat { body, remaining } => {
                if *remaining == 0 {
                    LoopStep::Finished
                } else {
                    *remaining -= 1;
                    LoopStep::Iterate {
                        body: *body,
                        value: None,
                        operator: "repeat",
                    }
                }
            }
            LoopFrame::Loop { body } => LoopStep::Iterate {
                body: *body,
                value: None,
                operator: "loop",
            },
        };
        match step {
            LoopStep::Finished => {
                self.pop_frame();
            }
            LoopStep::Iterate {
                body,
                value,
                operator,
            } => {
                if let Some(value) = value
                    && let Err(e) = self.push(value)
                {
                    let command = self.operator(operator).unwrap_or(Object::null());
                    self.raise(e, command);
                    return;
                }
                if let Err(e) = self.push_proc(body) {
                    self.raise(e, body);
                }
            }
        }
    }

    // --- errors --------------------------------------------------------------

    /// Enters the error machinery: the offending object is pushed and the
    /// `errordict` entry for the error runs; absent one, the default
    /// handling applies.
    pub(crate) fn raise(&mut self, error: VmError, command: Object) {
        let name = self.intern(error.name());
        let nested = self
            .estack
            .iter()
            .filter(|f| matches!(f, Frame::Marker(Marker::ErrorHandler)))
            .count();
        if nested >= MAX_NESTED_ERROR_HANDLERS {
            self.record_error(name, command);
            self.stop_at_boundary();
            return;
        }
        self.ostack.push(command);
        match self.errordict_get(name) {
            Some(handler) => {
                self.push_frame_unchecked(Frame::Marker(Marker::ErrorHandler));
                self.push_frame_unchecked(Frame::Object(handler));
            }
            None => self.default_error(name),
        }
    }

    /// What the default `errordict` entries do: take the offending object
    /// off the operand stack, record the error in `$error`, and `stop`.
    pub(crate) fn default_error(&mut self, name: Object) {
        let command = self.ostack.pop().unwrap_or(Object::null());
        self.record_error(name, command);
        self.stop();
    }

    pub(crate) fn record_error(&mut self, name: Object, command: Object) {
        let atoms = self.atoms;
        self.error_put(atoms.newerror, Object::boolean(true));
        self.error_put(atoms.errorname, name);
        self.error_put(atoms.command, command);
        let record = self
            .error_get(atoms.recordstacks)
            .and_then(Object::as_bool)
            .unwrap_or(true);
        if !record {
            return;
        }
        let global = self.mem.current_global();
        self.mem.set_global(false);
        let snapshots = [
            (atoms.ostack, self.ostack.clone()),
            (atoms.estack, self.exec_objects()),
            (atoms.dstack, self.dstack.clone()),
        ];
        for (key, items) in snapshots {
            if let Ok(array) = self.mem.alloc_array(items) {
                self.error_put(key, array);
            }
        }
        self.mem.set_global(global);
    }

    /// `stop`: unwinds to the nearest `Stopped` frame, which yields `true`,
    /// or to the run boundary.
    pub(crate) fn stop(&mut self) {
        for i in (0..self.estack.len()).rev() {
            match &self.estack[i] {
                Frame::Stopped => {
                    self.truncate_frames(i);
                    self.ostack.push(Object::boolean(true));
                    return;
                }
                Frame::Marker(Marker::RunBoundary) => {
                    self.truncate_frames(i + 1);
                    self.stopped = true;
                    return;
                }
                _ => {}
            }
        }
        self.truncate_frames(0);
        self.stopped = true;
    }

    fn stop_at_boundary(&mut self) {
        let boundary = self
            .estack
            .iter()
            .rposition(|f| matches!(f, Frame::Marker(Marker::RunBoundary)));
        self.truncate_frames(boundary.map_or(0, |i| i + 1));
        self.stopped = true;
    }

    /// `exit`: unwinds through the innermost loop; a `stopped` context or a
    /// marker in the way is `invalidexit`.
    pub(crate) fn exit(&mut self) -> Result<(), VmError> {
        for i in (0..self.estack.len()).rev() {
            match &self.estack[i] {
                Frame::Loop(_) => {
                    self.truncate_frames(i);
                    return Ok(());
                }
                Frame::Stopped | Frame::Marker(_) => return Err(VmError::InvalidExit),
                _ => {}
            }
        }
        Err(VmError::InvalidExit)
    }

    /// `quit`: abandons every frame, run boundaries included.
    pub(crate) fn quit(&mut self) {
        self.truncate_frames(0);
        self.quit = true;
    }

    /// The objects `execstack` reports, bottom first.
    pub(crate) fn exec_objects(&self) -> Vec<Object> {
        self.estack.iter().filter_map(Frame::object).collect()
    }

    pub(crate) fn new_error(&self) -> bool {
        self.error_get(self.atoms.newerror)
            .and_then(Object::as_bool)
            .unwrap_or(false)
    }

    pub(crate) fn error_summary(&self) -> ErrorSummary {
        let text = |key| {
            let object = self.error_get(key).unwrap_or(Object::null());
            String::from_utf8_lossy(&ops::output::brief(self, object)).into_owned()
        };
        ErrorSummary {
            name: text(self.atoms.errorname),
            command: text(self.atoms.command),
        }
    }
}
