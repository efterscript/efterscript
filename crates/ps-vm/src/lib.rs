// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! PostScript language VM.
//!
//! Scanner, object model, operand/dictionary/execution stacks, `save`/`restore`
//! VM semantics, Level 2 resource machinery, and full PostScript error
//! semantics. Graphics operators are dispatched to a trait implemented by
//! consumers (see `ps-graphics`); the VM itself holds no ambient authority —
//! file access, devices, and host-font enumeration exist only as capabilities
//! injected by the embedder.
//!
//! Independently useful as a PostScript scripting engine.

pub mod dict;
pub mod error;
pub mod files;
pub mod graphics;
pub mod interp;
pub mod io;
pub mod memory;
pub mod names;
pub mod object;
pub mod ops;
pub mod scanner;
pub mod source;

pub use dict::Dict;
pub use error::VmError;
pub use files::{FileCapability, FileTable, Stream};
pub use graphics::{
    Bounds, GraphicsBackend, ImageSpec, LineCap, LineJoin, Matrix, Point, Rect, Seg, SpaceSpec,
};
pub use interp::{
    Capabilities, Config, ErrorSummary, Frame, Interp, Limits, LoopFrame,
    MAX_NESTED_ERROR_HANDLERS, Marker, Outcome, Quirks, SourceFrame, SourceSlot, StandardDicts,
};
pub use io::{Capture, Io};
pub use memory::{
    Arena, GState, MAX_SAVE_DEPTH, Memory, PersistentMap, SaveRecord, Shared, Slot, check_store,
};
pub use names::{Atom, MAX_NAME_LEN, NameTable, NameTooLong};
pub use object::{Access, CompositeRef, Handle, Object, Space, Type};
pub use ops::{Num, OpEntry, OpFn, Sig, Visibility};
pub use scanner::{
    DscObserver, MAX_PROC_DEPTH, MAX_STRING_LEN, Resolver, Scan, ScanError, ScanErrorKind, Scanner,
    scan_all,
};
pub use source::{ChunkSource, FileSource, SliceSource, Source, Span, StringSource, line_of};
