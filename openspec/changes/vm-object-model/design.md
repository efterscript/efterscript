# Design: Object model and VM memory

Language semantics are the PostScript Language Reference's (PLRM3 §3.3
objects, §3.7 memory management); this document records only how EfterScript
represents them and why. Where the spec leaves a choice, the choice is
recorded here as a decision.

## 1. Goals and constraints

- `save` and `restore` must be cheap enough to wrap every job and every
  `stopped` context, and exact: every local composite value reverts, every
  object created after the save becomes unusable.
- Composite objects must share storage and compare by identity, including
  sub-intervals of arrays and strings.
- Global and local VM must be separate, with the "global may not reference
  local" rule enforceable at store time.
- Deterministic behaviour (dictionary iteration order, number formatting) so
  golden tests are stable.
- No ambient authority: files and other external resources are reachable only
  through handles the embedder issued.
- Single-threaded execution; snapshots may later be moved to other threads.

## 2. The object

An `Object` is a 16-byte `Copy` value: a 4-byte header and a 12-byte payload.

```
header  type:6  exec:1  access:2  space:1  packed:1  (rest reserved)
payload simple:    i32 | f32 | bool | atom:u32 | op:u32 | (none)
        composite: handle:u32  offset:u32  length:u32
```

Decisions:

- **Integers are `i32`, reals are `f32`.** These are the ranges the spec's
  implementation limits describe, and single-precision reals are what
  reference implementations print and round with. Arithmetic overflows from
  integer to real as the spec requires. Widening either type would change
  observable output (`cvs`, `=`) and is not a goal.
- **Composite objects are handles, not pointers.** The payload names a slot in
  a VM arena plus an offset and length into that slot's storage. Two objects
  are the same composite object (`eq`) iff they have the same space, handle,
  offset, and length. `getinterval` therefore allocates nothing: it copies
  the object and narrows offset/length. Because arrays contain handles rather
  than references, self-referential structures create no reference cycles.
- **The executable/literal attribute is per object**, in the header. Two
  objects may share one array while one is a procedure and the other a
  literal array.
- **Access is split.** For arrays, packed arrays, strings, and files the
  access attribute lives in the object header, so `readonly` yields a new
  object and earlier copies keep their access. For dictionaries access is a
  property of the storage, so every reference sees it. This is the spec's
  behaviour; the split in representation follows from it.
- **Packed arrays are ordinary arrays with the `packed` bit set** and
  read-only access. The spec allows them to be stored compactly; nothing here
  needs the compaction, and one storage form keeps every array operator
  uniform.
- **Names are interned atoms.** The name table is process-wide, append-only,
  and outside `save`/`restore`. Name equality is atom equality. Names are not
  reclaimed in v1.
- **Operators carry an index** into a static operator table that also holds
  the operator's name for printing and error reporting.
- Simple types: integer, real, boolean, null, mark, name, operator, and the
  save and font-ID types. Composite types: array, packed array, string,
  dictionary, file, gstate.

## 3. VM memory

```
Memory
├── local:  Arena            snapshotted by save, reverted by restore
├── global: Arena            never snapshotted
├── names:  NameTable        process-wide atoms
├── files:  FileTable        capability-issued streams
└── saves:  Vec<SaveRecord>  the save stack (nesting limit per PLRM3 App. B)

Arena
├── slots: PersistentMap<Handle, Slot>   path-copying trie keyed by handle
└── next:  Handle                        monotonically increasing

Slot = Array(Shared<Vec<Object>>) | String(Shared<Vec<u8>>)
     | Dict(Shared<Dict>) | GState(Shared<GState>)
```

`Shared<T>` is a reference-counted pointer (`Rc` by default; the alias exists
so snapshots can become `Arc` if page-level parallelism is ever pursued).

### 3.1 Two-level copy-on-write

The design has two layers of sharing, each doing one job:

