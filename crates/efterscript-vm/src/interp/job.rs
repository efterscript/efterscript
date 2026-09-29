// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The job server (PLRM3 §3.7.7): an interpreter serving a sequence of
//! jobs, each begun from the same initial state and reverted to it at
//! its end, unless `startjob` or `exitserver` made it unencapsulated.
//!
//! A job's outermost save captures local and global VM and the tables
//! the interpreter derives from objects in them ([`Derived`]); ending
//! the job restores all three. System parameters, the page count, the
//! name table, and identifier counters are the server's and never
//! revert: counters only grow, so an identifier from a reverted job is
//! never issued again.

use std::collections::HashMap;
use std::rc::Rc;

use efterscript_fonts::Program;
use efterscript_fonts::cmap::CMap;

use crate::error::VmError;
use crate::graphics::{FontRef, Matrix, ProcRef, Screen};
use crate::interp::{Interp, PatternInstance, VmGState};
use crate::object::{CompositeRef, Object};
use crate::ops::cie::CieEntry;

/// The tables derived from VM objects, as a job's outermost save found
/// them. Cloning is proportional to the number of entries: programs and
/// CMaps are shared.
#[derive(Clone)]
pub(crate) struct Derived {
    vm_gstate: VmGState,
    vm_gstates: Vec<VmGState>,
    font_instances: Vec<Object>,
    instance_index: HashMap<CompositeRef, u32>,
    defined_matrices: HashMap<u32, Matrix>,
    font_without_backend: Option<FontRef>,
    screens_without_backend: [Screen; 4],
    transfers_without_backend: [ProcRef; 4],
    graphics_procs: Vec<Object>,
    graphics_proc_index: HashMap<CompositeRef, ProcRef>,
    color_rendering_without_backend: Option<ProcRef>,
    default_color_rendering: Option<Object>,
    cie_spaces: Vec<CieEntry>,
    cie_index: HashMap<CompositeRef, u32>,
    resident_fonts: [Option<Object>; efterscript_fonts::ResidentFace::COUNT],
    loaded_procsets: [bool; 2],
    font_programs: HashMap<u32, Rc<Program>>,
    cid_programs: HashMap<Vec<u8>, Rc<Program>>,
    cmaps: HashMap<u32, Rc<CMap>>,
    predefined_cmaps: HashMap<Vec<u8>, Object>,
    pattern_instances: Vec<PatternInstance>,
}

impl Derived {
    fn capture(i: &Interp) -> Self {
        Derived {
            vm_gstate: i.vm_gstate,
            vm_gstates: i.vm_gstates.clone(),
            font_instances: i.font_instances.clone(),
            instance_index: i.instance_index.clone(),
            defined_matrices: i.defined_matrices.clone(),
            font_without_backend: i.font_without_backend,
            screens_without_backend: i.screens_without_backend,
            transfers_without_backend: i.transfers_without_backend,
            graphics_procs: i.graphics_procs.clone(),
            graphics_proc_index: i.graphics_proc_index.clone(),
            color_rendering_without_backend: i.color_rendering_without_backend,
            default_color_rendering: i.default_color_rendering,
            cie_spaces: i.cie_spaces.clone(),
            cie_index: i.cie_index.clone(),
            resident_fonts: i.resident_fonts,
            loaded_procsets: i.loaded_procsets,
            font_programs: i.font_programs.clone(),
            cid_programs: i.cid_programs.clone(),
            cmaps: i.cmaps.clone(),
            predefined_cmaps: i.predefined_cmaps.clone(),
            pattern_instances: i.pattern_instances.clone(),
        }
    }

    fn reinstate(self, i: &mut Interp) {
        i.vm_gstate = self.vm_gstate;
        i.vm_gstates = self.vm_gstates;
        i.font_instances = self.font_instances;
        i.instance_index = self.instance_index;
        i.defined_matrices = self.defined_matrices;
        i.font_without_backend = self.font_without_backend;
        i.screens_without_backend = self.screens_without_backend;
        i.transfers_without_backend = self.transfers_without_backend;
        i.graphics_procs = self.graphics_procs;
        i.graphics_proc_index = self.graphics_proc_index;
        i.color_rendering_without_backend = self.color_rendering_without_backend;
        i.default_color_rendering = self.default_color_rendering;
        i.cie_spaces = self.cie_spaces;
        i.cie_index = self.cie_index;
        i.resident_fonts = self.resident_fonts;
        i.loaded_procsets = self.loaded_procsets;
        i.font_programs = self.font_programs;
        i.cid_programs = self.cid_programs;
        i.cmaps = self.cmaps;
        i.predefined_cmaps = self.predefined_cmaps;
        i.pattern_instances = self.pattern_instances;
    }

