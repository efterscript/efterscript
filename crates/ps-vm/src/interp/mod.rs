// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The interpreter: stacks, the execution loop, name lookup, and the error
//! machinery (PLRM3 §3.5, §3.10, §3.12).

mod exec;
mod frame;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::error::VmError;
use crate::files::{FileCapability, Stream};
use crate::graphics::GraphicsBackend;
use crate::io::Io;
use crate::memory::Memory;
use crate::object::{Access, Object, Type};
use crate::ops::{self, Num, OpEntry, Visibility};

pub(crate) use exec::scan_error;
pub use frame::{Frame, LoopFrame, Marker, SourceFrame, SourceSlot};

// The file-table stream behind the job's source. The loop moves the bytes
// of the source handed to `run` into this buffer before scanning, so the
// scanner's cursor and `currentfile` reads share one file entry.
#[derive(Clone, Default)]
struct RunStream(Rc<RefCell<VecDeque<u8>>>);

impl Stream for RunStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        let mut pending = self.0.borrow_mut();
        let n = buf.len().min(pending.len());
        for (slot, byte) in buf.iter_mut().zip(pending.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }
}

/// Stack limits. The defaults are the PLRM3 Appendix B minimums.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub operand: usize,
    pub dict: usize,
    pub exec: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            operand: 500,
            dict: 20,
            exec: 250,
        }
    }
}

/// What the embedder grants a program; nothing else is reachable.
#[derive(Default)]
pub struct Capabilities {
    pub file: Option<Box<dyn FileCapability>>,
}

/// Tolerance policy; empty until a change defines the first quirk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quirks {}

#[derive(Default)]
pub struct Config {
    pub limits: Limits,
    pub io: Io,
    pub capabilities: Capabilities,
    pub quirks: Quirks,
}

/// The `$error` contents an uncaught error leaves behind, as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorSummary {
    pub name: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Error(ErrorSummary),
    /// The job's source ended mid-token and may grow; `resume` continues.
    Suspended,
}

/// Errors raised while that many `errordict` handlers are still running end
/// the job instead of nesting further.
pub const MAX_NESTED_ERROR_HANDLERS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct StandardDicts {
    pub systemdict: Object,
    pub globaldict: Object,
    pub userdict: Object,
    pub errordict: Object,
    /// `$error`
    pub error: Object,
    pub statusdict: Object,
}

// Literal names the machinery uses on every error.
#[derive(Clone, Copy)]
pub(crate) struct Atoms {
    pub newerror: Object,
    pub errorname: Object,
    pub command: Object,
    pub ostack: Object,
    pub estack: Object,
    pub dstack: Object,
    pub recordstacks: Object,
    pub handleerror: Object,
}

