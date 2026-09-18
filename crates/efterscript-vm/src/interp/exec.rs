// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The execution loop and the error machinery.
//!
//! The loop acts on the top frame of the execution stack and never calls
//! itself: control operators push frames, and `exit`, `stop`, and errors
//! unwind by truncating the stack.

use super::{ErrorSummary, Frame, Interp, LoopFrame, MAX_NESTED_ERROR_HANDLERS, Marker, Outcome};
use super::{ResourceKey, SourceFrame, SourceSlot, lookup_in};
use crate::error::VmError;
use crate::graphics::{Point, Seg};
use crate::memory::Memory;
use crate::names::Atom;
use crate::object::{Access, Object, Type};
use crate::ops::{self, Num};
use crate::scanner::{Scan, ScanErrorKind, Scanner};
use crate::source::{FileSource, Source, StringSource};

pub(crate) fn scan_error(kind: ScanErrorKind) -> VmError {
    match kind {
        ScanErrorKind::SyntaxError | ScanErrorKind::BinaryEncoding => VmError::SyntaxError,
        ScanErrorKind::LimitCheck => VmError::LimitCheck,
        ScanErrorKind::Undefined => VmError::Undefined,
        ScanErrorKind::Vm(e) => e,
    }
}

// The job's source as the scanner sees it: the run file entry, plus the
// borrowed source's promise of more bytes, which the entry cannot express.
struct RunSource {
    file: FileSource,
    more: bool,
}

impl Source for RunSource {
    fn peek(&mut self, memory: &mut Memory) -> Result<Option<u8>, VmError> {
        self.file.peek(memory)
    }

    fn advance(&mut self, memory: &mut Memory) {
        self.file.advance(memory);
    }

    fn position(&self, memory: &Memory) -> usize {
        self.file.position(memory)
    }

    fn more_may_come(&self) -> bool {
        self.more
    }
}

/// What an iteration pushes before its body: up to the six numbers of a
/// `pathforall` curve.
type Values = [Option<Object>; 6];

const NO_VALUES: Values = [None; 6];

fn one(value: Object) -> Values {
    [Some(value), None, None, None, None, None]
}

fn two(first: Object, second: Object) -> Values {
    [Some(first), Some(second), None, None, None, None]
}

fn points(points: &[Point]) -> Values {
    let mut values = NO_VALUES;
    for (slot, p) in values.chunks_mut(2).zip(points) {
        slot[0] = Some(Object::real(p.x));
        slot[1] = Some(Object::real(p.y));
    }
    values
}

enum LoopStep {
    Finished,
    Iterate {
        body: Object,
        values: Values,
        operator: &'static str,
    },
    /// The image's data is complete: hand it to the backend.
    FinishImage,
    /// A pattern cell's or form body's procedure has returned: end the
    /// capture and finish the operator.
    FinishPaintProcedure,
    /// A CIE conversion has every result it needs: finish its operator.
    FinishCie,
    /// A reusable stream's source has ended: finish `filter`.
    FinishReusable,
    /// The frame stays and takes another step next.
    Continue,
    Failed(VmError, &'static str),
}

impl Interp {
    /// Executes `source` as a job: a run boundary and a `Source` frame are
    /// pushed and the loop runs until the boundary is reached again.
    pub fn run(&mut self, source: &mut dyn Source) -> Outcome {
        self.reset_run_input();
        self.push_frame_unchecked(Frame::Marker(Marker::RunBoundary));
        self.push_frame_unchecked(Frame::Source(Box::new(SourceFrame {
            slot: SourceSlot::Run,
            scanner: Scanner::new(),
        })));
        self.execute_until_boundary(source)
    }

    /// Continues a suspended run once `source` has more bytes.
    pub fn resume(&mut self, source: &mut dyn Source) -> Outcome {
        // An operator re-queued for its reads runs before the source
        // frame gets its turn, so the new bytes are moved in first.
        if let Err(e) = self.pump_run_source(source) {
            self.raise(e, Object::null());
        }
        self.execute_until_boundary(source)
    }