1. **The slot table is a persistent map.** Taking a snapshot of an arena is
   copying the map's root: O(1). Inserting a new slot path-copies O(log n)
   nodes. The map is a home-grown array-mapped trie keyed by the dense `u32`
   handle; dense integer keys make this a few hundred lines and avoid a
   dependency with copyleft terms in the core crate.
2. **Slot contents are reference-counted and mutated through `make_mut`.**
   Mutating an array whose storage is referenced only by the current slot
   table happens in place. If a snapshot also holds the storage, `make_mut`
   clones it first, so the snapshot keeps the old version and the live table
   gets the new one.

Together these give the spec's semantics without allocator save-level
tagging: no object carries a save level, and no operator needs to know
whether a save is active.

### 3.2 save

A `save` pushes a `SaveRecord { local_root, local_next, gstate_depth,
file_watermark }`: the local arena's map root (cloned, O(1)), its next-handle
watermark, and the graphics-state stack depth (save performs an implicit
`gsave`). The global arena is untouched. The `save` object returned to the
program carries the record's index and a validity flag.

### 3.3 restore

`restore` first checks validity: the save object must be the innermost live
one (or an outer one, in which case the inner ones are discarded), and the
operand, dictionary, and execution stacks must contain no local composite
object whose handle is at or above the record's watermark — otherwise
`invalidrestore`. It then replaces the local map root with the saved root,
pops graphics states to the saved depth, closes files opened since the save,
and invalidates every save object created after it. Dropping the discarded
root frees every storage version created since the save, because nothing
else references it.

Handles are never reused. A handle at or above the watermark that survived
the stack check (it cannot, by the rules below, but defensively) resolves to
no slot and raises `invalidaccess` rather than misbehaving.

### 3.4 Local and global VM

Each arena has its own handle space; the header's `space` bit says which.
Allocation goes to the arena selected by `setglobal`. Storing an object into
a composite object checks the rule: a global composite may not contain a
local composite, and `put`, `def`, `putinterval`, `copy`, and the scanner all
route through the same check, raising `invalidaccess`. This rule is what
makes `restore` safe: after the stack check, no reachable object can name a
discarded slot.

### 3.5 Dictionaries

`Dict` is an insertion-ordered hash map so `forall` order is deterministic.
Keys are objects compared with `eq` semantics; integer-valued reals and
integers hash and compare equal, string keys are converted to names at
store time as the spec requires. Dictionaries grow automatically (Level 2
semantics); `maxlength` is reported but not enforced as a limit.

### 3.6 Strings

Strings are byte vectors. A string object is a handle plus offset/length,
so sub-strings alias their parent through the slot; a `put` through either
object goes through the same `make_mut` and is visible through both.

### 3.7 Files

File objects are handles into the file table; the table entry owns a stream
provided by the embedder through the capability layer. The interpreter has no
way to open a file except through an injected capability. Entries created
after a save are closed by `restore`.

### 3.8 Memory reclamation

No garbage collector in v1. `restore` reclaims local VM (job encapsulation
wraps every job in save/restore, so session mode does not accumulate);
reference counting reclaims versions superseded by copy-on-write. `vmreclaim`
is accepted and ignored; `vmstatus` reports arena counters. A collector for
global VM is a future change.

## 4. Scanner interface

The scanner allocates through `&mut Memory`: names via the name table,
strings and procedure bodies (`{ … }`) in the arena currently selected by
`setglobal`. It produces `Object` values and nothing else, so the token
stream and the runtime share one representation with no conversion step.

## 5. Alternatives considered

- **`Rc<RefCell<…>>` graphs** — direct, but save/restore then needs a
  save-level tag on every object and a walk to revert, and cycles leak.
- **Allocator save-level tagging** (the classic implementation) — well
  understood, but couples every mutation to the save stack and makes the
  snapshot a property of the allocator rather than of a value.