pub struct Interp {
    pub(crate) mem: Memory,
    pub(crate) ostack: Vec<Object>,
    pub(crate) dstack: Vec<Object>,
    pub(crate) estack: Vec<Frame>,
    limits: Limits,
    pub(crate) ops: &'static [OpEntry],
    stdin: Option<Object>,
    stdout: Option<Object>,
    stderr: Option<Object>,
    run_file: Object,
    run_buffer: RunStream,
    graphics: Option<Box<dyn GraphicsBackend>>,
    // One entry per live `save`: the graphics-state depth just after the
    // gsave that `save` performed, below which `grestore` must not pop.
    gstate_floors: Vec<usize>,
    page_device: Object,
    #[allow(dead_code)]
    pub(crate) quirks: Quirks,
    pub(crate) dicts: StandardDicts,
    pub(crate) atoms: Atoms,
    dstack_floor: usize,
    exec_count: usize,
    // Set by `stop` when it unwinds to the run boundary.
    stopped: bool,
    pending_error: Option<ErrorSummary>,
    quit: bool,
    #[cfg(debug_assertions)]
    host_depth: u32,
    #[cfg(debug_assertions)]
    max_host_depth: u32,
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

impl Interp {
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Self {
        let Config {
            limits,
            io,
            capabilities,
            quirks,
        } = config;
        let mut mem = Memory::new();
        mem.set_file_capability(capabilities.file);
        let stdin = io
            .stdin
            .map(|s| mem.open_stream(s).with_access(Access::ReadOnly));
        let stdout = io.stdout.map(|s| mem.open_stream(s));
        let stderr = io.stderr.map(|s| mem.open_stream(s));
        let run_buffer = RunStream::default();
        let run_file = mem
            .open_stream(Box::new(run_buffer.clone()))
            .with_access(Access::ReadOnly)
            .expect("file objects carry access");
        let ops = ops::table();

        mem.set_global(true);
        let systemdict = mem.new_dict(u32::try_from(ops.len()).unwrap_or(u32::MAX));
        let globaldict = mem.new_dict(200);
        let statusdict = mem.new_dict(16);
        let page_device = mem.new_dict(32);
        mem.set_global(false);
        let userdict = mem.new_dict(200);
        let errordict = mem.new_dict(32);
        let error = mem.new_dict(16);

        let mut name = |text: &str| mem.intern(text.as_bytes()).expect("short name");
        let atoms = Atoms {
            newerror: name("newerror"),
            errorname: name("errorname"),
            command: name("command"),
            ostack: name("ostack"),
            estack: name("estack"),
            dstack: name("dstack"),
            recordstacks: name("recordstacks"),
            handleerror: name("handleerror"),
        };

        let mut interp = Interp {
            mem,
            ostack: Vec::new(),
            dstack: vec![systemdict, globaldict, userdict],
            estack: Vec::new(),
            limits,
            ops,
            stdin: stdin.flatten(),
            stdout,
            stderr,
            run_file,
            run_buffer,
            graphics: None,
            gstate_floors: Vec::new(),
            page_device,
            quirks,
            dicts: StandardDicts {
                systemdict,
                globaldict,
                userdict,
                errordict,
                error,
                statusdict,
            },
            atoms,
            dstack_floor: 3,
            exec_count: 0,
            stopped: false,
            pending_error: None,
            quit: false,
            #[cfg(debug_assertions)]
            host_depth: 0,
            #[cfg(debug_assertions)]
            max_host_depth: 0,
        };
        interp.populate();
        interp
    }

    fn populate(&mut self) {
        let dicts = self.dicts;
        let ops = self.ops;
        for (index, entry) in ops.iter().enumerate() {
            let dict = match entry.visibility {
                Visibility::Public => dicts.systemdict,
                Visibility::Internal => dicts.errordict,
                Visibility::Graphics => continue,
            };
            let key = self.intern(entry.name);
            let op = Object::operator(u32::try_from(index).expect("table fits in u32"));
            self.mem.dict_put(dict, key, op).expect("fresh dictionary");
        }
        // The standard dictionaries are entries of systemdict whatever VM
        // they live in; they are older than any save, so the global/local
        // rule has nothing to protect and the raw insert is used.
        let constants = [
            ("true", Object::boolean(true)),
            ("false", Object::boolean(false)),
            ("null", Object::null()),
            ("languagelevel", Object::integer(2)),
            ("systemdict", dicts.systemdict),
            ("globaldict", dicts.globaldict),
            ("userdict", dicts.userdict),
            ("errordict", dicts.errordict),
            ("$error", dicts.error),
            ("statusdict", dicts.statusdict),
        ];
        for (name, value) in constants {
            let key = self.intern(name);
            self.mem
                .dict_mut(dicts.systemdict)
                .expect("systemdict exists")
                .insert(key, value);
        }
        self.mem
            .dict_set_access(dicts.systemdict, Access::ReadOnly)
            .expect("systemdict exists");

        ops::pagedevice::seed(self).expect("fresh dictionary");

        let atoms = self.atoms;
        for (key, value) in [
            (atoms.newerror, Object::boolean(false)),
            (atoms.errorname, Object::null()),
            (atoms.command, Object::null()),
            (atoms.recordstacks, Object::boolean(true)),
        ] {
            self.mem
                .dict_put(dicts.error, key, value)
                .expect("fresh dictionary");
        }
    }

    // --- graphics ------------------------------------------------------------

