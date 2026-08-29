// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! VM memory: local and global arenas over a persistent slot map.
//!
//! Sharing happens at two levels. The slot table is a persistent trie, so a
//! snapshot is a clone of its root; slot contents are reference counted and
//! mutated through [`Shared::make_mut`], so a write after a snapshot copies
//! only the storage being written.

use std::fmt;
use std::rc::Rc;

use crate::dict::Dict;
use crate::error::VmError;
use crate::files::{FileCapability, FileTable, Stream};
use crate::names::{Atom, NameTable, NameTooLong};
use crate::object::{Access, Handle, Object, Space, Type};

/// Reference-counted storage for slot contents. An alias so snapshots can
/// move to `Arc` if they ever cross threads.
pub type Shared<T> = Rc<T>;

/// Placeholder until the graphics layer decides where gstates live.
#[derive(Clone, Debug, Default)]
pub struct GState {}

#[derive(Clone, Debug)]
pub enum Slot {
    Array(Shared<Vec<Object>>),
    String(Shared<Vec<u8>>),
    Dict(Shared<Dict>),
    GState(Shared<GState>),
}

// ---------------------------------------------------------------------------
// Persistent map

const BITS: u32 = 5;
const WIDTH: usize = 1 << BITS;
const MASK: u32 = WIDTH as u32 - 1;

// A node at shift 0 is always a leaf and any other node a branch, so the
// depth is uniform and a lookup needs no per-node tag check. Nodes only ever
// live behind an `Rc`, so the enum's size is the allocation's size and boxing
// the larger variant would only add a pointer chase.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Node<V> {
    Branch([Option<Rc<Node<V>>>; WIDTH]),
    Leaf([Option<V>; WIDTH]),
}

impl<V> Node<V> {
    fn empty(shift: u32) -> Self {
        if shift == 0 {
            Node::Leaf(std::array::from_fn(|_| None))
        } else {
            Node::Branch(std::array::from_fn(|_| None))
        }
    }
}

/// Array-mapped trie keyed by `u32`. Cloning is O(1); insertion path-copies
/// only the nodes shared with another clone, so a map with no live clones
/// mutates in place.
#[derive(Clone, Debug)]
pub struct PersistentMap<V> {
    root: Option<Rc<Node<V>>>,
    shift: u32,
    len: usize,
}

impl<V> Default for PersistentMap<V> {
    fn default() -> Self {
        PersistentMap {
            root: None,
            shift: 0,
            len: 0,
        }
    }
}

