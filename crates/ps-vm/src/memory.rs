// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! VM memory: local and global arenas over a persistent slot map.
//!
//! Sharing happens at two levels. The slot table is a persistent trie, so a
//! snapshot is a clone of its root; slot contents are reference counted and
//! mutated through [`Shared::make_mut`], so a write after a snapshot copies
//! only the storage being written.

use std::rc::Rc;

use crate::names::{Atom, NameTable, NameTooLong};
use crate::object::{Handle, Object, Space, Type};

/// Reference-counted storage for slot contents. An alias so snapshots can
/// move to `Arc` if they ever cross threads.
pub type Shared<T> = Rc<T>;

/// Placeholder until dictionaries land; kept so [`Slot`] has its final shape.
#[derive(Clone, Debug, Default)]
pub struct Dict {}

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
// Memory

/// Local and global VM plus the name table. Allocation goes to the arena
/// selected by `setglobal`.
#[derive(Debug)]
pub struct Memory {
    local: Arena,
    global: Arena,
    names: NameTable,
    allocate_global: bool,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Memory {
            local: Arena::new(Space::Local),
            global: Arena::new(Space::Global),
            names: NameTable::new(),
            allocate_global: false,
        }
    }

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

    pub fn alloc_array(&mut self, items: Vec<Object>) -> Object {
        self.current_arena().alloc_array(items)
    }

    pub fn alloc_packed_array(&mut self, items: Vec<Object>) -> Object {
        self.current_arena().alloc_packed_array(items)
    }

    pub fn alloc_string(&mut self, bytes: Vec<u8>) -> Object {
        self.current_arena().alloc_string(bytes)
    }

    pub fn alloc_dict(&mut self, dict: Dict) -> Object {
        self.current_arena().alloc_dict(dict)
    }

    pub fn alloc_gstate(&mut self, gstate: GState) -> Object {
        self.current_arena().alloc_gstate(gstate)
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let l = m.alloc_array(ints(1));
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
        let a = m.alloc_array(ints(4));
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
        let a = m.alloc_array(ints(2));
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
        let packed = m.alloc_packed_array(ints(1));
        assert!(m.array(packed).is_some());
        assert!(m.intern(b"n").unwrap().as_name().is_some());
        assert_eq!(m.name_text(m.names().lookup(b"n").unwrap()), b"n");
        assert_eq!(m.names_mut().intern(b"n").unwrap(), Atom(0));
    }

    // --- Property tests ----------------------------------------------------

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
