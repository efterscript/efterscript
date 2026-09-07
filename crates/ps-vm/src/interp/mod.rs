// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The interpreter: stacks, the execution loop, name lookup, and the error
//! machinery (PLRM3 §3.5, §3.10, §3.12).

mod exec;
mod frame;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use ps_fonts::cmap::CMapBuilder;
use ps_fonts::{CMap, Program};

use crate::error::VmError;
use crate::files::{FileCapability, Stream};
use crate::graphics::{FontRef, GraphicsBackend, MarkValue, Matrix};
use crate::io::Io;
use crate::memory::Memory;
use crate::object::{Access, CompositeRef, Handle, Object, Type};
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
    /// The execution budget: how many objects (loop iterations included)
    /// a run may execute before `limitcheck` is raised on the one being
    /// executed. `None` leaves execution unbounded. Once raised, the
    /// budget grows by a grace of one sixteenth (at least 1000) so an
    /// error handler can report; the second exceed is raised again and
    /// every object after it, so a handler that loops ends the job.
    pub steps: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            operand: 500,
            dict: 20,
            exec: 250,
            steps: None,
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

/// Font behaviour the embedder chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontConfig {
    /// Whether `findfont` of a name no font is defined under resolves to
    /// one of the resident fonts by name; `false` raises `invalidfont`,
    /// as the reference specifies.
    pub substitute: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig { substitute: true }
    }
}

/// One `findfont` that resolved through substitution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontSubstitution {
    /// The key the program asked for.
    pub requested: Vec<u8>,
    /// The PostScript name of the resident font it received.
    pub substitute: &'static str,
}

#[derive(Default)]
pub struct Config {
    pub limits: Limits,
    pub io: Io,
    pub capabilities: Capabilities,
    pub quirks: Quirks,
    pub fonts: FontConfig,
}