    fn execute_until_boundary(&mut self, source: &mut dyn Source) -> Outcome {
        #[cfg(debug_assertions)]
        {
            self.host_depth += 1;
            self.max_host_depth = self.max_host_depth.max(self.host_depth);
        }
        let outcome = loop {
            if self.starved {
                self.starved = false;
                break Outcome::Suspended;
            }
            match self.estack.last_mut() {
                None => break Outcome::Ok,
                Some(Frame::Object(object)) => {
                    let object = *object;
                    self.pop_frame();
                    self.execute(object, false);
                }
                Some(Frame::Proc { array, next }) => {
                    let (array, index) = (*array, *next);
                    // An exhausted procedure needs no storage: `restore`
                    // may already have discarded it.
                    if index >= array.length().unwrap_or(0) {
                        self.pop_frame();
                        continue;
                    }
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
                    // A file that was closed while being executed (the
                    // job's own source, or an `eexec` layer ended by
                    // `closefile`) has nothing more to run.
                    let file = match frame.slot {
                        SourceSlot::Run => Some(self.run_file),
                        SourceSlot::File { object, .. } => Some(object),
                        SourceSlot::String(_) => None,
                    };
                    if let Some(file) = file {
                        if !self.mem.file_is_open(file) {
                            self.pop_frame();
                            continue;
                        }
                        if let Err(e) = self.pump_run_source(source) {
                            self.raise(e, Object::null());
                            continue;
                        }
                    }
                    let Some(Frame::Source(frame)) = self.estack.last_mut() else {
                        unreachable!("frame checked above");
                    };
                    let SourceFrame { slot, scanner } = &mut **frame;
                    let dstack = &self.dstack;
                    let mut resolver =
                        |atom: Atom, mem: &mut Memory| lookup_in(dstack, mem, Object::name(atom));
                    let result = match slot {
                        SourceSlot::Run => {
                            let mut run_source = RunSource {
                                file: FileSource::new(self.run_file).expect("file"),
                                more: source.more_may_come(),
                            };
                            scanner.next(&mut run_source, &mut self.mem, &mut resolver)
                        }
                        SourceSlot::File { source, .. } => {
                            scanner.next(source, &mut self.mem, &mut resolver)
                        }
                        SourceSlot::String(source) => {
                            scanner.next(source, &mut self.mem, &mut resolver)
                        }
                    };
                    let command = slot.object().unwrap_or(Object::null());
                    // A filter executed to its end is read through its
                    // end-of-data marker and closed, so the file under it
                    // continues after the marker.
                    let filter = match slot {
                        SourceSlot::File { object, .. }
                            if self.mem.files().is_filter(object.handle().expect("file")) =>
                        {
                            Some(*object)
                        }
                        _ => None,
                    };
                    match result {
                        Ok(Scan::Token { object, .. }) => self.execute(object, true),
                        Ok(Scan::End) => {
                            match filter.map(|file| ops::file::drain_to_marker(self, file)) {
                                None | Some(Ok(())) => {
                                    if let Some(file) = filter {
                                        let _ = self.mem.close_file(file);
                                    }
                                    self.pop_frame();
                                }
                                Some(Err(VmError::NeedMore)) => {
                                    self.mem.files_mut().rollback();
                                    if !self.run_wanted_procedure() {
                                        self.starved = true;
                                    }
                                }
                                Some(Err(e)) => {
                                    self.pop_frame();
                                    self.raise(e, command);
                                }
                            }
                        }
                        Ok(Scan::NeedMore) => {
                            if !self.run_wanted_procedure() {
                                break Outcome::Suspended;
                            }
                        }
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
                Some(Frame::Marker(Marker::CMapLoad { name, retry, .. })) => {
                    let (name, retry) = (*name, *retry);
                    self.pop_frame();
                    if let Err(e) = ops::cidinit::loaded(self, name, retry) {
                        self.raise(e, Object::operator(retry));
                    }
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
        self.mem.files_mut().commit();
        if self.charge(object) {
            return;
        }
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
                    match self.call_operator(index) {
                        Ok(()) => {}
                        Err(VmError::NeedMore) => self.suspend_for_input(object),
                        Err(e) => self.raise(e, object),
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

    /// An operator's read of the job's source found no byte with more to
    /// come: its reads are undone, the operand stack is as it left it
    /// (a reading operator pops nothing before its read completes), and
    /// the operator is queued to run again when the loop resumes. The
    /// step it was charged is refunded so a split job counts as an
    /// unsplit one.
    fn suspend_for_input(&mut self, operator: Object) {
        self.mem.files_mut().rollback();
        self.steps = self.steps.saturating_sub(1);
        self.push_frame_unchecked(Frame::Object(operator));
        if !self.run_wanted_procedure() {
            self.starved = true;
        }
    }

    /// If the read that found no byte was from a filter over a
    /// procedure source, arranges for the procedure to run and deliver
    /// its next string before the reader tries again, and returns true;
    /// otherwise the bytes come from outside and the run must suspend.
    fn run_wanted_procedure(&mut self) -> bool {
        let Some((handle, body)) = self.mem.files_mut().take_wanted_procedure() else {
            return false;
        };
        self.push_frame_unchecked(Frame::Loop(LoopFrame::FilterData {
            body,
            handle,
            started: false,
        }));
        true
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
        self.mem.files_mut().commit();
        if let Some(Frame::Loop(LoopFrame::Show(_))) = self.estack.last() {
            ops::show::step(self);
            return;
        }
        let Some(Frame::Loop(frame)) = self.estack.last_mut() else {
            return;
        };
        let step = match frame {
            // Stepped above.
            LoopFrame::Show(_) => LoopStep::Finished,
            LoopFrame::ResourceForAll {
                body,
                keys,
                scratch,
                next,
            } => match keys.get(*next) {
                None => LoopStep::Finished,
                Some(ResourceKey::Int(key)) => {
                    *next += 1;
                    LoopStep::Iterate {
                        body: *body,
                        values: one(Object::integer(*key)),
                        operator: "resourceforall",
                    }
                }
                Some(ResourceKey::Name(name)) => {
                    let filled = u32::try_from(name.len())
                        .ok()
                        .and_then(|n| scratch.with_interval(0, n))
                        .ok_or(VmError::RangeCheck)
                        .and_then(|interval| {
                            self.mem
                                .string_put_bytes(*scratch, 0, name)
                                .map(|()| interval)
                        });
                    match filled {
                        Ok(interval) => {
                            *next += 1;
                            LoopStep::Iterate {
                                body: *body,
                                values: one(interval),
                                operator: "resourceforall",
                            }
                        }
                        Err(e) => LoopStep::Failed(e, "resourceforall"),
                    }
                }
            },
            LoopFrame::PathForAll { procs, segs, next } => match segs.get(*next) {
                None => LoopStep::Finished,
                Some(seg) => {
                    let (body, values) = match *seg {
                        Seg::Move(p) => (procs[0], points(&[p])),
                        Seg::Line(p) => (procs[1], points(&[p])),
                        Seg::Curve(a, b, c) => (procs[2], points(&[a, b, c])),
                        Seg::Close => (procs[3], NO_VALUES),
                    };
                    *next += 1;
                    LoopStep::Iterate {
                        body,
                        values,
                        operator: "pathforall",
                    }
                }
            },
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
                        values: one(value),
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
                        values: NO_VALUES,
                        operator: "repeat",
                    }
                }
            }
            LoopFrame::Loop { body } => LoopStep::Iterate {
                body: *body,
                values: NO_VALUES,
                operator: "loop",
            },
            LoopFrame::ForAll {
                body,
                container,
                next,
            } => {
                let index = *next as usize;
                let element = match container.ty() {
                    Type::Array | Type::PackedArray => {
                        if index >= container.length().unwrap_or(0) as usize {
                            Ok(None)
                        } else {
                            self.mem.array_get(*container, index).map(|v| Some(one(v)))
                        }
                    }
                    Type::String => {
                        if index >= container.length().unwrap_or(0) as usize {
                            Ok(None)
                        } else {
                            self.mem
                                .string_get(*container, index)
                                .map(|b| Some(one(Object::integer(i32::from(b)))))
                        }
                    }
                    _ => self
                        .mem
                        .dict_entry_at(*container, index)
                        .map(|entry| entry.map(|(k, v)| two(k, v))),
                };
                match element {
                    Ok(None) => LoopStep::Finished,
                    Ok(Some(values)) => {
                        *next += 1;
                        LoopStep::Iterate {
                            body: *body,
                            values,
                            operator: "forall",
                        }
                    }
                    Err(e) => LoopStep::Failed(e, "forall"),
                }
            }
            LoopFrame::FilterData {
                body,
                handle,
                started,
            } => {
                if !*started {
                    *started = true;
                    LoopStep::Iterate {
                        body: *body,
                        values: NO_VALUES,
                        operator: "filter",
                    }
                } else {
                    // The procedure left its next string on the operand
                    // stack; an empty one ends the source.
                    match self.ostack.pop() {
                        None => LoopStep::Failed(VmError::StackUnderflow, "filter"),
                        Some(chunk) if chunk.ty() == Type::String => {
                            let readable = chunk.access().unwrap_or_default() <= Access::ReadOnly;
                            match self.mem.string(chunk).filter(|_| readable) {
                                None => LoopStep::Failed(VmError::InvalidAccess, "filter"),
                                Some(bytes) => {
                                    let bytes = bytes.to_vec();
                                    self.mem.files_mut().feed_procedure(*handle, &bytes);
                                    LoopStep::Finished
                                }
                            }
                        }
                        Some(_) => LoopStep::Failed(VmError::TypeCheck, "filter"),
                    }
                }
            }
            // A paint procedure runs once with its dictionary as the
            // operand; the frame's `started` marks the capture as open
            // until the second step takes it over.
            LoopFrame::PatternCell {
                body,
                dict,
                operator,
                started,
                ..
            } => {
                if *started {
                    *started = false;
                    LoopStep::FinishPaintProcedure
                } else {
                    *started = true;
                    LoopStep::Iterate {
                        body: *body,
                        values: one(*dict),
                        operator,
                    }
                }
            }
            LoopFrame::FormBody {
                body,
                dict,
                started,
                ..
            } => {
                if *started {
                    *started = false;
                    LoopStep::FinishPaintProcedure
                } else {
                    *started = true;
                    LoopStep::Iterate {
                        body: *body,
                        values: one(*dict),
                        operator: "execform",
                    }
                }
            }
            // The procedure in flight left its result on the operand
            // stack; the next call, if any, is issued with its input.
            LoopFrame::CieDecode { job } => {
                let operator = job.operator();
                let collected = match job.take_pending() {
                    None => Ok(()),
                    Some(depth) => match self.ostack.pop() {
                        None => Err(VmError::StackUnderflow),
                        Some(result) => match result.as_number() {
                            Some(n) => {
                                self.ostack.truncate(depth);
                                job.deliver(f64::from(n));
                                Ok(())
                            }
                            None => Err(VmError::TypeCheck),
                        },
                    },
                };
                match collected {
                    Err(e) => LoopStep::Failed(e, operator),
                    Ok(()) => match job.advance() {
                        Some((body, input)) => {
                            job.issue(self.ostack.len());
                            LoopStep::Iterate {
                                body,
                                values: one(Object::real(input)),
                                operator,
                            }
                        }
                        None => LoopStep::FinishCie,
                    },
                }
            }
            LoopFrame::ReusableRead(read) => match read.step(&mut self.mem) {
                Ok(true) => LoopStep::FinishReusable,
                Ok(false) => LoopStep::Continue,
                Err(e) => LoopStep::Failed(e, "filter"),
            },
            LoopFrame::ImageData { body, acquisition } => {
                let operator = acquisition.operator_name();
                if !acquisition.started {
                    acquisition.started = true;
                    if acquisition.is_complete() {
                        LoopStep::FinishImage
                    } else {
                        LoopStep::Iterate {
                            body: *body,
                            values: NO_VALUES,
                            operator,
                        }
                    }
                } else {
                    // The procedure left its next chunk on the operand stack.
                    match self.ostack.pop() {
                        None => LoopStep::Failed(VmError::StackUnderflow, operator),
                        Some(chunk) if chunk.ty() == Type::String => {
                            let readable = chunk.access().unwrap_or_default() <= Access::ReadOnly;
                            match self.mem.string(chunk).filter(|_| readable) {
                                None => LoopStep::Failed(VmError::InvalidAccess, operator),
                                Some(bytes) => {
                                    if acquisition.feed(bytes) {
                                        if let Some(next) = acquisition.next_source() {
                                            *body = next;
                                        }
                                        LoopStep::Iterate {
                                            body: *body,
                                            values: NO_VALUES,
                                            operator,
                                        }
                                    } else {
                                        LoopStep::FinishImage
                                    }
                                }
                            }
                        }
                        Some(_) => LoopStep::Failed(VmError::TypeCheck, operator),
                    }
                }
            }
        };
        match step {
            LoopStep::Finished => {
                self.pop_frame();
            }
            LoopStep::FinishImage => {
                let Some(Frame::Loop(LoopFrame::ImageData { acquisition, .. })) = self.pop_frame()
                else {
                    unreachable!("frame checked above");
                };
                let operator = acquisition.operator_name();
                if let Err(e) = ops::image::finish(self, *acquisition) {
                    let command = self.operator(operator).unwrap_or(Object::null());
                    self.raise(e, command);
                }
            }
            LoopStep::FinishCie => {
                let Some(Frame::Loop(LoopFrame::CieDecode { job })) = self.pop_frame() else {
                    unreachable!("frame checked above");
                };
                let operator = job.operator();
                if let Err(e) = ops::cie::finish_job(self, *job) {
                    let command = self.operator(operator).unwrap_or(Object::null());
                    self.raise(e, command);
                }
            }
            LoopStep::FinishReusable => {
                let Some(Frame::Loop(LoopFrame::ReusableRead(read))) = self.pop_frame() else {
                    unreachable!("frame checked above");
                };
                if let Err(e) = ops::filter::finish_reusable(self, *read) {
                    let command = self.operator("filter").unwrap_or(Object::null());
                    self.raise(e, command);
                }
            }
            LoopStep::Continue => {}
            LoopStep::FinishPaintProcedure => match self.pop_frame() {
                Some(Frame::Loop(LoopFrame::PatternCell {
                    depth, operator, ..
                })) => ops::pattern::finish_cell(self, depth, operator),
                Some(Frame::Loop(LoopFrame::FormBody { depth, info, .. })) => {
                    ops::form::finish_body(self, depth, &info);
                }
                _ => unreachable!("frame checked above"),
            },
            LoopStep::Failed(VmError::NeedMore, _) => {
                // The frame stays and takes the step again on resume.
                self.mem.files_mut().rollback();
                if !self.run_wanted_procedure() {
                    self.starved = true;
                }
            }
            LoopStep::Failed(e, operator) => {
                self.pop_frame();
                let command = self.operator(operator).unwrap_or(Object::null());
                self.raise(e, command);
            }
            LoopStep::Iterate {
                body,
                values,
                operator,
            } => {
                let command = self.operator(operator).unwrap_or(Object::null());
                if self.charge(command) {
                    return;
                }
                for value in values.into_iter().flatten() {
                    if let Err(e) = self.push(value) {
                        let command = self.operator(operator).unwrap_or(Object::null());
                        self.raise(e, command);
                        return;
                    }
                }
                if let Err(e) = self.push_proc(body) {
                    self.raise(e, body);
                }
            }
        }
    }

    // --- the execution budget --------------------------------------------------

    /// Counts one executed object against `Limits::steps`. When the
    /// budget is exceeded, `limitcheck` is raised on `command` and the
    /// budget is extended once by a grace so a handler can run; a second
    /// exceed is raised on every object from then on, so no handler can
    /// keep the job alive. Returns whether an error was raised.
    fn charge(&mut self, command: Object) -> bool {
        self.steps += 1;
        let Some(limit) = self.steps_limit else {
            return false;
        };
        if self.steps <= limit {
            return false;
        }
        if !self.grace_given {
            self.grace_given = true;
            self.steps_limit = Some(limit.saturating_add((limit / 16).max(1000)));
        }
        self.raise(VmError::LimitCheck, command);
        true
    }

    // --- errors --------------------------------------------------------------

    /// Enters the error machinery: the offending object is pushed and the
    /// `errordict` entry for the error runs; absent one, the default
    /// handling applies.
    pub(crate) fn raise(&mut self, error: VmError, command: Object) {
        debug_assert!(
            error != VmError::NeedMore,
            "a starved read outside an operator or loop step"
        );
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

    /// Every object the execution stack still needs, for `restore`'s
    /// check: loop bodies and containers count, an exhausted procedure
    /// (typically the one that called `restore`) does not.
    pub(crate) fn exec_references(&self) -> Vec<Object> {
        let mut objects = Vec::new();
        for frame in &self.estack {
            match frame {
                Frame::Object(object) => objects.push(*object),
                Frame::Proc { array, next } => {
                    if array.length().is_some_and(|len| *next < len) {
                        objects.push(*array);
                    }
                }
                Frame::Source(frame) => objects.extend(frame.slot.object()),
                Frame::Loop(frame) => objects.extend(frame.references()),
                Frame::Stopped | Frame::Marker(_) => {}
            }
        }
        objects
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