    /// Installs the graphics backend and defines the graphics operators in
    /// `systemdict`. Until this is called, `moveto` and the rest of the
    /// group are undefined names. The backend is told the current page
    /// size so its media box agrees with `currentpagedevice`.
    pub fn set_graphics_backend(&mut self, backend: Box<dyn GraphicsBackend>) {
        let mut backend = backend;
        if let Some(media_box) = ops::pagedevice::media_box(self) {
            let _ = backend.set_media_box(media_box);
        }
        let first = self.graphics.is_none();
        self.graphics = Some(backend);
        if !first {
            return;
        }
        let systemdict = self.dicts.systemdict;
        for (index, entry) in self.ops.iter().enumerate() {
            if entry.visibility != Visibility::Graphics {
                continue;
            }
            let key = self.intern(entry.name);
            let op = Object::operator(u32::try_from(index).expect("table fits in u32"));
            // systemdict is read-only by now; the entries predate every
            // save and are global, so the raw insert is safe.
            self.mem
                .dict_mut(systemdict)
                .expect("systemdict exists")
                .insert(key, op);
        }
    }

    pub fn has_graphics_backend(&self) -> bool {
        self.graphics.is_some()
    }

    pub fn graphics_backend(&mut self) -> Option<&mut (dyn GraphicsBackend + 'static)> {
        self.graphics.as_deref_mut()
    }

    /// The backend a graphics operator dispatches to; `undefined` without
    /// one, which can only happen to an operator object obtained before
    /// the backend was removed, since the names are not defined otherwise.
    pub(crate) fn backend(&mut self) -> Result<&mut (dyn GraphicsBackend + 'static), VmError> {
        self.graphics.as_deref_mut().ok_or(VmError::Undefined)
    }

    /// The page-device dictionary `currentpagedevice` returns.
    pub fn page_device(&self) -> Object {
        self.page_device
    }

    /// The depth `grestore` may not pop below: the state the innermost
    /// `save` left on the graphics-state stack.
    pub(crate) fn gstate_floor(&self) -> usize {
        self.gstate_floors.last().copied().unwrap_or(0)
    }

    pub(crate) fn push_gstate_floor(&mut self, floor: usize) {
        self.gstate_floors.push(floor);
    }

    /// Keeps one floor per live save after `restore` discarded the nested
    /// ones.
    pub(crate) fn truncate_gstate_floors(&mut self) {
        let live = self.mem.save_depth();
        self.gstate_floors.truncate(live);
    }

    // --- state -------------------------------------------------------------

    pub fn memory(&self) -> &Memory {
        &self.mem
    }

    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.mem
    }

    pub fn ostack(&self) -> &[Object] {
        &self.ostack
    }

    pub fn dstack(&self) -> &[Object] {
        &self.dstack
    }