/// A resource category's instance dictionaries: one per VM, so
/// `defineresource` follows the allocation mode and `restore` reverts
/// only the local one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Category {
    pub local: Object,
    pub global: Object,
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
    /// The `currentdistillerparams` dictionary, in global VM.
    distiller_params: Object,
    pub(crate) fonts_config: FontConfig,
    /// `FontDirectory` and `GlobalFontDirectory`.
    pub(crate) font_category: Category,
    pub(crate) encoding_category: Category,
    pub(crate) procset_category: Category,
    pub(crate) fontset_category: Category,
    pub(crate) cmap_category: Category,
    pub(crate) cidfont_category: Category,
    /// The built-in `FontSetInit` procedure set.
    pub(crate) font_set_init: Object,
    /// The built-in `CIDInit` procedure set.
    pub(crate) cid_init: Object,
    /// Which built-in procedure sets `findresource` has returned, in
    /// the resource operators' table order.
    pub(crate) loaded_procsets: [bool; 2],
    pub(crate) standard_encoding: Object,
    pub(crate) iso_latin1_encoding: Object,
    pub(crate) resident_fonts: [Option<Object>; ps_fonts::ResidentFace::COUNT],
    // The font instance table: the graphics state names a font by its
    // index here (D1). Entries are never removed; one that `restore`
    // invalidated is never looked up again, because the graphics state
    // that referred to it was restored too.
    font_instances: Vec<Object>,
    instance_index: HashMap<CompositeRef, u32>,
    // Instances the current backend has been told about.
    described_fonts: HashSet<u32>,
    // The `FontMatrix` each `FID` was first defined with, which derived
    // fonts compose their own from and which a backend records Type 3
    // glyphs against.
    defined_matrices: HashMap<u32, Matrix>,
    // The current font of a VM without a graphics backend, which has no
    // graphics state to keep it in.
    font_without_backend: Option<FontRef>,
    // Program snapshots by `FID`, built on the first glyph a font needs
    // and never invalidated: a job that alters its font dictionary
    // afterwards is not followed.
    font_programs: HashMap<u32, Rc<Program>>,
    // CID-keyed programs loaded through `StartData`, by the name the CFF
    // gives them; nothing in the PostScript-visible font machinery
    // refers to them until a composite font does.
    cid_programs: HashMap<Vec<u8>, Rc<Program>>,
    // CMaps built by `endcmap`, by the id their dictionary's `CodeMap`
    // entry carries.
    cmaps: HashMap<u32, Rc<CMap>>,
    next_cmap_id: u32,
    // The predefined CMaps loaded so far: global read-only dictionaries
    // outside every category dictionary, so `resourcestatus` reports
    // them as loaded rather than defined.
    predefined_cmaps: HashMap<Vec<u8>, Object>,
    // The CMap programs between `begincmap` and `endcmap`, innermost
    // last: loading a predefined parent runs its program inside.
    pub(crate) cmap_builders: Vec<CMapBuilder>,
    next_fid: u32,
    substitutions: Vec<FontSubstitution>,
    #[allow(dead_code)]
    pub(crate) quirks: Quirks,
    pub(crate) dicts: StandardDicts,
    pub(crate) atoms: Atoms,
    dstack_floor: usize,
    exec_count: usize,
    // Objects executed so far, against `Limits::steps`.
    steps: u64,
    // The budget currently in force: the configured one, then once
    // extended by the grace after the first exceed.
    steps_limit: Option<u64>,
    grace_given: bool,
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
            fonts,
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
        let distiller_params = mem.new_dict(32);
        let global_font_directory = mem.new_dict(32);
        let global_encodings = mem.new_dict(8);
        let global_procsets = mem.new_dict(8);
        let global_fontsets = mem.new_dict(8);
        let global_cmaps = mem.new_dict(8);
        let global_cidfonts = mem.new_dict(8);
        let font_set_init = ops::fontset::init_dict(&mut mem).expect("fresh dictionary");
        let cid_init = ops::cidinit::init_dict(&mut mem).expect("fresh dictionary");
        let standard_encoding = ops::font::encoding_array(&mut mem, &ps_fonts::STANDARD_ENCODING)
            .expect("names are simple objects");
        let iso_latin1_encoding =
            ops::font::encoding_array(&mut mem, &ps_fonts::ISO_LATIN1_ENCODING)
                .expect("names are simple objects");
        mem.set_global(false);
        let userdict = mem.new_dict(200);
        let errordict = mem.new_dict(32);
        let error = mem.new_dict(16);
        let font_directory = mem.new_dict(32);
        let local_encodings = mem.new_dict(8);
        let local_procsets = mem.new_dict(8);
        let local_fontsets = mem.new_dict(8);
        let local_cmaps = mem.new_dict(8);
        let local_cidfonts = mem.new_dict(8);

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
            distiller_params,
            fonts_config: fonts,
            font_category: Category {
                local: font_directory,
                global: global_font_directory,
            },
            encoding_category: Category {
                local: local_encodings,
                global: global_encodings,
            },
            procset_category: Category {
                local: local_procsets,
                global: global_procsets,
            },
            fontset_category: Category {
                local: local_fontsets,
                global: global_fontsets,
            },
            cmap_category: Category {
                local: local_cmaps,
                global: global_cmaps,
            },
            cidfont_category: Category {
                local: local_cidfonts,
                global: global_cidfonts,
            },
            font_set_init,
            cid_init,
            loaded_procsets: [false; 2],
            standard_encoding,
            iso_latin1_encoding,
            resident_fonts: [None; ps_fonts::ResidentFace::COUNT],
            font_instances: Vec::new(),
            instance_index: HashMap::new(),
            described_fonts: HashSet::new(),
            defined_matrices: HashMap::new(),
            font_without_backend: None,
            font_programs: HashMap::new(),
            cid_programs: HashMap::new(),
            cmaps: HashMap::new(),
            next_cmap_id: 0,
            predefined_cmaps: HashMap::new(),
            cmap_builders: Vec::new(),
            next_fid: 0,
            substitutions: Vec::new(),
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
            steps: 0,
            steps_limit: limits.steps,
            grace_given: false,
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
                Visibility::Graphics | Visibility::ProcSet => continue,
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
            ("FontDirectory", self.font_category.local),
            ("GlobalFontDirectory", self.font_category.global),
            ("StandardEncoding", self.standard_encoding),
            ("ISOLatin1Encoding", self.iso_latin1_encoding),
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
        ops::distiller::seed(self).expect("fresh dictionary");

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
        self.described_fonts.clear();
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

    /// The dictionary `currentdistillerparams` copies.
    pub fn distiller_params(&self) -> Object {
        self.distiller_params
    }

    /// Stores `entries` as the current distillation parameters without
    /// telling the backend: how an embedder whose writer starts from
    /// other values than the built-in defaults keeps the job's view of
    /// them in step. A job's own `setdistillerparams` merges over these.
    pub fn set_distiller_params(
        &mut self,
        entries: &[(Vec<u8>, MarkValue)],
    ) -> Result<(), VmError> {
        ops::distiller::put_values(self, entries)
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

    // --- fonts ---------------------------------------------------------------

    /// The current font: the graphics state's, or the VM's own slot when
    /// no backend is installed.
    pub fn current_font(&self) -> Option<FontRef> {
        match &self.graphics {
            Some(backend) => backend.font(),
            None => self.font_without_backend,
        }
    }

    pub(crate) fn set_current_font(&mut self, font: Option<FontRef>) -> Result<(), VmError> {
        match self.graphics.as_deref_mut() {
            Some(backend) => backend.set_font(font),
            None => {
                self.font_without_backend = font;
                Ok(())
            }
        }
    }

    /// The instance id the graphics state refers to `dict` by, allocated
    /// on first use.
    pub(crate) fn font_instance(&mut self, dict: Object) -> Result<u32, VmError> {
        let key = dict.composite_ref().ok_or(VmError::TypeCheck)?;
        if let Some(&instance) = self.instance_index.get(&key) {
            return Ok(instance);
        }
        let instance = u32::try_from(self.font_instances.len()).map_err(|_| VmError::LimitCheck)?;
        self.font_instances.push(dict);
        self.instance_index.insert(key, instance);
        Ok(instance)
    }

    /// The font dictionary behind an instance id of a `FontRef`.
    pub fn font_dict(&self, instance: u32) -> Option<Object> {
        self.font_instances.get(instance as usize).copied()
    }

    /// Whether the backend has been told about `instance`.
    pub(crate) fn font_described(&self, instance: u32) -> bool {
        self.described_fonts.contains(&instance)
    }

    pub(crate) fn mark_font_described(&mut self, instance: u32) {
        self.described_fonts.insert(instance);
    }

    /// Records the matrix `fid` was defined with; a font redefined under
    /// another key keeps its first.
    pub(crate) fn record_defined_matrix(&mut self, fid: u32, matrix: Matrix) {
        self.defined_matrices.entry(fid).or_insert(matrix);
    }

    pub(crate) fn defined_matrix(&self, fid: u32) -> Option<Matrix> {
        self.defined_matrices.get(&fid).copied()
    }

    /// Every `findfont` so far that resolved through substitution, in
    /// order.
    pub fn font_substitutions(&self) -> &[FontSubstitution] {
        &self.substitutions
    }

    pub(crate) fn record_substitution(&mut self, requested: Vec<u8>, substitute: &'static str) {
        self.substitutions.push(FontSubstitution {
            requested,
            substitute,
        });
    }

    /// The glyph program of a Type 1 or Type 42 font dictionary, built
    /// from its `CharStrings` (and `Private` or `sfnts`) on first use and
    /// cached by `FID`, or of a FontType 2 dictionary, cached when its
    /// FontSet was loaded; `invalidfont` when the dictionary has none.
    pub fn font_program(&mut self, dict: Object) -> Result<Rc<Program>, VmError> {
        let fid = ops::font::entry(self, dict, "FID")?
            .and_then(Object::as_font_id)
            .ok_or(VmError::InvalidFont)?;
        if let Some(program) = self.font_programs.get(&fid) {
            return Ok(program.clone());
        }
        let program = Rc::new(ops::embedded::snapshot(self, dict)?);
        self.font_programs.insert(fid, program.clone());
        Ok(program)
    }

    /// Caches `program` as the glyph program of the font family `fid`.
    pub(crate) fn cache_font_program(&mut self, fid: u32, program: Rc<Program>) {
        self.font_programs.insert(fid, program);
    }

    pub(crate) fn cache_cid_program(&mut self, name: Vec<u8>, program: Rc<Program>) {
        self.cid_programs.insert(name, program);
    }

    /// A CID-keyed program a FontSet loaded, by the name its CFF gives
    /// it.
    pub fn cid_program(&self, name: &[u8]) -> Option<Rc<Program>> {
        self.cid_programs.get(name).cloned()
    }

    /// Keeps a CMap `endcmap` built and returns the id its dictionary's
    /// `CodeMap` entry carries.
    pub(crate) fn register_cmap(&mut self, cmap: Rc<CMap>) -> u32 {
        let id = self.next_cmap_id;
        self.next_cmap_id += 1;
        self.cmaps.insert(id, cmap);
        id
    }

    /// The CMap behind a `CodeMap` id.
    pub fn cmap(&self, id: u32) -> Option<Rc<CMap>> {
        self.cmaps.get(&id).cloned()
    }

    /// The dictionary of a predefined CMap already loaded.
    pub(crate) fn predefined_cmap(&self, name: &[u8]) -> Option<Object> {
        self.predefined_cmaps.get(name).copied()
    }

    pub(crate) fn cache_predefined_cmap(&mut self, name: Vec<u8>, dict: Object) {
        self.predefined_cmaps.insert(name, dict);
    }

    pub(crate) fn allocate_fid(&mut self) -> Object {
        let id = self.next_fid;
        self.next_fid += 1;
        Object::font_id(id)
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

    /// Objects executed so far, loop iterations included: what
    /// `Limits::steps` is measured against.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Whether the execution budget has been exceeded at least once.
    pub fn budget_exceeded(&self) -> bool {
        self.grace_given
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
        if let Frame::Loop(LoopFrame::Show(show)) = &frame {
            ops::show::abandon(self, show);
        }
        if let Frame::Marker(Marker::Eexec { layer, dicts }) = &frame {
            self.end_eexec(*layer, *dicts);
        }
        if let Frame::Marker(Marker::CMapLoad { global, .. }) = &frame {
            self.mem.set_global(*global);
        }
        Some(frame)
    }

    /// Ends an `eexec` section: the dictionary stack returns to its depth
    /// before the section's `systemdict` was pushed, and the layer is
    /// closed (a no-op when `closefile` already did).
    fn end_eexec(&mut self, layer: Option<Handle>, dicts: usize) {
        self.dstack.truncate(dicts.max(self.dstack_floor));
        if let Some(handle) = layer {
            let _ = self.mem.files_mut().close(handle);
        }
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
