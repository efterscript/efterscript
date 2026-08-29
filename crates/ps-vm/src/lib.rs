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

pub mod memory;
pub mod names;
pub mod object;

pub use memory::{Arena, Dict, GState, Memory, PersistentMap, Shared, Slot};
pub use names::{Atom, MAX_NAME_LEN, NameTable, NameTooLong};
pub use object::{Access, CompositeRef, Handle, Object, Space, Type};