    pub fn estack(&self) -> &[Frame] {
        &self.estack
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn dicts(&self) -> StandardDicts {
        self.dicts
    }

    /// Whether `quit` has been executed.
    pub fn has_quit(&self) -> bool {
        self.quit
    }

    /// The deepest nesting of the execution loop on the host stack seen so
    /// far; stays at one unless `run` is called from within a stream.
    #[cfg(debug_assertions)]
    pub fn max_host_depth(&self) -> u32 {
        self.max_host_depth
    }

    /// The file object behind `print` and `=`, if a stream was injected.
    pub fn stdout_file(&self) -> Option<Object> {
        self.stdout
    }

    pub fn stderr_file(&self) -> Option<Object> {
        self.stderr
    }

    /// The file object `%stdin` opens, if a stream was injected.
    pub fn stdin_file(&self) -> Option<Object> {
        self.stdin
    }

    /// The file object reading the job's source: what `currentfile` returns
    /// at the top level of a run.
    pub fn run_file(&self) -> Object {
        self.run_file
    }

    /// `currentfile`: the file of the innermost file frame on the execution
    /// stack, the job's own source when no file is being executed.
    pub fn current_file(&self) -> Object {
        self.estack
            .iter()
            .rev()
            .find_map(|frame| match frame {
                Frame::Source(frame) => match frame.slot {
                    SourceSlot::File { object, .. } => Some(object),
                    SourceSlot::Run => Some(self.run_file),
                    SourceSlot::String(_) => None,
                },
                _ => None,
            })
            .unwrap_or(self.run_file)
    }

    /// Discards the unread remainder of the job's source: what `closefile`
    /// on it means, and what keeps bytes a `quit` left behind from
    /// preceding the next job. The entry itself stays open, so it is never
    /// newer than a `save`.
    pub(crate) fn discard_run_input(&mut self) {
        self.run_buffer.0.borrow_mut().clear();
        let _ = self.mem.file_read(self.run_file, &mut [0u8; 1]);
    }

    /// Moves the bytes `source` has available into the run file.
    pub(crate) fn pump_run_source(
        &mut self,
        source: &mut dyn crate::source::Source,
    ) -> Result<(), VmError> {
        let mut bytes = Vec::new();
        source.drain_into(&mut self.mem, &mut bytes)?;
        if !bytes.is_empty() {
            self.run_buffer.0.borrow_mut().extend(bytes);
        }
        Ok(())
    }

    /// A literal name object for `text`, which must fit the name limit.
    pub fn intern(&mut self, text: &str) -> Object {
        self.mem
            .intern(text.as_bytes())
            .expect("interpreter names are short")
    }

    /// The operator object `systemdict` defines under `name`: a public
    /// operator, or a graphics operator once a backend is installed.
    pub fn operator(&self, name: &str) -> Option<Object> {
        ops::find(name, Visibility::Public)
            .or_else(|| {
                self.graphics
                    .as_ref()
                    .and_then(|_| ops::find(name, Visibility::Graphics))
            })
            .map(Object::operator)
    }

    /// `def` into the current dictionary.
    pub fn define(&mut self, name: &str, value: Object) -> Result<(), VmError> {
        let key = self.intern(name);
        let dict = self.current_dict();
        self.mem.dict_put(dict, key, value)
    }

    // --- name lookup ---------------------------------------------------------

    /// The value of `key` through the dictionary stack, top-down. String
    /// keys must already have been converted with `Memory::dict_key`.
    pub fn lookup(&self, key: Object) -> Option<Object> {
        lookup_in(&self.dstack, &self.mem, key)
    }

    /// The topmost dictionary on the stack that defines `key`.
    pub fn find_dict(&self, key: Object) -> Option<Object> {
        self.dstack
            .iter()
            .rev()
            .copied()
            .find(|&d| self.mem.dict(d).is_some_and(|dict| dict.contains(key)))
    }

    pub fn current_dict(&self) -> Object {
        *self.dstack.last().expect("permanent dictionaries")
    }

    pub(crate) fn errordict_get(&self, key: Object) -> Option<Object> {
        self.mem.dict(self.dicts.errordict)?.get(key)
    }

    pub(crate) fn error_get(&self, key: Object) -> Option<Object> {
        self.mem.dict(self.dicts.error)?.get(key)
    }

    pub(crate) fn error_put(&mut self, key: Object, value: Object) {
        let _ = self.mem.dict_put(self.dicts.error, key, value);
    }

    // --- operand stack -------------------------------------------------------

    pub fn push(&mut self, object: Object) -> Result<(), VmError> {
        if self.ostack.len() >= self.limits.operand {
            return Err(VmError::StackOverflow);
        }
        self.ostack.push(object);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Object, VmError> {
        self.ostack.pop().ok_or(VmError::StackUnderflow)
    }

    /// The object `n` below the top without popping it.
    pub fn peek(&self, n: usize) -> Result<Object, VmError> {
        self.ostack
            .len()
            .checked_sub(n + 1)
            .map(|i| self.ostack[i])
            .ok_or(VmError::StackUnderflow)
    }

    pub fn pop_int(&mut self) -> Result<i32, VmError> {
        let object = self.peek(0)?;
        let value = object.as_i32().ok_or(VmError::TypeCheck)?;
        self.pop()?;
        Ok(value)
    }

    pub fn pop_num(&mut self) -> Result<Num, VmError> {
        let object = self.peek(0)?;
        let value = Num::of(object).ok_or(VmError::TypeCheck)?;
        self.pop()?;
        Ok(value)
    }

    pub fn pop_bool(&mut self) -> Result<bool, VmError> {
        let object = self.peek(0)?;
        let value = object.as_bool().ok_or(VmError::TypeCheck)?;
        self.pop()?;
        Ok(value)
    }

    fn pop_typed(&mut self, accept: fn(Type) -> bool) -> Result<Object, VmError> {
        let object = self.peek(0)?;
        if !accept(object.ty()) {
            return Err(VmError::TypeCheck);
        }
        self.pop()
    }

    pub fn pop_dict(&mut self) -> Result<Object, VmError> {
        self.pop_typed(|t| t == Type::Dict)
    }

    /// An array or packed array.
    pub fn pop_array(&mut self) -> Result<Object, VmError> {
        self.pop_typed(|t| matches!(t, Type::Array | Type::PackedArray))
    }

    pub fn pop_string(&mut self) -> Result<Object, VmError> {
        self.pop_typed(|t| t == Type::String)
    }

    // --- dictionary stack ----------------------------------------------------

    /// `begin`
    pub fn push_dict(&mut self, dict: Object) -> Result<(), VmError> {
        if dict.ty() != Type::Dict {
            return Err(VmError::TypeCheck);
        }
        if self.dstack.len() >= self.limits.dict {
            return Err(VmError::DictStackOverflow);
        }
        self.dstack.push(dict);
        Ok(())
    }

    /// `end`; the permanent dictionaries cannot be popped.
    pub fn end_dict(&mut self) -> Result<Object, VmError> {
        if self.dstack.len() <= self.dstack_floor {
            return Err(VmError::DictStackUnderflow);
        }
        Ok(self.dstack.pop().expect("above the floor"))
    }

    pub(crate) fn dstack_floor(&self) -> usize {
        self.dstack_floor
    }

    // --- execution stack -----------------------------------------------------

    /// Number of frames counted toward the execution-stack limit.
    pub fn exec_count(&self) -> usize {
        self.exec_count
    }

    pub(crate) fn push_frame(&mut self, frame: Frame) -> Result<(), VmError> {
        if frame.is_counted() && self.exec_count >= self.limits.exec {
            return Err(VmError::ExecStackOverflow);
        }
        self.push_frame_unchecked(frame);
        Ok(())
    }

    // For the run boundary and the error machinery, which must make
    // progress even when the stack is full.
    pub(crate) fn push_frame_unchecked(&mut self, frame: Frame) {
        if frame.is_counted() {
            self.exec_count += 1;
        }
        self.estack.push(frame);
    }

    pub(crate) fn pop_frame(&mut self) -> Option<Frame> {
        let frame = self.estack.pop()?;
        if frame.is_counted() {
            self.exec_count -= 1;
        }
        Some(frame)
    }

    pub(crate) fn truncate_frames(&mut self, len: usize) {
        while self.estack.len() > len {
            self.pop_frame();
        }
    }

    /// Arranges for `array` to run as a procedure; an empty one needs no
    /// frame.
    pub(crate) fn push_proc(&mut self, array: Object) -> Result<(), VmError> {
        if array.length() == Some(0) {
            return Ok(());
        }
        self.push_frame(Frame::Proc { array, next: 0 })
    }

    /// Executes `object` as `exec` would: a literal is pushed, anything else
    /// runs next.
    pub(crate) fn exec_indirect(&mut self, object: Object) -> Result<(), VmError> {
        if object.is_literal() {
            self.push(object)
        } else {
            self.push_frame(Frame::Object(object))
        }
    }

    // --- output --------------------------------------------------------------

    fn write_all(&mut self, file: Option<Object>, bytes: &[u8]) -> Result<(), VmError> {
        match file {
            Some(file) => self.write_file(file, bytes),
            None => Ok(()),
        }
    }

    /// Writes all of `bytes` to `file`; a stream that accepts nothing is
    /// `ioerror`.
    pub fn write_file(&mut self, file: Object, mut bytes: &[u8]) -> Result<(), VmError> {
        while !bytes.is_empty() {
            let n = self.mem.file_write(file, bytes)?;
            if n == 0 {
                return Err(VmError::IoError);
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }

    pub fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), VmError> {
        self.write_all(self.stdout, bytes)
    }

    pub fn write_stderr(&mut self, bytes: &[u8]) -> Result<(), VmError> {
        self.write_all(self.stderr, bytes)
    }
}

pub(crate) fn lookup_in(dstack: &[Object], mem: &Memory, key: Object) -> Option<Object> {
    dstack.iter().rev().find_map(|&d| mem.dict(d)?.get(key))
}
