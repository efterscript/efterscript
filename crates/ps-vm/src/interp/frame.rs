// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Execution-stack frames.

use crate::object::Object;
use crate::ops::Num;
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

#[derive(Clone, Copy, Debug)]
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
}

/// Barriers on the execution stack. `Interrupt` and `Timeout` are reserved
/// for the embedder and currently behave as no-ops when reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// Pushed by `Interp::run`; `stop` unwinds no further.
    RunBoundary,
    /// Below an `errordict` handler while it runs.
    ErrorHandler,
    Interrupt,
    Timeout,
}