    /// Entry counts of the growing tables, for the check that an empty
    /// encapsulated job leaves them as it found them.
    #[cfg(debug_assertions)]
    fn sizes(i: &Interp) -> [usize; 12] {
        [
            i.font_instances.len(),
            i.instance_index.len(),
            i.defined_matrices.len(),
            i.graphics_procs.len(),
            i.graphics_proc_index.len(),
            i.cie_spaces.len(),
            i.cie_index.len(),
            i.font_programs.len(),
            i.cid_programs.len(),
            i.cmaps.len(),
            i.predefined_cmaps.len(),
            i.pattern_instances.len(),
        ]
    }
}

/// The job being served.
pub(crate) struct JobState {
    /// The outermost save, `None` while the job is unencapsulated.
    save: Option<Object>,
    /// The save depth at the job's start, which `startjob` requires.
    start_depth: usize,
    /// Started with the system-parameter password (PLRM3 §C.3.1).
    admin: bool,
    /// Whether any part of the job was unencapsulated.
    permanent: bool,
}

/// How a job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobEnd {
    /// Whether the job changed the initial state of later jobs.
    pub permanent: bool,
}

impl Interp {
    /// Begins an encapsulated job: establishes the initial state and
    /// takes the outermost save of local and global VM and the derived
    /// tables. `invalidaccess` if a job is already open.
    pub fn begin_job(&mut self) -> Result<(), VmError> {
        if self.job.is_some() {
            return Err(VmError::InvalidAccess);
        }
        self.reset_for_job();
        self.job = Some(JobState {
            save: None,
            start_depth: 0,
            admin: false,
            permanent: false,
        });
        self.enter_job(true)
    }

    /// Ends the open job: clears the stacks and, if it is encapsulated,
    /// restores the outermost save; an unencapsulated job's pending saves
    /// are restored to the outermost of them. The graphics backend is the
    /// caller's to have removed. With no job open, nothing happens.
    pub fn end_job(&mut self) -> JobEnd {
        #[cfg(debug_assertions)]
        let sizes = self.job_sizes.take();
        let Some(job) = self.job.take() else {
            return JobEnd { permanent: false };
        };
        self.clear_stacks();
        self.estack.clear();
        let outermost = job.save.or_else(|| self.mem.outermost_save());
        if let Some(save) = outermost {
            let restored = self.vm_restore(save);
            debug_assert!(restored.is_ok(), "a job's outermost save restores");
        }
        self.reset_run_input();
        self.server_level = false;
        #[cfg(debug_assertions)]
        if let Some(sizes) = sizes
            && !job.permanent
        {
            debug_assert_eq!(sizes, Derived::sizes(self), "derived state leaked");
        }
        JobEnd {
            permanent: job.permanent,
        }
    }

    /// Whether a job is open.
    pub fn in_job(&self) -> bool {
        self.job.is_some()
    }

    /// Whether the open job may change system parameters without a
    /// password: an administrator job, or the prelude.
    pub(crate) fn admin_job(&self) -> bool {
        self.prelude_running || self.job.as_ref().is_some_and(|j| j.admin)
    }