impl<V: Clone> PersistentMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn capacity(&self) -> u64 {
        1u64 << (self.shift + BITS)
    }

    pub fn get(&self, key: u32) -> Option<&V> {
        if u64::from(key) >= self.capacity() {
            return None;
        }
        let mut node = self.root.as_ref()?;
        let mut shift = self.shift;
        loop {
            match &**node {
                Node::Branch(children) => {
                    node = children[((key >> shift) & MASK) as usize].as_ref()?;
                    shift -= BITS;
                }
                Node::Leaf(values) => return values[(key & MASK) as usize].as_ref(),
            }
        }
    }

    pub fn contains_key(&self, key: u32) -> bool {
        self.get(key).is_some()
    }

    /// Mutable access to an existing entry, path-copying nodes shared with
    /// other clones. `None` leaves the map untouched.
    pub fn get_mut(&mut self, key: u32) -> Option<&mut V> {
        if !self.contains_key(key) {
            return None;
        }
        let mut node = self.root.as_mut()?;
        let mut shift = self.shift;
        loop {
            match Rc::make_mut(node) {
                Node::Branch(children) => {
                    node = children[((key >> shift) & MASK) as usize].as_mut()?;
                    shift -= BITS;
                }
                Node::Leaf(values) => return values[(key & MASK) as usize].as_mut(),
            }
        }
    }

    /// Inserts or replaces; returns the previous value.
    pub fn insert(&mut self, key: u32, value: V) -> Option<V> {
        if self.root.is_none() {
            self.root = Some(Rc::new(Node::empty(0)));
            self.shift = 0;
        }
        while u64::from(key) >= self.capacity() {
            let mut parent = Node::empty(self.shift + BITS);
            if let Node::Branch(children) = &mut parent {
                children[0] = self.root.take();
            }
            self.root = Some(Rc::new(parent));
            self.shift += BITS;
        }
        let mut node = self.root.as_mut().expect("root exists");
        let mut shift = self.shift;
        loop {
            match Rc::make_mut(node) {
                Node::Branch(children) => {
                    let child = &mut children[((key >> shift) & MASK) as usize];
                    shift -= BITS;
                    node = child.get_or_insert_with(|| Rc::new(Node::empty(shift)));
                }
                Node::Leaf(values) => {
                    let previous = values[(key & MASK) as usize].replace(value);
                    if previous.is_none() {
                        self.len += 1;
                    }
                    return previous;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Arena

/// One VM's slot storage. Cloning an arena is a snapshot: O(1), sharing every
/// slot with the original until either side writes.
#[derive(Clone, Debug)]
pub struct Arena {
    space: Space,
    slots: PersistentMap<Slot>,
    next: Handle,
}

impl Arena {
    pub fn new(space: Space) -> Self {
        Arena {
            space,
            slots: PersistentMap::new(),
            next: Handle(0),
        }
    }

    pub fn space(&self) -> Space {
        self.space
    }

    /// The handle the next allocation will receive. Handles below it have
    /// been issued (though a snapshot taken earlier may not know them).
    pub fn next_handle(&self) -> Handle {
        self.next
    }

    /// Number of slots reachable from this arena's current root.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn alloc(&mut self, slot: Slot) -> Handle {
        let handle = self.next;
        self.next = Handle(handle.0.checked_add(1).expect("handle space exhausted"));
        self.slots.insert(handle.0, slot);
        handle
    }

    fn length_of(len: usize) -> u32 {
        u32::try_from(len).expect("composite longer than u32::MAX")
    }

    pub fn alloc_array(&mut self, items: Vec<Object>) -> Object {
        let length = Self::length_of(items.len());
        let handle = self.alloc(Slot::Array(Shared::new(items)));
        Object::array(self.space, handle, length)
    }

    pub fn alloc_packed_array(&mut self, items: Vec<Object>) -> Object {
        let length = Self::length_of(items.len());
        let handle = self.alloc(Slot::Array(Shared::new(items)));
        Object::packed_array(self.space, handle, length)
    }

    pub fn alloc_string(&mut self, bytes: Vec<u8>) -> Object {
        let length = Self::length_of(bytes.len());
        let handle = self.alloc(Slot::String(Shared::new(bytes)));
        Object::string(self.space, handle, length)
    }

    pub fn alloc_dict(&mut self, dict: Dict) -> Object {
        let handle = self.alloc(Slot::Dict(Shared::new(dict)));
        Object::dict(self.space, handle)
    }

    pub fn alloc_gstate(&mut self, gstate: GState) -> Object {
        let handle = self.alloc(Slot::GState(Shared::new(gstate)));
        Object::gstate(self.space, handle)
    }

    pub fn slot(&self, handle: Handle) -> Option<&Slot> {
        self.slots.get(handle.0)
    }

    /// Mutable access to a slot. The slot map is path-copied if a snapshot
    /// shares it; the slot's contents are copied by the caller's `make_mut`.
    pub fn slot_mut(&mut self, handle: Handle) -> Option<&mut Slot> {
        self.slots.get_mut(handle.0)
    }

    pub fn array(&self, handle: Handle) -> Option<&[Object]> {
        match self.slot(handle)? {
            Slot::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn array_mut(&mut self, handle: Handle) -> Option<&mut Vec<Object>> {
        match self.slot_mut(handle)? {
            Slot::Array(items) => Some(Shared::make_mut(items)),
            _ => None,
        }
    }

    pub fn string(&self, handle: Handle) -> Option<&[u8]> {
        match self.slot(handle)? {
            Slot::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn string_mut(&mut self, handle: Handle) -> Option<&mut Vec<u8>> {
        match self.slot_mut(handle)? {
            Slot::String(bytes) => Some(Shared::make_mut(bytes)),
            _ => None,
        }
    }

    pub fn dict(&self, handle: Handle) -> Option<&Dict> {
        match self.slot(handle)? {
            Slot::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    pub fn dict_mut(&mut self, handle: Handle) -> Option<&mut Dict> {
        match self.slot_mut(handle)? {
            Slot::Dict(dict) => Some(Shared::make_mut(dict)),
            _ => None,
        }
    }

    pub fn gstate(&self, handle: Handle) -> Option<&GState> {
        match self.slot(handle)? {
            Slot::GState(gstate) => Some(gstate),
            _ => None,
        }
    }

    pub fn gstate_mut(&mut self, handle: Handle) -> Option<&mut GState> {
        match self.slot_mut(handle)? {
            Slot::GState(gstate) => Some(Shared::make_mut(gstate)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Save records

/// Deepest `save` nesting accepted, per PLRM3 Appendix B.
pub const MAX_SAVE_DEPTH: usize = 15;

/// What a `save` captured: the local arena (an O(1) snapshot whose
/// `next_handle` is the watermark), the graphics-state stack depth for the
/// caller to pop back to, and the file table watermark.
#[derive(Clone, Debug)]
pub struct SaveRecord {
    serial: u32,
    local: Arena,
    gstate_depth: usize,
    file_watermark: u32,
}

impl SaveRecord {
    /// Local handles at or above this were issued after the save.
    pub fn watermark(&self) -> Handle {
        self.local.next_handle()
    }

    pub fn gstate_depth(&self) -> usize {
        self.gstate_depth
    }

    /// File handles at or above this were opened after the save.
    pub fn file_watermark(&self) -> u32 {
        self.file_watermark
    }

    /// Whether `object` is a local composite created after this save.
    pub fn outlived_by(&self, object: Object) -> bool {
        let Some(r) = object.composite_ref() else {
            return false;
        };
        if r.space != Space::Local {
            return false;
        }
        if object.ty() == Type::File {
            r.handle.0 >= self.file_watermark
        } else {
            r.handle >= self.watermark()
        }
    }
}

/// The global/local rule (PLRM3 §3.7.2): a global composite may not come to
/// contain a local composite. Every storing path calls this before writing.
pub fn check_store(container: Object, value: Object) -> Result<(), VmError> {
    match (container.space(), value.space()) {
        (Some(Space::Global), Some(Space::Local)) => Err(VmError::InvalidAccess),
        _ => Ok(()),
    }
}

fn check_store_into(space: Space, values: &[Object]) -> Result<(), VmError> {
    if space == Space::Global && values.iter().any(|v| v.space() == Some(Space::Local)) {
        Err(VmError::InvalidAccess)
    } else {
        Ok(())
    }
}

// Access needed for the operation, compared against what the object grants.
fn require(granted: Access, needed: Access) -> Result<(), VmError> {
    if granted <= needed {
        Ok(())
    } else {
        Err(VmError::InvalidAccess)
    }
}

fn index_in(index: usize, length: usize) -> Result<usize, VmError> {
    if index < length {
        Ok(index)
    } else {
        Err(VmError::RangeCheck)
    }
}

fn span_in(index: usize, count: usize, length: usize) -> Result<std::ops::Range<usize>, VmError> {
    match index.checked_add(count) {
        Some(end) if end <= length => Ok(index..end),
        _ => Err(VmError::RangeCheck),
    }
}

// ---------------------------------------------------------------------------
// Memory

/// Local and global VM, the name table, the file table, and the save stack.
/// Allocation goes to the arena selected by `setglobal`.
///
/// The `*_get`, `*_put`, and `*_put_interval` methods are the storing and
/// reading paths operators use: they check access attributes, ranges, and
/// the global/local rule, and report PostScript errors. The plain `array`,
/// `string`, and `dict` accessors are raw storage views.
pub struct Memory {
    local: Arena,
    global: Arena,
    names: NameTable,
    files: FileTable,
    file_capability: Option<Box<dyn FileCapability>>,
    saves: Vec<SaveRecord>,
    next_save_serial: u32,
    allocate_global: bool,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("local", &self.local)
            .field("global", &self.global)
            .field("names", &self.names.len())
            .field("files", &self.files)
            .field("file_capability", &self.file_capability.is_some())
            .field("saves", &self.saves.len())
            .field("allocate_global", &self.allocate_global)
            .finish()
    }
}

impl Memory {
    pub fn new() -> Self {
        Memory {
            local: Arena::new(Space::Local),
            global: Arena::new(Space::Global),
            names: NameTable::new(),
            files: FileTable::new(),
            file_capability: None,
            saves: Vec::new(),
            next_save_serial: 0,
            allocate_global: false,
        }
    }

    /// Installs the embedder's answer to `file` requests. Without one, every
    /// request raises `undefinedfilename`.
    pub fn set_file_capability(&mut self, capability: Option<Box<dyn FileCapability>>) {
        self.file_capability = capability;
    }

    // --- save / restore ----------------------------------------------------

    /// `save`. `gstate_depth` is the graphics-state stack depth to return to
    /// on `restore`.
    pub fn save(&mut self, gstate_depth: usize) -> Result<Object, VmError> {
        if self.saves.len() >= MAX_SAVE_DEPTH {
            return Err(VmError::LimitCheck);
        }
        let serial = self.next_save_serial;
        self.next_save_serial = serial.checked_add(1).ok_or(VmError::LimitCheck)?;
        self.saves.push(SaveRecord {
            serial,
            local: self.local.clone(),
            gstate_depth,
            file_watermark: self.files.watermark(),
        });
        Ok(Object::save(serial))
    }

    pub fn save_depth(&self) -> usize {
        self.saves.len()
    }

    /// The live save record `object` names, innermost last.
    fn save_position(&self, object: Object) -> Result<usize, VmError> {
        let serial = object.as_save().ok_or(VmError::TypeCheck)?;
        self.saves
            .iter()
            .position(|r| r.serial == serial)
            .ok_or(VmError::InvalidRestore)
    }

    /// Whether `object` is a save object that `restore` would accept.
    pub fn save_is_live(&self, object: Object) -> bool {
        self.save_position(object).is_ok()
    }

    pub fn save_record(&self, object: Object) -> Option<&SaveRecord> {
        self.save_position(object).ok().map(|i| &self.saves[i])
    }

    /// `restore`. `stacks` are the operand, dictionary, and execution stacks
    /// (or whatever the caller holds objects in); any local composite on
    /// them created after the save is `invalidrestore` and nothing changes.
    /// Saves nested inside the one restored are discarded. Returns the
    /// graphics-state depth recorded by the matching `save`.
    pub fn restore(&mut self, save: Object, stacks: &[&[Object]]) -> Result<usize, VmError> {
        let position = self.save_position(save)?;
        let record = &self.saves[position];
        if stacks
            .iter()
            .flat_map(|s| s.iter())
            .any(|&o| record.outlived_by(o))
        {
            return Err(VmError::InvalidRestore);
        }
        self.saves.truncate(position + 1);
        let record = self.saves.pop().expect("record at position");
        let next = self.local.next;
        self.local = record.local;
        self.local.next = next;
        self.files.close_from(record.file_watermark)?;
        Ok(record.gstate_depth)
    }

    // --- allocation --------------------------------------------------------

    /// `setglobal`
    pub fn set_global(&mut self, global: bool) {
        self.allocate_global = global;
    }

    /// `currentglobal`
    pub fn current_global(&self) -> bool {
        self.allocate_global
    }

    pub fn current_space(&self) -> Space {
        if self.allocate_global {
            Space::Global
        } else {
            Space::Local
        }
    }

    pub fn arena(&self, space: Space) -> &Arena {
        match space {
            Space::Local => &self.local,
            Space::Global => &self.global,
        }
    }

    pub fn arena_mut(&mut self, space: Space) -> &mut Arena {
        match space {
            Space::Local => &mut self.local,
            Space::Global => &mut self.global,
        }
    }

    pub fn current_arena(&mut self) -> &mut Arena {
        self.arena_mut(self.current_space())
    }

    pub fn names(&self) -> &NameTable {
        &self.names
    }

    pub fn names_mut(&mut self) -> &mut NameTable {
        &mut self.names
    }

    /// A literal name object for `text`.
    pub fn intern(&mut self, text: &[u8]) -> Result<Object, NameTooLong> {
        self.names.intern(text).map(Object::name)
    }

    pub fn name_text(&self, atom: Atom) -> &[u8] {
        self.names.text(atom)
    }

    /// An array holding `items`, subject to the global/local rule: the
    /// scanner builds procedure bodies this way.
    pub fn alloc_array(&mut self, items: Vec<Object>) -> Result<Object, VmError> {
        check_store_into(self.current_space(), &items)?;
        Ok(self.current_arena().alloc_array(items))
    }

    pub fn alloc_packed_array(&mut self, items: Vec<Object>) -> Result<Object, VmError> {
        check_store_into(self.current_space(), &items)?;
        Ok(self.current_arena().alloc_packed_array(items))
    }

    pub fn alloc_string(&mut self, bytes: Vec<u8>) -> Object {
        self.current_arena().alloc_string(bytes)
    }

    pub fn alloc_dict(&mut self, dict: Dict) -> Object {
        self.current_arena().alloc_dict(dict)
    }

    /// `dict`: an empty dictionary reporting `maxlength`.
    pub fn new_dict(&mut self, maxlength: u32) -> Object {
        self.alloc_dict(Dict::new(maxlength))
    }

    pub fn alloc_gstate(&mut self, gstate: GState) -> Object {
        self.current_arena().alloc_gstate(gstate)
    }

    // --- files -------------------------------------------------------------

    /// A file object for a stream the embedder issued directly. The object
    /// belongs to the current allocation space.
    pub fn open_stream(&mut self, stream: Box<dyn Stream>) -> Object {
        let handle = self.files.open(stream);
        Object::file(self.current_space(), handle)
    }

    /// `file`: asks the installed capability for a stream. With none
    /// installed the request fails with `undefinedfilename` without any
    /// host resource being consulted.
    pub fn open_file(&mut self, name: &[u8], mode: &[u8]) -> Result<Object, VmError> {
        let capability = self
            .file_capability
            .as_mut()
            .ok_or(VmError::UndefinedFileName)?;
        let stream = capability.open(name, mode)?;
        Ok(self.open_stream(stream))
    }

    fn file_handle(object: Object, needed: Access) -> Result<Handle, VmError> {
        if object.ty() != Type::File {
            return Err(VmError::TypeCheck);
        }
        require(object.access().unwrap_or_default(), needed)?;
        Ok(object.handle().expect("file is composite"))
    }

    pub fn file_is_open(&self, object: Object) -> bool {
        object.ty() == Type::File && self.files.is_open(object.handle().expect("composite"))
    }

    pub fn file_read(&mut self, object: Object, buf: &mut [u8]) -> Result<usize, VmError> {
        let handle = Self::file_handle(object, Access::ReadOnly)?;
        self.files.read(handle, buf)
    }

    pub fn file_write(&mut self, object: Object, buf: &[u8]) -> Result<usize, VmError> {
        let handle = Self::file_handle(object, Access::Unlimited)?;
        self.files.write(handle, buf)
    }

    /// `closefile`
    pub fn close_file(&mut self, object: Object) -> Result<(), VmError> {
        let handle = Self::file_handle(object, Access::None)?;
        self.files.close(handle)
    }

    pub fn files(&self) -> &FileTable {
        &self.files
    }

    /// Raw access to the file table; the scanner's `FileSource` reads
    /// through it so its cursor is the file's.
    pub fn files_mut(&mut self) -> &mut FileTable {
        &mut self.files
    }

    fn interval(object: Object, ty: Type) -> Option<(Space, Handle, std::ops::Range<usize>)> {
        if object.ty() != ty && !(ty == Type::Array && object.ty() == Type::PackedArray) {
            return None;
        }
        let r = object.composite_ref()?;
        let start = r.offset as usize;
        Some((r.space, r.handle, start..start + r.length as usize))
    }

    /// The elements an array or packed array object addresses. `None` if the
    /// object is not an array or its handle resolves to nothing.
    pub fn array(&self, object: Object) -> Option<&[Object]> {
        let (space, handle, range) = Self::interval(object, Type::Array)?;
        self.arena(space).array(handle)?.get(range)
    }

    /// Mutable view of an array's elements. Access attributes are the
    /// operators' concern; this is raw storage.
    pub fn array_mut(&mut self, object: Object) -> Option<&mut [Object]> {
        let (space, handle, range) = Self::interval(object, Type::Array)?;
        self.arena_mut(space).array_mut(handle)?.get_mut(range)
    }

    pub fn string(&self, object: Object) -> Option<&[u8]> {
        let (space, handle, range) = Self::interval(object, Type::String)?;
        self.arena(space).string(handle)?.get(range)
    }

    pub fn string_mut(&mut self, object: Object) -> Option<&mut [u8]> {
        let (space, handle, range) = Self::interval(object, Type::String)?;
        self.arena_mut(space).string_mut(handle)?.get_mut(range)
    }

    pub fn dict(&self, object: Object) -> Option<&Dict> {
        let (space, handle, _) = Self::interval(object, Type::Dict)?;
        self.arena(space).dict(handle)
    }

    pub fn dict_mut(&mut self, object: Object) -> Option<&mut Dict> {
        let (space, handle, _) = Self::interval(object, Type::Dict)?;
        self.arena_mut(space).dict_mut(handle)
    }

    // --- checked access: arrays -------------------------------------------

    fn resolve<T>(object: Object, ty: Type, view: Option<T>) -> Result<T, VmError> {
        if object.ty() != ty && !(ty == Type::Array && object.ty() == Type::PackedArray) {
            return Err(VmError::TypeCheck);
        }
        // A handle that resolves to nothing was discarded by `restore`.
        view.ok_or(VmError::InvalidAccess)
    }

    fn readable_array(&self, object: Object) -> Result<&[Object], VmError> {
        let items = Self::resolve(object, Type::Array, self.array(object))?;
        require(object.access().unwrap_or_default(), Access::ReadOnly)?;
        Ok(items)
    }

    fn writable_array(&mut self, object: Object) -> Result<&mut [Object], VmError> {
        require(object.access().unwrap_or_default(), Access::Unlimited)?;
        Self::resolve(object, Type::Array, self.array_mut(object))
    }

    /// `get` on an array.
    pub fn array_get(&self, array: Object, index: usize) -> Result<Object, VmError> {
        let items = self.readable_array(array)?;
        Ok(items[index_in(index, items.len())?])
    }

    /// `put` on an array.
    pub fn array_put(&mut self, array: Object, index: usize, value: Object) -> Result<(), VmError> {
        require(array.access().unwrap_or_default(), Access::Unlimited)?;
        let length = Self::resolve(array, Type::Array, self.array(array))?.len();
        let index = index_in(index, length)?;
        check_store(array, value)?;
        self.writable_array(array)?[index] = value;
        Ok(())
    }

    /// `putinterval` with an array source. Source and destination may
    /// overlap.
    pub fn array_put_interval(
        &mut self,
        array: Object,
        index: usize,
        source: Object,
    ) -> Result<(), VmError> {
        let items = self.readable_array(source)?.to_vec();
        self.array_put_items(array, index, &items)
    }

    /// `putinterval` with elements the caller already holds.
    pub fn array_put_items(
        &mut self,
        array: Object,
        index: usize,
        items: &[Object],
    ) -> Result<(), VmError> {
        require(array.access().unwrap_or_default(), Access::Unlimited)?;
        let length = Self::resolve(array, Type::Array, self.array(array))?.len();
        let range = span_in(index, items.len(), length)?;
        check_store_into(array.space().expect("array is composite"), items)?;
        self.writable_array(array)?[range].copy_from_slice(items);
        Ok(())
    }

    /// `copy` with an array source: fills the front of `array` and returns
    /// the sub-array holding the copy.
    pub fn array_copy(&mut self, source: Object, array: Object) -> Result<Object, VmError> {
        self.array_put_interval(array, 0, source)?;
        Ok(array
            .with_interval(0, source.length().expect("array is composite"))
            .expect("length checked by put_interval"))
    }

    // --- checked access: strings ------------------------------------------

    fn readable_string(&self, object: Object) -> Result<&[u8], VmError> {
        let bytes = Self::resolve(object, Type::String, self.string(object))?;
        require(object.access().unwrap_or_default(), Access::ReadOnly)?;
        Ok(bytes)
    }

    fn writable_string(&mut self, object: Object) -> Result<&mut [u8], VmError> {
        require(object.access().unwrap_or_default(), Access::Unlimited)?;
        Self::resolve(object, Type::String, self.string_mut(object))
    }

    /// `get` on a string.
    pub fn string_get(&self, string: Object, index: usize) -> Result<u8, VmError> {
        let bytes = self.readable_string(string)?;
        Ok(bytes[index_in(index, bytes.len())?])
    }

    /// `put` on a string.
    pub fn string_put(&mut self, string: Object, index: usize, byte: u8) -> Result<(), VmError> {
        let bytes = self.writable_string(string)?;
        let index = index_in(index, bytes.len())?;
        bytes[index] = byte;
        Ok(())
    }

    /// `putinterval` with a string source. Source and destination may
    /// overlap.
    pub fn string_put_interval(
        &mut self,
        string: Object,
        index: usize,
        source: Object,
    ) -> Result<(), VmError> {
        let bytes = self.readable_string(source)?.to_vec();
        self.string_put_bytes(string, index, &bytes)
    }

    /// `putinterval` with bytes the caller already holds.
    pub fn string_put_bytes(
        &mut self,
        string: Object,
        index: usize,
        bytes: &[u8],
    ) -> Result<(), VmError> {
        let target = self.writable_string(string)?;
        let range = span_in(index, bytes.len(), target.len())?;
        target[range].copy_from_slice(bytes);
        Ok(())
    }

    /// `copy` with a string source.
    pub fn string_copy(&mut self, source: Object, string: Object) -> Result<Object, VmError> {
        self.string_put_interval(string, 0, source)?;
        Ok(string
            .with_interval(0, source.length().expect("string is composite"))
            .expect("length checked by put_interval"))
    }

    // --- checked access: dictionaries -------------------------------------

    /// The key as stored: strings become names (PLRM3 §3.3.9), which needs
    /// read access to the string.
    pub fn dict_key(&mut self, key: Object) -> Result<Object, VmError> {
        if key.ty() != Type::String {
            return Ok(key);
        }
        let text = self.readable_string(key)?.to_vec();
        Ok(self.intern(&text)?)
    }

    fn readable_dict(&self, object: Object, needed: Access) -> Result<&Dict, VmError> {
        let dict = Self::resolve(object, Type::Dict, self.dict(object))?;
        require(dict.access(), needed)?;
        Ok(dict)
    }

    /// `get` on a dictionary; `None` if the key is absent (`undefined` is
    /// the operator's decision, since `load` and `get` report it while
    /// `known` does not).
    pub fn dict_get(&mut self, dict: Object, key: Object) -> Result<Option<Object>, VmError> {
        let key = self.dict_key(key)?;
        Ok(self.readable_dict(dict, Access::ReadOnly)?.get(key))
    }

    /// `known`
    pub fn dict_known(&mut self, dict: Object, key: Object) -> Result<bool, VmError> {
        self.dict_get(dict, key).map(|v| v.is_some())
    }

    /// `put` on a dictionary, and the storing half of `def`.
    pub fn dict_put(&mut self, dict: Object, key: Object, value: Object) -> Result<(), VmError> {
        let key = self.dict_key(key)?;
        self.readable_dict(dict, Access::Unlimited)?;
        check_store(dict, key)?;
        check_store(dict, value)?;
        self.dict_mut(dict)
            .expect("resolved above")
            .insert(key, value);
        Ok(())
    }

    /// `undef`; removing an absent key is not an error.
    pub fn dict_undef(&mut self, dict: Object, key: Object) -> Result<(), VmError> {
        let key = self.dict_key(key)?;
        self.readable_dict(dict, Access::Unlimited)?;
        self.dict_mut(dict).expect("resolved above").remove(key);
        Ok(())
    }

    /// `copy` with a dictionary source: every entry goes through the
    /// storing check. Returns the destination.
    pub fn dict_copy(&mut self, source: Object, dict: Object) -> Result<Object, VmError> {
        let entries: Vec<_> = self
            .readable_dict(source, Access::ReadOnly)?
            .iter()
            .collect();
        self.readable_dict(dict, Access::Unlimited)?;
        for &(key, value) in &entries {
            check_store(dict, key)?;
            check_store(dict, value)?;
        }
        let target = self.dict_mut(dict).expect("resolved above");
        for (key, value) in entries {
            target.insert(key, value);
        }
        Ok(dict)
    }

    /// `length` of a dictionary.
    pub fn dict_len(&self, dict: Object) -> Result<usize, VmError> {
        Ok(self.readable_dict(dict, Access::ReadOnly)?.len())
    }

    /// `maxlength`
    pub fn dict_maxlength(&self, dict: Object) -> Result<u32, VmError> {
        Ok(self.readable_dict(dict, Access::ReadOnly)?.maxlength())
    }

    /// Entries in insertion order, for `forall`.
    pub fn dict_entries(&self, dict: Object) -> Result<Vec<(Object, Object)>, VmError> {
        Ok(self.readable_dict(dict, Access::ReadOnly)?.iter().collect())
    }

    /// The access attribute, shared by every reference to the dictionary.
    pub fn dict_access(&self, dict: Object) -> Result<Access, VmError> {
        Ok(Self::resolve(dict, Type::Dict, self.dict(dict))?.access())
    }

    /// `readonly`, `executeonly`, and `noaccess` on a dictionary. Access
    /// only ever tightens.
    pub fn dict_set_access(&mut self, dict: Object, access: Access) -> Result<(), VmError> {
        let current = self.dict_access(dict)?;
        if access < current {
            return Err(VmError::InvalidAccess);
        }
        self.dict_mut(dict)
            .expect("resolved above")
            .set_access(access);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Probe;
    use proptest::prelude::*;

    fn ints(n: usize) -> Vec<Object> {
        (0..n).map(|i| Object::integer(i as i32)).collect()
    }

    fn values(m: &Memory, a: Object) -> Vec<i32> {
        m.array(a)
            .unwrap()
            .iter()
            .map(|o| o.as_i32().unwrap())
            .collect()
    }

    // --- PersistentMap -----------------------------------------------------

    #[test]
    fn map_insert_get_replace() {
        let mut m = PersistentMap::new();
        assert!(m.is_empty());
        assert_eq!(m.get(0), None);
        assert_eq!(m.insert(0, "a"), None);
        assert_eq!(m.insert(1, "b"), None);
        assert_eq!(m.get(0), Some(&"a"));
        assert_eq!(m.get(1), Some(&"b"));
        assert_eq!(m.get(2), None);
        assert_eq!(m.insert(0, "c"), Some("a"));
        assert_eq!(m.get(0), Some(&"c"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn map_grows_to_sparse_and_extreme_keys() {
        let mut m = PersistentMap::new();
        for key in [0, 31, 32, 1023, 1024, 1 << 20, u32::MAX - 1, u32::MAX] {
            m.insert(key, key);
        }
        for key in [0, 31, 32, 1023, 1024, 1 << 20, u32::MAX - 1, u32::MAX] {
            assert_eq!(m.get(key), Some(&key));
        }
        for key in [1, 33, 1025, (1 << 20) + 1, 1 << 31] {
            assert_eq!(m.get(key), None);
        }
        assert_eq!(m.len(), 8);
    }

    #[test]
    fn map_get_mut_only_touches_existing_entries() {
        let mut m = PersistentMap::new();
        m.insert(5, 1);
        let snapshot = m.clone();
        assert_eq!(m.get_mut(6), None);
        assert!(Rc::ptr_eq(
            m.root.as_ref().unwrap(),
            snapshot.root.as_ref().unwrap()
        ));
        *m.get_mut(5).unwrap() = 2;
        assert_eq!(m.get(5), Some(&2));
        assert_eq!(snapshot.get(5), Some(&1));
    }

    #[test]
    fn map_clone_is_independent() {
        let mut live = PersistentMap::new();
        for k in 0..100 {
            live.insert(k, k);
        }
        let snap = live.clone();
        for k in 100..200 {
            live.insert(k, k);
        }
        live.insert(0, 999);
        assert_eq!(snap.len(), 100);
        assert_eq!(snap.get(0), Some(&0));
        assert_eq!(snap.get(150), None);
        assert_eq!(live.len(), 200);
        assert_eq!(live.get(0), Some(&999));
        assert_eq!(live.get(150), Some(&150));
        drop(snap);
        assert_eq!(live.get(99), Some(&99));
    }

    #[test]
    fn map_dropping_the_live_side_keeps_the_snapshot() {
        let mut live = PersistentMap::new();
        for k in 0..2000 {
            live.insert(k, k);
        }
        let snap = live.clone();
        for k in 0..2000 {
            live.insert(k, k + 1);
        }
        drop(live);
        for k in 0..2000 {
            assert_eq!(snap.get(k), Some(&k));
        }
    }

    // --- Arena -------------------------------------------------------------

    #[test]
    fn allocation_sets_space_and_length() {
        let mut local = Arena::new(Space::Local);
        let mut global = Arena::new(Space::Global);
        let a = local.alloc_array(ints(3));
        let s = global.alloc_string(b"abc".to_vec());
        assert_eq!(a.space(), Some(Space::Local));
        assert_eq!(a.length(), Some(3));
        assert_eq!(a.handle(), Some(Handle(0)));
        assert_eq!(s.space(), Some(Space::Global));
        assert_eq!(s.length(), Some(3));
        assert_eq!(s.handle(), Some(Handle(0)));
        assert_eq!(local.string(Handle(0)), None);
        assert!(global.array(Handle(0)).is_none());
        assert_eq!(local.array(Handle(0)).unwrap().len(), 3);
        assert_eq!(global.string(Handle(0)), Some(&b"abc"[..]));
        let p = local.alloc_packed_array(ints(2));
        assert_eq!(p.ty(), Type::PackedArray);
        assert_eq!(local.array(p.handle().unwrap()).unwrap().len(), 2);
        let d = local.alloc_dict(Dict::default());
        assert!(local.dict(d.handle().unwrap()).is_some());
        assert!(local.dict_mut(d.handle().unwrap()).is_some());
        let g = local.alloc_gstate(GState::default());
        assert!(local.gstate(g.handle().unwrap()).is_some());
        assert!(local.gstate_mut(g.handle().unwrap()).is_some());
        assert_eq!(local.slot_count(), 4);
    }

    #[test]
    fn snapshot_keeps_old_version_after_write() {
        let mut arena = Arena::new(Space::Local);
        let a = arena.alloc_array(ints(3));
        let h = a.handle().unwrap();
        let snapshot = arena.clone();
        arena.array_mut(h).unwrap()[0] = Object::integer(9);
        assert_eq!(arena.array(h).unwrap()[0].as_i32(), Some(9));
        assert_eq!(snapshot.array(h).unwrap()[0].as_i32(), Some(0));
        let b = arena.alloc_array(ints(1));
        assert!(snapshot.slot(b.handle().unwrap()).is_none());
        assert_eq!(snapshot.next_handle(), Handle(1));
        assert_eq!(arena.next_handle(), Handle(2));
    }

    #[test]
    fn writes_without_a_snapshot_happen_in_place() {
        let mut arena = Arena::new(Space::Local);
        let a = arena.alloc_array(ints(3));
        let h = a.handle().unwrap();
        let before = match arena.slot(h).unwrap() {
            Slot::Array(items) => Rc::as_ptr(items),
            _ => unreachable!(),
        };
        arena.array_mut(h).unwrap()[1] = Object::integer(5);
        let after = match arena.slot(h).unwrap() {
            Slot::Array(items) => Rc::as_ptr(items),
            _ => unreachable!(),
        };
        assert_eq!(before, after);
    }

    #[test]
    fn self_reference_creates_no_cycle() {
        let mut arena = Arena::new(Space::Local);
        let a = arena.alloc_array(vec![Object::null()]);
        let h = a.handle().unwrap();
        arena.array_mut(h).unwrap()[0] = a;
        let weak = match arena.slot(h).unwrap() {
            Slot::Array(items) => Rc::downgrade(items),
            _ => unreachable!(),
        };
        assert!(arena.array(h).unwrap()[0].eq(a));
        drop(arena);
        assert!(weak.upgrade().is_none());
    }

    // --- Memory ------------------------------------------------------------

    #[test]
    fn setglobal_selects_the_arena() {
        let mut m = Memory::new();
        assert!(!m.current_global());
        let l = m.alloc_array(ints(1)).unwrap();
        m.set_global(true);
        assert!(m.current_global());
        let g = m.alloc_string(b"g".to_vec());
        m.set_global(false);
        let l2 = m.alloc_dict(Dict::default());
        assert_eq!(l.space(), Some(Space::Local));
        assert_eq!(g.space(), Some(Space::Global));
        assert_eq!(l2.space(), Some(Space::Local));
        assert_eq!(l.handle(), Some(Handle(0)));
        assert_eq!(g.handle(), Some(Handle(0)));
        assert_eq!(l2.handle(), Some(Handle(1)));
        assert_eq!(m.arena(Space::Local).slot_count(), 2);
        assert_eq!(m.arena(Space::Global).slot_count(), 1);
        assert_eq!(m.string(g), Some(&b"g"[..]));
        assert!(m.dict(l2).is_some());
        assert!(m.dict_mut(l2).is_some());
    }

    #[test]
    fn copies_share_storage_and_intervals_alias() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(4)).unwrap();
        let b = a;
        m.array_mut(b).unwrap()[1] = Object::integer(42);
        assert_eq!(values(&m, a), [0, 42, 2, 3]);
        assert!(a.eq(b));

        let s = m.alloc_string(b"hello".to_vec());
        let t = s.with_interval(2, 3).unwrap();
        m.string_mut(t).unwrap()[0] = b'A';
        assert_eq!(m.string(s), Some(&b"heAlo"[..]));
        assert_eq!(m.string(t), Some(&b"Alo"[..]));
        assert!(!s.eq(t));
        assert_eq!(m.string(s.with_interval(5, 0).unwrap()), Some(&b""[..]));
    }

    #[test]
    fn lookups_are_typed_and_total() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(2)).unwrap();
        let s = m.alloc_string(b"xy".to_vec());
        assert!(m.string(a).is_none());
        assert!(m.array(s).is_none());
        assert!(m.dict(a).is_none());
        assert!(m.array(Object::integer(1)).is_none());
        assert!(m.array_mut(Object::null()).is_none());
        assert!(
            m.array(Object::array(Space::Local, Handle(50), 1))
                .is_none()
        );
        assert!(
            m.array(Object::array(Space::Global, Handle(0), 2))
                .is_none()
        );
        assert!(
            m.string(Object::string(Space::Local, Handle(1), 3))
                .is_none()
        );
        assert!(
            m.string_mut(Object::string(Space::Local, Handle(1), 3))
                .is_none()
        );
        let packed = m.alloc_packed_array(ints(1)).unwrap();
        assert!(m.array(packed).is_some());
        assert!(m.intern(b"n").unwrap().as_name().is_some());
        assert_eq!(m.name_text(m.names().lookup(b"n").unwrap()), b"n");
        assert_eq!(m.names_mut().intern(b"n").unwrap(), Atom(0));
    }

    // --- save / restore ----------------------------------------------------

    #[test]
    fn save_objects_are_distinct_and_live_until_restored() {
        let mut m = Memory::new();
        let s0 = m.save(0).unwrap();
        let s1 = m.save(3).unwrap();
        assert_eq!(s0.ty(), Type::Save);
        assert!(!s0.eq(s1));
        assert_eq!(m.save_depth(), 2);
        assert!(m.save_is_live(s0) && m.save_is_live(s1));
        assert_eq!(m.save_record(s1).unwrap().gstate_depth(), 3);
        assert_eq!(m.restore(s1, &[]), Ok(3));
        assert!(!m.save_is_live(s1));
        assert!(m.save_record(s1).is_none());
        assert_eq!(m.restore(s1, &[]), Err(VmError::InvalidRestore));
        let s2 = m.save(0).unwrap();
        assert!(!s1.eq(s2));
        assert!(!m.save_is_live(s1));
        assert_eq!(m.restore(s2, &[]), Ok(0));
        assert_eq!(m.restore(s0, &[]), Ok(0));
        assert_eq!(m.save_depth(), 0);
        assert_eq!(m.restore(Object::integer(1), &[]), Err(VmError::TypeCheck));
        assert_eq!(
            m.restore(Object::save(99), &[]),
            Err(VmError::InvalidRestore)
        );
    }

    #[test]
    fn save_nesting_is_limited() {
        let mut m = Memory::new();
        let saves: Vec<_> = (0..MAX_SAVE_DEPTH).map(|_| m.save(0).unwrap()).collect();
        assert_eq!(m.save(0).err(), Some(VmError::LimitCheck));
        assert_eq!(m.save_depth(), MAX_SAVE_DEPTH);
        m.restore(saves[0], &[]).unwrap();
        assert_eq!(m.save_depth(), 0);
        assert!(m.save(0).is_ok());
    }

    #[test]
    fn restore_reverts_local_and_keeps_global() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(3)).unwrap();
        m.set_global(true);
        let g = m.alloc_array(ints(1)).unwrap();
        m.set_global(false);
        let s = m.save(0).unwrap();
        m.array_put(a, 0, Object::integer(9)).unwrap();
        m.array_put(g, 0, Object::integer(2)).unwrap();
        let b = m.alloc_array(ints(2)).unwrap();
        m.set_global(true);
        let g2 = m.alloc_string(b"kept".to_vec());
        m.set_global(false);
        m.restore(s, &[]).unwrap();
        assert_eq!(values(&m, a), [0, 1, 2]);
        assert_eq!(values(&m, g), [2]);
        assert_eq!(m.string(g2), Some(&b"kept"[..]));
        assert!(m.array(b).is_none());
        assert_eq!(m.array_get(b, 0).err(), Some(VmError::InvalidAccess));
        assert_eq!(
            m.array_put(b, 0, Object::null()),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.arena(Space::Local).slot_count(), 1);
    }

    #[test]
    fn handles_never_go_backwards_across_restore() {
        let mut m = Memory::new();
        let s = m.save(0).unwrap();
        let b = m.alloc_array(ints(1)).unwrap();
        let next = m.arena(Space::Local).next_handle();
        m.restore(s, &[]).unwrap();
        assert_eq!(m.arena(Space::Local).next_handle(), next);
        let c = m.alloc_array(ints(1)).unwrap();
        assert!(c.handle() > b.handle());
        assert!(m.array(b).is_none());
    }

    #[test]
    fn outer_restore_discards_inner_saves() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(1)).unwrap();
        let outer = m.save(0).unwrap();
        m.array_put(a, 0, Object::integer(1)).unwrap();
        let inner = m.save(0).unwrap();
        m.array_put(a, 0, Object::integer(2)).unwrap();
        m.restore(outer, &[]).unwrap();
        assert_eq!(values(&m, a), [0]);
        assert!(!m.save_is_live(inner));
        assert_eq!(m.save_depth(), 0);
    }

    #[test]
    fn restore_rejects_newer_local_objects_on_the_stacks() {
        let mut m = Memory::new();
        let old = m.alloc_array(ints(1)).unwrap();
        let s = m.save(0).unwrap();
        let new = m.alloc_array(ints(2)).unwrap();
        m.set_global(true);
        let g = m.alloc_array(ints(1)).unwrap();
        m.set_global(false);
        let stack = [old, Object::integer(1), new];
        assert_eq!(m.restore(s, &[&stack]), Err(VmError::InvalidRestore));
        assert!(m.save_is_live(s));
        assert_eq!(values(&m, new), [0, 1]);
        assert_eq!(m.restore(s, &[&[old], &[g, s, Object::null()]]), Ok(0));
    }

    #[test]
    fn restore_rejects_files_opened_after_save() {
        let mut m = Memory::new();
        let old = m.open_stream(Box::new(Probe::default()));
        let s = m.save(0).unwrap();
        let new = m.open_stream(Box::new(Probe::default()));
        assert_eq!(m.restore(s, &[&[new]]), Err(VmError::InvalidRestore));
        assert_eq!(m.restore(s, &[&[old]]), Ok(0));
    }

    #[test]
    fn restore_closes_files_opened_since_the_save() {
        let mut m = Memory::new();
        let before = Probe::default();
        let f0 = m.open_stream(Box::new(before.clone()));
        let s = m.save(0).unwrap();
        let after = Probe::default();
        let f1 = m.open_stream(Box::new(after.clone()));
        assert!(m.file_is_open(f1));
        m.restore(s, &[]).unwrap();
        assert!(!before.closed() && m.file_is_open(f0));
        assert!(after.closed() && !m.file_is_open(f1));
        assert_eq!(m.file_read(f1, &mut [0; 1]), Err(VmError::IoError));
    }

    // --- the global/local rule ---------------------------------------------

    #[test]
    fn global_containers_reject_local_values() {
        let mut m = Memory::new();
        let la = m.alloc_array(ints(1)).unwrap();
        let ls = m.alloc_string(b"l".to_vec());
        let holder = m.alloc_array(vec![ls]).unwrap();
        m.set_global(true);
        let ga = m.alloc_array(vec![Object::null(); 2]).unwrap();
        let gd = m.new_dict(1);
        let global_str = m.alloc_string(b"gg".to_vec());
        let k = m.intern(b"k").unwrap();
        assert_eq!(m.array_put(ga, 0, la), Err(VmError::InvalidAccess));
        assert_eq!(
            m.array_put_items(ga, 0, &[Object::integer(1), ls]),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(
            m.array_put_interval(ga, 0, holder),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.dict_put(gd, k, la), Err(VmError::InvalidAccess));
        assert_eq!(m.dict_put(gd, la, k), Err(VmError::InvalidAccess));
        assert_eq!(m.alloc_array(vec![la]).err(), Some(VmError::InvalidAccess));
        assert_eq!(
            m.alloc_packed_array(vec![ls]).err(),
            Some(VmError::InvalidAccess)
        );
        assert!(m.array(ga).unwrap().iter().all(|o| o.ty() == Type::Null));
        assert_eq!(m.dict_len(gd), Ok(0));
        // Strings hold bytes, and `la` holds integers: no reference crosses.
        assert_eq!(m.string_put_interval(global_str, 0, ls), Ok(()));
        assert_eq!(m.array_put_interval(ga, 0, la), Ok(()));
        assert_eq!(m.array_put(ga, 0, global_str), Ok(()));
        assert_eq!(m.dict_put(gd, k, global_str), Ok(()));
    }

    #[test]
    fn check_store_only_rejects_local_into_global() {
        let l = Object::array(Space::Local, Handle(0), 0);
        let g = Object::array(Space::Global, Handle(0), 0);
        assert_eq!(check_store(g, l), Err(VmError::InvalidAccess));
        assert_eq!(check_store(l, g), Ok(()));
        assert_eq!(check_store(g, g), Ok(()));
        assert_eq!(check_store(l, l), Ok(()));
        assert_eq!(check_store(g, Object::integer(1)), Ok(()));
        assert_eq!(check_store(Object::integer(1), l), Ok(()));
    }

    #[test]
    fn local_containers_accept_anything() {
        let mut m = Memory::new();
        let la = m.alloc_array(vec![Object::null(); 2]).unwrap();
        let ld = m.new_dict(1);
        m.set_global(true);
        let ga = m.alloc_array(ints(1)).unwrap();
        m.set_global(false);
        let lb = m.alloc_array(ints(1)).unwrap();
        assert_eq!(m.array_put(la, 0, ga), Ok(()));
        assert_eq!(m.array_put(la, 1, lb), Ok(()));
        assert_eq!(m.dict_put(ld, ga, lb), Ok(()));
        assert_eq!(m.alloc_array(vec![ga, lb]).map(|_| ()), Ok(()));
        assert!(m.dict_get(ld, ga).unwrap().unwrap().eq(lb));
    }

    #[test]
    fn copy_routes_through_the_store_check() {
        let mut m = Memory::new();
        let la = m.alloc_array(ints(1)).unwrap();
        let src = m.alloc_array(vec![la]).unwrap();
        let ld = m.new_dict(1);
        let k = m.intern(b"k").unwrap();
        m.dict_put(ld, k, la).unwrap();
        m.set_global(true);
        let ga = m.alloc_array(vec![Object::null(); 2]).unwrap();
        let gd = m.new_dict(1);
        assert_eq!(m.array_copy(src, ga).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.dict_copy(ld, gd).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.dict_len(gd), Ok(0));
        let gsrc = m.alloc_array(ints(1)).unwrap();
        let copied = m.array_copy(gsrc, ga).unwrap();
        assert_eq!(copied.length(), Some(1));
        assert_eq!(values(&m, copied), [0]);
        assert_eq!(m.array_copy(ga, gsrc).err(), Some(VmError::RangeCheck));
        m.set_global(false);
        let ld2 = m.new_dict(0);
        assert!(m.dict_copy(ld, ld2).unwrap().eq(ld2));
        assert!(m.dict_get(ld2, k).unwrap().unwrap().eq(la));
    }

    // --- checked access ----------------------------------------------------

    #[test]
    fn array_access_is_per_object() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(2)).unwrap();
        let b = a.with_access(Access::ReadOnly).unwrap();
        assert_eq!(m.array_put(a, 0, Object::integer(1)), Ok(()));
        assert_eq!(
            m.array_put(b, 0, Object::integer(1)),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.array_get(b, 0).unwrap().as_i32(), Some(1));
        let x = a.with_access(Access::ExecuteOnly).unwrap();
        assert_eq!(m.array_get(x, 0).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.array_put_items(x, 0, &[]), Err(VmError::InvalidAccess));
        assert_eq!(m.array_put_interval(a, 0, x), Err(VmError::InvalidAccess));
        let p = m.alloc_packed_array(ints(1)).unwrap();
        assert_eq!(
            m.array_put(p, 0, Object::null()),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.array_get(p, 0).unwrap().as_i32(), Some(0));
        assert_eq!(m.array_put_interval(a, 1, p), Ok(()));
        assert_eq!(values(&m, a), [1, 0]);
    }

    #[test]
    fn array_errors_are_typed() {
        let mut m = Memory::new();
        let a = m.alloc_array(ints(2)).unwrap();
        let s = m.alloc_string(b"ab".to_vec());
        assert_eq!(m.array_get(a, 2).err(), Some(VmError::RangeCheck));
        assert_eq!(m.array_put(a, 2, Object::null()), Err(VmError::RangeCheck));
        assert_eq!(
            m.array_put_items(a, 1, &[Object::null(); 2]),
            Err(VmError::RangeCheck)
        );
        assert_eq!(
            m.array_put_items(a, usize::MAX, &[Object::null()]),
            Err(VmError::RangeCheck)
        );
        assert_eq!(m.array_put_items(a, 2, &[]), Ok(()));
        assert_eq!(m.array_get(s, 0).err(), Some(VmError::TypeCheck));
        assert_eq!(
            m.array_put(Object::integer(1), 0, Object::null()),
            Err(VmError::TypeCheck)
        );
        assert_eq!(m.array_put_interval(a, 0, s), Err(VmError::TypeCheck));
        let sub = a.with_interval(1, 1).unwrap();
        m.array_put(sub, 0, Object::integer(7)).unwrap();
        assert_eq!(values(&m, a), [0, 7]);
        assert_eq!(m.array_get(sub, 1).err(), Some(VmError::RangeCheck));
    }

    #[test]
    fn string_interval_writes_alias_through_sub_strings() {
        let mut m = Memory::new();
        let s = m.alloc_string(b"abcdef".to_vec());
        let t = s.with_interval(2, 3).unwrap();
        m.string_put(t, 0, b'A').unwrap();
        assert_eq!(m.string(s), Some(&b"abAdef"[..]));
        assert_eq!(m.string_get(s, 2), Ok(b'A'));
        assert_eq!(m.string_get(t, 3), Err(VmError::RangeCheck));
        assert_eq!(m.string_put(t, 3, b'x'), Err(VmError::RangeCheck));
        m.string_put_bytes(t, 1, b"XY").unwrap();
        assert_eq!(m.string(s), Some(&b"abAXYf"[..]));
        assert_eq!(m.string_put_bytes(t, 2, b"XY"), Err(VmError::RangeCheck));
        assert_eq!(
            m.string_put_bytes(t, usize::MAX, b"X"),
            Err(VmError::RangeCheck)
        );
        // Overlapping putinterval reads the source before writing.
        m.string_put_interval(s, 1, s.with_interval(0, 4).unwrap())
            .unwrap();
        assert_eq!(m.string(s), Some(&b"aabAXf"[..]));
        let src = m.alloc_string(b"zz".to_vec());
        let copied = m.string_copy(src, t).unwrap();
        assert_eq!(m.string(copied), Some(&b"zz"[..]));
        assert_eq!(m.string(s), Some(&b"aazzXf"[..]));
        assert_eq!(m.string_copy(s, src).err(), Some(VmError::RangeCheck));
        let ro = s.with_access(Access::ReadOnly).unwrap();
        assert_eq!(m.string_put(ro, 0, b'q'), Err(VmError::InvalidAccess));
        assert_eq!(m.string_put_bytes(ro, 0, b""), Err(VmError::InvalidAccess));
        assert_eq!(m.string_get(ro, 0), Ok(b'a'));
        let none = s.with_access(Access::None).unwrap();
        assert_eq!(m.string_get(none, 0), Err(VmError::InvalidAccess));
        assert_eq!(
            m.string_put_interval(s, 0, none),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.string_get(Object::integer(1), 0), Err(VmError::TypeCheck));
        assert_eq!(
            m.string_get(Object::string(Space::Local, Handle(77), 1), 0),
            Err(VmError::InvalidAccess)
        );
    }

    #[test]
    fn dict_access_is_shared_by_every_reference() {
        let mut m = Memory::new();
        let d = m.new_dict(2);
        let e = d;
        let k = m.intern(b"k").unwrap();
        m.dict_put(d, k, Object::integer(1)).unwrap();
        m.dict_set_access(e, Access::ReadOnly).unwrap();
        assert_eq!(m.dict_access(d), Ok(Access::ReadOnly));
        assert_eq!(
            m.dict_put(d, k, Object::integer(2)),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.dict_undef(d, k), Err(VmError::InvalidAccess));
        assert_eq!(m.dict_get(d, k).unwrap().unwrap().as_i32(), Some(1));
        assert_eq!(
            m.dict_set_access(d, Access::Unlimited),
            Err(VmError::InvalidAccess)
        );
        m.dict_set_access(d, Access::ExecuteOnly).unwrap();
        assert_eq!(m.dict_get(e, k).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.dict_len(e), Err(VmError::InvalidAccess));
        assert_eq!(m.dict_entries(e).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.dict_copy(e, d).err(), Some(VmError::InvalidAccess));
        assert_eq!(m.dict_access(e), Ok(Access::ExecuteOnly));
        assert_eq!(m.dict_access(k), Err(VmError::TypeCheck));
        assert_eq!(
            m.dict_access(Object::dict(Space::Local, Handle(9))),
            Err(VmError::InvalidAccess)
        );
        assert_eq!(m.dict_set_access(k, Access::None), Err(VmError::TypeCheck));
    }

    #[test]
    fn dict_string_keys_become_names_and_order_is_insertion() {
        let mut m = Memory::new();
        let d = m.new_dict(1);
        let b = m.intern(b"b").unwrap();
        let a_str = m.alloc_string(b"a".to_vec());
        m.dict_put(d, b, Object::integer(1)).unwrap();
        m.dict_put(d, a_str, Object::integer(2)).unwrap();
        let a = m.intern(b"a").unwrap();
        let b_str = m.alloc_string(b"b".to_vec());
        assert_eq!(m.dict_get(d, a).unwrap().unwrap().as_i32(), Some(2));
        assert_eq!(m.dict_get(d, b_str).unwrap().unwrap().as_i32(), Some(1));
        assert_eq!(m.dict_known(d, a_str), Ok(true));
        assert_eq!(m.dict_known(d, Object::integer(3)), Ok(false));
        assert!(m.dict_get(d, Object::integer(3)).unwrap().is_none());
        let keys: Vec<_> = m
            .dict_entries(d)
            .unwrap()
            .into_iter()
            .map(|(k, _)| m.name_text(k.as_name().unwrap()).to_vec())
            .collect();
        assert_eq!(keys, [b"b".to_vec(), b"a".to_vec()]);
        assert_eq!(m.dict_len(d), Ok(2));
        assert_eq!(m.dict_maxlength(d), Ok(2));
        m.dict_undef(d, b_str).unwrap();
        assert_eq!(m.dict_known(d, b), Ok(false));
        m.dict_undef(d, b).unwrap();
        assert_eq!(m.dict_len(d), Ok(1));
        let hidden = a_str.with_access(Access::None).unwrap();
        assert_eq!(m.dict_get(d, hidden).err(), Some(VmError::InvalidAccess));
        let long = m.alloc_string(vec![b'n'; crate::names::MAX_NAME_LEN + 1]);
        assert_eq!(
            m.dict_put(d, long, Object::null()),
            Err(VmError::LimitCheck)
        );
        assert_eq!(m.dict_put(a, a, a), Err(VmError::TypeCheck));
        assert_eq!(m.dict_put(d, Object::real(1.0), a), Ok(()));
        assert!(m.dict_get(d, Object::integer(1)).unwrap().unwrap().eq(a));
    }

    // --- files -------------------------------------------------------------

    struct Capability {
        opened: Vec<(Vec<u8>, Vec<u8>)>,
        probe: Probe,
    }

    impl crate::files::FileCapability for Capability {
        fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError> {
            if name == b"%denied" {
                return Err(VmError::InvalidFileAccess);
            }
            self.opened.push((name.to_vec(), mode.to_vec()));
            Ok(Box::new(self.probe.clone()))
        }
    }

    #[test]
    fn file_without_capability_is_undefinedfilename() {
        let mut m = Memory::new();
        assert_eq!(
            m.open_file(b"/etc/passwd", b"r").err(),
            Some(VmError::UndefinedFileName)
        );
        assert_eq!(m.files().watermark(), 0);
        assert!(format!("{m:?}").contains("file_capability: false"));
    }

    #[test]
    fn file_capability_issues_streams() {
        let mut m = Memory::new();
        let probe = Probe::with_input(b"data");
        m.set_file_capability(Some(Box::new(Capability {
            opened: Vec::new(),
            probe: probe.clone(),
        })));
        assert_eq!(
            m.open_file(b"%denied", b"r").err(),
            Some(VmError::InvalidFileAccess)
        );
        let f = m.open_file(b"%in", b"r").unwrap();
        assert_eq!(f.ty(), Type::File);
        assert_eq!(f.space(), Some(Space::Local));
        assert!(m.file_is_open(f));
        let mut buf = [0; 8];
        assert_eq!(m.file_read(f, &mut buf), Ok(4));
        assert_eq!(&buf[..4], b"data");
        assert_eq!(m.file_write(f, b"out"), Ok(3));
        assert_eq!(probe.output(), b"out");
        let ro = f.with_access(Access::ReadOnly).unwrap();
        assert_eq!(m.file_write(ro, b"x"), Err(VmError::InvalidAccess));
        assert_eq!(m.file_read(ro, &mut buf), Ok(0));
        let none = f.with_access(Access::None).unwrap();
        assert_eq!(m.file_read(none, &mut buf), Err(VmError::InvalidAccess));
        assert_eq!(
            m.file_read(Object::null(), &mut buf),
            Err(VmError::TypeCheck)
        );
        assert!(!m.file_is_open(Object::null()));
        m.close_file(none).unwrap();
        assert!(probe.closed());
        assert!(!m.file_is_open(f));
        assert_eq!(m.close_file(f), Ok(()));
        assert_eq!(m.close_file(Object::integer(0)), Err(VmError::TypeCheck));
        m.set_global(true);
        let g = m.open_file(b"%g", b"w").unwrap();
        assert_eq!(g.space(), Some(Space::Global));
        m.set_file_capability(None);
        assert_eq!(
            m.open_file(b"%in", b"r").err(),
            Some(VmError::UndefinedFileName)
        );
    }

    // --- Property tests ----------------------------------------------------

    #[derive(Clone, Debug)]
    enum Vm {
        Alloc {
            global: bool,
            len: u8,
        },
        Put {
            global: bool,
            slot: u8,
            index: u8,
            value: i32,
        },
        Save,
        Restore(u8),
    }

    fn vm_ops() -> impl Strategy<Value = Vec<Vm>> {
        prop::collection::vec(
            prop_oneof![
                3 => (any::<bool>(), 1u8..6).prop_map(|(global, len)| Vm::Alloc { global, len }),
                4 => (any::<bool>(), any::<u8>(), any::<u8>(), any::<i32>())
                    .prop_map(|(global, slot, index, value)| Vm::Put { global, slot, index, value }),
                2 => Just(Vm::Save),
                1 => any::<u8>().prop_map(Vm::Restore),
            ],
            1..80,
        )
    }

    // Arrays live at the handles they were issued, which stay dense only
    // until a restore discards some and the next allocation skips past them.
    type Arrays = Vec<(Handle, Vec<i32>)>;

    #[derive(Clone, Debug, Default)]
    struct Model {
        local: Arrays,
        global: Arrays,
        saves: Vec<(Object, Arrays)>,
    }

    fn arena_contents(m: &Memory, space: Space, arrays: &Arrays) -> Arrays {
        arrays
            .iter()
            .map(|&(h, _)| {
                let items = m
                    .arena(space)
                    .array(h)
                    .unwrap()
                    .iter()
                    .map(|o| o.as_i32().unwrap())
                    .collect();
                (h, items)
            })
            .collect()
    }

    proptest! {
        #[test]
        fn save_restore_matches_a_model(program in vm_ops()) {
            let mut m = Memory::new();
            let mut model = Model::default();
            let mut last_next = Handle(0);
            for op in &program {
                match *op {
                    Vm::Alloc { global, len } => {
                        m.set_global(global);
                        let a = m.alloc_array(ints(len as usize)).unwrap();
                        let arrays = if global { &mut model.global } else { &mut model.local };
                        let handle = a.handle().unwrap();
                        prop_assert!(arrays.iter().all(|&(h, _)| h < handle));
                        arrays.push((handle, (0..i32::from(len)).collect()));
                    }
                    Vm::Put { global, slot, index, value } => {
                        let space = if global { Space::Global } else { Space::Local };
                        let arrays = if global { &mut model.global } else { &mut model.local };
                        if arrays.is_empty() {
                            continue;
                        }
                        let which = slot as usize % arrays.len();
                        let (h, items) = &mut arrays[which];
                        let i = index as usize % items.len();
                        let a = Object::array(space, *h, items.len() as u32);
                        m.array_put(a, i, Object::integer(value)).unwrap();
                        items[i] = value;
                    }
                    Vm::Save => match m.save(model.saves.len()) {
                        Ok(s) => model.saves.push((s, model.local.clone())),
                        Err(e) => {
                            prop_assert_eq!(e, VmError::LimitCheck);
                            prop_assert_eq!(model.saves.len(), MAX_SAVE_DEPTH);
                        }
                    },
                    Vm::Restore(which) => {
                        if model.saves.is_empty() {
                            continue;
                        }
                        let i = which as usize % model.saves.len();
                        let (s, local) = model.saves[i].clone();
                        prop_assert_eq!(m.restore(s, &[]), Ok(i));
                        for (stale, _) in model.saves.drain(i..) {
                            prop_assert!(!m.save_is_live(stale));
                        }
                        model.local = local;
                    }
                }
                let next = m.arena(Space::Local).next_handle();
                prop_assert!(next >= last_next);
                prop_assert!(model.local.iter().all(|&(h, _)| h < next));
                last_next = next;
                prop_assert_eq!(m.save_depth(), model.saves.len());
                prop_assert_eq!(arena_contents(&m, Space::Local, &model.local), model.local.clone());
                prop_assert_eq!(arena_contents(&m, Space::Global, &model.global), model.global.clone());
                prop_assert_eq!(m.arena(Space::Local).slot_count(), model.local.len());
                prop_assert!(m.arena(Space::Local).slot(next).is_none());
            }
        }
    }

    #[derive(Clone, Debug)]
    enum Op {
        Alloc(u8),
        Put { slot: u8, index: u8, value: i32 },
    }

    fn ops() -> impl Strategy<Value = Vec<Op>> {
        prop::collection::vec(
            prop_oneof![
                (1u8..8).prop_map(Op::Alloc),
                (any::<u8>(), any::<u8>(), any::<i32>()).prop_map(|(slot, index, value)| Op::Put {
                    slot,
                    index,
                    value
                }),
            ],
            1..64,
        )
    }

    fn apply(arena: &mut Arena, handles: &mut Vec<Handle>, op: &Op) {
        match *op {
            Op::Alloc(n) => handles.push(arena.alloc_array(ints(n as usize)).handle().unwrap()),
            Op::Put { slot, index, value } => {
                if handles.is_empty() {
                    return;
                }
                let h = handles[slot as usize % handles.len()];
                let items = arena.array_mut(h).unwrap();
                let i = index as usize % items.len();
                items[i] = Object::integer(value);
            }
        }
    }

    fn contents(arena: &Arena, handles: &[Handle]) -> Vec<Vec<i32>> {
        handles
            .iter()
            .map(|&h| {
                arena
                    .array(h)
                    .unwrap()
                    .iter()
                    .map(|o| o.as_i32().unwrap())
                    .collect()
            })
            .collect()
    }

    proptest! {
        #[test]
        fn snapshot_isolation(before in ops(), after in ops()) {
            let mut arena = Arena::new(Space::Local);
            let mut handles = Vec::new();
            for op in &before {
                apply(&mut arena, &mut handles, op);
            }
            let frozen = contents(&arena, &handles);
            let snapshot = arena.clone();
            let mut live_handles = handles.clone();
            for op in &after {
                apply(&mut arena, &mut live_handles, op);
            }
            prop_assert_eq!(&contents(&snapshot, &handles), &frozen);
            prop_assert_eq!(snapshot.next_handle(), Handle(handles.len() as u32));
            for h in &live_handles[handles.len()..] {
                prop_assert!(snapshot.slot(*h).is_none());
            }
            drop(arena);
            prop_assert_eq!(&contents(&snapshot, &handles), &frozen);
        }

        #[test]
        fn snapshot_shares_structure(before in ops(), after in ops()) {
            let mut arena = Arena::new(Space::Local);
            let mut handles = Vec::new();
            for op in &before {
                apply(&mut arena, &mut handles, op);
            }
            let storage = |a: &Arena, h: Handle| match a.slot(h).unwrap() {
                Slot::Array(items) => items.clone(),
                _ => unreachable!(),
            };
            // Taking the snapshot copies nothing: every slot's storage is still
            // held by exactly one slot table entry.
            let snapshot = arena.clone();
            for &h in &handles {
                prop_assert_eq!(Rc::strong_count(&storage(&arena, h)), 2);
                prop_assert!(Rc::ptr_eq(&storage(&arena, h), &storage(&snapshot, h)));
            }
            let mut written = std::collections::HashSet::new();
            let mut live_handles = handles.clone();
            for op in &after {
                if let Op::Put { slot, .. } = op
                    && !live_handles.is_empty()
                {
                    written.insert(live_handles[*slot as usize % live_handles.len()]);
                }
                apply(&mut arena, &mut live_handles, op);
            }
            // Written slots diverged into two versions; the rest still share
            // one allocation between the two tables (held at most twice, by
            // the original leaf and its path copy). `live` and `old` are two
            // more holders each.
            for &h in &handles {
                let live = storage(&arena, h);
                let old = storage(&snapshot, h);
                if written.contains(&h) {
                    prop_assert!(!Rc::ptr_eq(&live, &old));
                    prop_assert_eq!(Rc::strong_count(&live), 2);
                    prop_assert_eq!(Rc::strong_count(&old), 2);
                } else {
                    prop_assert!(Rc::ptr_eq(&live, &old));
                    prop_assert!(Rc::strong_count(&live) <= 4);
                }
            }
        }

        #[test]
        fn handles_are_never_reused(program in ops()) {
            let mut arena = Arena::new(Space::Global);
            let mut handles = Vec::new();
            for op in &program {
                let next = arena.next_handle();
                apply(&mut arena, &mut handles, op);
                if let Op::Alloc(_) = op {
                    prop_assert_eq!(*handles.last().unwrap(), next);
                    prop_assert!(arena.next_handle() > next);
                }
            }
            let mut sorted = handles.clone();
            sorted.sort();
            sorted.dedup();
            prop_assert_eq!(sorted.len(), handles.len());
            prop_assert_eq!(arena.slot_count(), handles.len());
        }

        #[test]
        fn unknown_handles_resolve_to_nothing(program in ops(), probe in any::<u32>()) {
            let mut arena = Arena::new(Space::Local);
            let mut handles = Vec::new();
            for op in &program {
                apply(&mut arena, &mut handles, op);
            }
            let known = arena.next_handle().0;
            prop_assert!(arena.slot(Handle(known)).is_none());
            prop_assert!(arena.array(Handle(known)).is_none());
            prop_assert!(arena.slot_mut(Handle(known)).is_none());
            prop_assert!(arena.slot(Handle(u32::MAX)).is_none());
            if probe >= known {
                prop_assert!(arena.slot(Handle(probe)).is_none());
                prop_assert!(arena.string_mut(Handle(probe)).is_none());
            } else {
                prop_assert!(arena.slot(Handle(probe)).is_some());
            }
        }

        #[test]
        fn map_matches_a_reference_model(
            writes in prop::collection::vec((any::<u32>(), any::<u16>()), 0..200),
            probes in prop::collection::vec(any::<u32>(), 0..50),
        ) {
            let mut map = PersistentMap::new();
            let mut model = std::collections::HashMap::new();
            for &(k, v) in &writes {
                prop_assert_eq!(map.insert(k, v), model.insert(k, v));
            }
            prop_assert_eq!(map.len(), model.len());
            for &(k, _) in &writes {
                prop_assert_eq!(map.get(k), model.get(&k));
            }
            for &k in &probes {
                prop_assert_eq!(map.get(k), model.get(&k));
            }
        }
    }
}