- **Fully persistent structures for slot contents too** — simplest to reason
  about, but every `put` becomes O(log n) and string-heavy jobs (font
  downloads, `putinterval` loops) pay for it constantly; copy-on-write at the
  slot level pays only at the first write after a save.
- **`f64` reals** — tempting for precision, rejected for fidelity.

## 6. Open questions

- Whether `gstate` objects live in the arena (as above) or in a dedicated
  table owned by the graphics layer; decided when `ps-graphics` is designed.
- Name-table reclamation for long-running sessions that generate names
  dynamically; measure before deciding.

## 7. Implementation notes

Recorded where the code departs from, or pins down, the text above.

- **`Object::eq` decides only what the values alone can decide**: numbers
  across integer/real, simple objects by payload, composites by identity
  (space, handle, offset, length; attributes ignored). The `eq` operator's
  comparison of string contents, and of a string against a name, needs
  storage access and is layered on top of this in the operator layer, so the
  object type stays free of any reference to `Memory`.
- **The save object carries only the record's index.** A `Copy` value cannot
  be invalidated in place, so validity is a property of the save stack and is
  checked there; the payload has two spare words if a generation tag later
  proves useful.
- **A snapshot is a clone of the `Arena`**, not a separate `local_root` /
  `local_next` pair: `Arena` is `Clone` and cloning it is O(1), so a
  `SaveRecord` can hold an `Arena` directly. Same semantics, one fewer type.
- **Packed arrays share the `Array` type tag** and are distinguished by the
  packed bit, exactly as §2 describes. `Object::ty()` reports `PackedArray`
  when the bit is set so the `type` operator needs no special case.
- **The trie's root depth grows on demand** (a root at shift 0 addresses 32
  handles; each growth adds five bits) rather than being fixed at seven
  levels, so small jobs take short paths. A key beyond the current capacity
  is a miss, never a panic.
- **The name length limit is 127 bytes** (PLRM3 Appendix B); interning a
  longer name returns an error the interpreter maps to `limitcheck`.
- **The save object carries a serial number, not a stack position.** A
  position would be reused by the next `save` after a `restore`, making a
  stale save object valid again; a serial never repeats, and liveness is a
  scan of the (at most 15-deep) save stack for that serial.
- **`save` takes the graphics-state depth as an argument and `restore`
  returns it.** No graphics stack exists in `ps-vm`; recording the depth in
  the record and handing it back is all §3.2/§3.3 need from this crate, and
  the owner of the stack does the popping.
- **File objects carry the space bit of the arena selected when they were
  opened**, so the global/local rule treats a file like any other composite,
  while their handle indexes the file table rather than an arena. `restore`
  closes every entry at or above the record's file watermark regardless of
  space, and the stack scan treats a local file above the watermark as
  `invalidrestore`, like any other newer local composite.
- **Errors are a plain `VmError` enum** carrying the PostScript error name.
  Operator-level distinctions the storage cannot make (`undefined` versus a
  missing key for `known`) are left to the operator: `dict_get` returns
  `Option`.
- **Dictionary keys hash a reified `eq`.** Integer-valued reals in `i32`
  range fold onto the integer; other reals key by bit pattern, so a NaN key
  can be found again (the `eq` operator says NaN is not `eq` to itself, but
  a key that can never be retrieved is worse than a benign departure) and
  two large integers that `eq` compares equal through `f32` rounding remain
  distinct keys. Both are outside any behaviour the corpus pins down.
- **Access on dictionaries only tightens**, matching the object-level
  operators: `dict_set_access` to a more permissive level is
  `invalidaccess`.
- **`Memory::alloc_array` and `alloc_packed_array` apply the global/local
  rule** and so return `Result`; this is the scanner's storing path from
  §3.4. `Arena::alloc_*` stay raw.
- **The overflow-promotion scenario has a corpus file but no Rust test
  yet**: integer arithmetic belongs to the operator layer, not to this
  change's memory API.
