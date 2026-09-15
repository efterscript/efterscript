// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Execution-stack frames.

use crate::graphics::{FormInfo, Seg};
use crate::names::Atom;
use crate::object::{Handle, Object};
use crate::ops::Num;
use crate::ops::image::ImageAcquisition;
use crate::ops::show::ShowFrame;
use crate::scanner::Scanner;
use crate::source::{FileSource, StringSource};

/// One entry of the execution stack. The loop always acts on the top frame.
#[derive(Debug)]
pub enum Frame {
    /// A single object to execute, reached through `exec` or a name.
    Object(Object),
    /// An executable array run element by element; `next` is the index of
    /// the element to execute next.
    Proc {
        array: Object,
        next: u32,
    },
    /// A file, string, or the job's input being scanned and executed.
    Source(Box<SourceFrame>),
    /// A loop operator's continuation.
    Loop(LoopFrame),
    /// Pushed by `stopped`; reaching it normally yields `false`.
    Stopped,
    Marker(Marker),
}

impl Frame {
    /// Whether the frame counts toward the execution-stack limit. Frames
    /// holding a program object do; the interpreter's own control frames do
    /// not, since each accompanies a counted frame.
    pub fn is_counted(&self) -> bool {
        matches!(
            self,
            Frame::Object(_) | Frame::Proc { .. } | Frame::Source(_)
        )
    }

    /// The object `execstack` reports for this frame; for a procedure, its
    /// unexecuted remainder.
    pub fn object(&self) -> Option<Object> {
        match self {
            Frame::Object(object) => Some(*object),
            Frame::Proc { array, next } => array
                .length()
                .and_then(|len| array.with_interval(*next, len.saturating_sub(*next))),
            Frame::Source(frame) => frame.slot.object(),
            Frame::Loop(_) | Frame::Stopped | Frame::Marker(_) => None,
        }
    }
}

/// A source being scanned, with the scanner state that may be mid-token.
#[derive(Debug)]
pub struct SourceFrame {
    pub slot: SourceSlot,
    pub scanner: Scanner<'static>,
}

/// Where a `Source` frame reads from.
#[derive(Clone, Copy, Debug)]
pub enum SourceSlot {
    /// The source handed to `Interp::run` or `Interp::resume`.
    Run,
    File {
        object: Object,
        source: FileSource,
    },
    String(StringSource),
}