    /// `bool password startjob` (PLRM3 §3.7.7, §8.2): whether a new job
    /// was started. Under a job server, with a password matching
    /// `StartJobPassword` or `SystemParamsPassword`, at the save depth the
    /// job started at, and with nothing on the execution stack the
    /// restore would reject, the current job ends and a new one begins —
    /// unencapsulated when `persistent` — reading the same input.
    pub(crate) fn start_job(&mut self, persistent: bool, password: &[u8]) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let params = self.system_params();
        let admin = params.is_system_password(password);
        if !admin && !params.start_job_allowed(password) {
            return false;
        }
        if self.mem.save_depth() != job.start_depth {
            return false;
        }
        if let Some(save) = job.save {
            let references = self.exec_references();
            let blocked = self
                .mem
                .save_record(save)
                .is_some_and(|record| references.iter().any(|&o| record.outlived_by(o)));
            if blocked {
                return false;
            }
        }
        let save = job.save;
        self.clear_stacks();
        if let Some(save) = save {
            let restored = self.vm_restore(save);
            debug_assert!(restored.is_ok(), "the job's save restores once checked");
        }
        let job = self.job.as_mut().expect("checked above");
        job.admin = admin;
        job.permanent |= persistent;
        if self.enter_job(!persistent).is_err() {
            // Unreachable: the depth was the job's own, below the limit.
            return false;
        }
        true
    }

    /// Starts the job's execution proper: encapsulated behind a save
    /// covering both VMs and the derived tables, or unencapsulated.
    fn enter_job(&mut self, encapsulated: bool) -> Result<(), VmError> {
        self.mem.set_global(false);
        self.mem.set_packing(false);
        self.server_level = !encapsulated;
        let save = if encapsulated {
            #[cfg(debug_assertions)]
            {
                self.job_sizes = Some(Derived::sizes(self));
            }
            Some(self.vm_save(true)?)
        } else {
            #[cfg(debug_assertions)]
            {
                self.job_sizes = None;
            }
            None
        };
        let depth = self.mem.save_depth();
        let job = self.job.as_mut().expect("a job is open");
        job.save = save;
        job.start_depth = depth;
        Ok(())
    }

    /// Whether a `save` now would be the outermost level of an
    /// unencapsulated job, and so capture global VM too.
    pub(crate) fn save_covers_global(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.save.is_none() && self.mem.save_depth() == 0)
    }

    /// `save` without the operand stack: the implicit graphics save when
    /// a backend is installed, the VM snapshot, and the derived tables
    /// alongside it when it covers global VM.
    pub(crate) fn vm_save(&mut self, global: bool) -> Result<Object, VmError> {
        let depth = self
            .graphics_backend()
            .map(|backend| backend.gstate_depth());
        if depth.is_some() {
            self.gsave()?;
        }
        let derived = global.then(|| Derived::capture(self));
        let save = match self.mem.save_with(depth.unwrap_or(0), global) {
            Ok(save) => save,
            Err(e) => {
                if depth.is_some() {
                    let _ = self.grestore();
                }
                return Err(e);
            }
        };
        if let Some(derived) = derived {
            self.derived_saves
                .push((save.as_save().expect("a save object"), derived));
        }
        self.push_gstate_floor(depth.map_or(0, |d| d + 1));
        Ok(save)
    }

    /// `restore` without the operand stack: rejects a save something on
    /// the stacks outlived, reverts VM, pops the graphics state back, and
    /// reinstates the derived tables of a save that covered global VM.
    pub(crate) fn vm_restore(&mut self, save: Object) -> Result<(), VmError> {
        let references = self.exec_references();
        let (ostack, dstack) = (self.ostack.clone(), self.dstack.clone());
        let depth = self.mem.restore(save, &[&ostack, &dstack, &references])?;
        self.truncate_gstate_floors();
        let serial = save.as_save().expect("restore checked the type");
        let mut covered_global = false;
        if let Some(at) = self.derived_saves.iter().position(|(s, _)| *s >= serial) {
            let mut discarded = self.derived_saves.split_off(at);
            if discarded[0].0 == serial {
                discarded.swap_remove(0).1.reinstate(self);
                covered_global = true;
            }
        }
        if self.has_graphics_backend() {
            if covered_global {
                self.define_graphics_operators();
            }
            self.grestore_to(depth)?;
        }
        Ok(())
    }

    /// The operand stack emptied and the dictionary stack back at the
    /// standard dictionaries.
    fn clear_stacks(&mut self) {
        self.ostack.clear();
        self.dstack.truncate(self.dstack_floor);
    }

    /// The per-job reset (PLRM3 §3.7.7 step 3) of what lives outside VM.
    fn reset_for_job(&mut self) {
        self.clear_stacks();
        self.estack.clear();
        self.pending_error = None;
        self.stopped = false;
        self.quit = false;
        self.starved = false;
        self.steps = 0;
        self.steps_limit = self.limits.steps;
        self.grace_given = false;
        self.substitutions.clear();
        self.declared_bbox = None;
        self.declared_device_bbox = None;
        self.cmap_builders.clear();
        self.paint_procedures = 0;
        self.uncoloured_cells = 0;
        self.reset_run_input();
        let newerror = self.atoms.newerror;
        self.error_put(newerror, Object::boolean(false));
    }
}