impl SourceSlot {
    pub fn object(&self) -> Option<Object> {
        match self {
            SourceSlot::Run => None,
            SourceSlot::File { object, .. } => Some(*object),
            SourceSlot::String(source) => Some(source.object()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum LoopFrame {
    For {
        body: Object,
        current: Num,
        increment: Num,
        limit: Num,
        /// Set when the control variable can no longer advance.
        done: bool,
    },
    Repeat {
        body: Object,
        remaining: i32,
    },
    Loop {
        body: Object,
    },
    /// `forall` over an array, packed array, string, or dictionary; `next`
    /// is the element or entry index to visit.
    ForAll {
        body: Object,
        container: Object,
        next: u32,
    },
    /// `image` or `imagemask` collecting sample data from a procedure: the
    /// body runs until enough bytes have been delivered or it returns an
    /// empty string.
    ImageData {
        body: Object,
        acquisition: Box<ImageAcquisition>,
    },
    /// A filter whose source is a procedure needs bytes: the body runs
    /// once and the string it leaves is fed to the file entry `handle`,
    /// after which the read that wanted the bytes runs again.
    FilterData {
        body: Object,
        handle: Handle,
        started: bool,
    },
    /// A `show`-family operator or `stringwidth` in progress: glyphs are
    /// consumed one step at a time so a Type 3 glyph procedure or a
    /// `kshow` procedure can run as frames above it.
    Show(Box<ShowFrame>),
    /// `resourceforall`: each name in `keys` is written into `scratch` and
    /// the body runs with the filled interval; an integer key (the
    /// implicit categories') is pushed as it is.
    ResourceForAll {
        body: Object,
        keys: Vec<ResourceKey>,
        scratch: Object,
        next: usize,
    },
    /// `pathforall`: each segment of `segs` (already in user space) pushes
    /// its coordinates and runs the procedure of its kind — move, line,
    /// curve, close, in `procs` order.
    PathForAll {
        procs: [Object; 4],
        segs: Vec<Seg>,
        next: usize,
    },
    /// A tiling pattern's paint procedure running so the backend can
    /// capture its cell (PLRM3 §4.9.2), on behalf of the painting
    /// operator `operator`, whose operands stay on the operand stack:
    /// `body` runs once with `dict` as its operand; then the capture
    /// ends, the graphics state returns to `depth`, and the operator
    /// runs again. `started` is set while the capture is open, so a
    /// frame discarded by an error closes it. While an `uncoloured`
    /// cell runs, the colour operators are undefined.
    PatternCell {
        body: Object,
        dict: Object,
        depth: usize,
        operator: &'static str,
        uncoloured: bool,
        started: bool,
    },
    /// A form's paint procedure running so the backend can capture its
    /// body (PLRM3 §4.7): `body` runs once with `dict` as its operand;
    /// then the capture ends, the graphics state returns to `depth`, and
    /// the form is placed. `started` as for a pattern cell.
    FormBody {
        body: Object,
        dict: Object,
        depth: usize,
        info: FormInfo,
        started: bool,
    },
}

/// A resource instance's key as `resourceforall` enumerates it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceKey {
    Name(Vec<u8>),
    Int(i32),
}

impl LoopFrame {
    /// The procedure the loop runs; for a show frame, the glyph or
    /// `kshow` procedure it runs between its own steps (null if none).
    pub fn body(&self) -> Object {
        match self {
            LoopFrame::For { body, .. }
            | LoopFrame::Repeat { body, .. }
            | LoopFrame::Loop { body }
            | LoopFrame::ForAll { body, .. }
            | LoopFrame::ImageData { body, .. }
            | LoopFrame::FilterData { body, .. }
            | LoopFrame::ResourceForAll { body, .. }
            | LoopFrame::PatternCell { body, .. }
            | LoopFrame::FormBody { body, .. } => *body,
            LoopFrame::PathForAll { procs, .. } => procs[0],
            LoopFrame::Show(frame) => frame.procedure(),
        }
    }

    /// Every object the frame still needs, for `restore`'s check.
    pub fn references(&self) -> Vec<Object> {
        let mut objects = vec![self.body()];
        match self {
            LoopFrame::ForAll { container, .. } => objects.push(*container),
            LoopFrame::ResourceForAll { scratch, .. } => objects.push(*scratch),
            LoopFrame::ImageData { acquisition, .. } => objects.extend(&acquisition.sources),
            LoopFrame::PathForAll { procs, .. } => objects.extend(&procs[1..]),
            LoopFrame::Show(frame) => objects.extend(frame.references()),
            LoopFrame::PatternCell { dict, .. } | LoopFrame::FormBody { dict, .. } => {
                objects.push(*dict);
            }
            _ => {}
        }
        objects
    }
}

/// Barriers on the execution stack. `Interrupt` and `Timeout` are reserved
/// for the embedder and currently behave as no-ops when reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// Pushed by `Interp::run`; `stop` unwinds no further.
    RunBoundary,
    /// Below an `errordict` handler while it runs.
    ErrorHandler,
    /// Below an `eexec` source. When the frame ends, however it ends, the
    /// dictionary stack is cut back to `dicts` entries (dropping the
    /// `systemdict` pushed for the section) and the layer file, if the
    /// section is a file, is closed.
    Eexec {
        layer: Option<Handle>,
        dicts: usize,
    },
    /// Below the program text of a predefined CMap being loaded on
    /// behalf of the operator `retry` (an operator-table index), whose
    /// operands are still on the operand stack. The load runs in global
    /// allocation mode; when the frame ends, however it ends, the mode
    /// returns to `global`. Reached normally, the loaded resource is
    /// moved into the predefined table and `retry` runs again.
    CMapLoad {
        name: Atom,
        global: bool,
        retry: u32,
    },
    Interrupt,
    Timeout,
}
