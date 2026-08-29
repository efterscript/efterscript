# Change: Object model and VM memory

## Why

Every other component consumes PostScript objects: the scanner produces them,
the interpreter executes them, the graphics layer reads them, fonts are
dictionaries of them. The way composite objects share storage, how `save`
and `restore` snapshot and revert that storage, and how local and global VM
are separated are decisions that touch every operator and cannot be changed
later without rewriting the interpreter. They must be fixed before the first
line of scanner code.

## What changes

- Defines the in-memory representation of a PostScript object (16-byte value
  with type, attributes, and payload) and the rule that composite objects are
  *handles* into VM-owned storage, never pointers.
- Defines VM memory as two arenas (local, global) whose slot tables are
  persistent maps, with per-slot copy-on-write storage, so that `save` is an
  O(1) snapshot and `restore` is an O(1) swap.
- Defines object identity (`eq`), the executable/literal attribute, and the
  split of access attributes between object and storage.
- Defines the name table, file table, and save records as VM-owned resources
  outside the snapshot mechanism.
- Defers garbage collection; memory is reclaimed by `restore` and reference
  counting.

## Impact

- New capability spec: `vm-object-model`.
- Code: `crates/ps-vm` (`object`, `memory`, `names` modules). No other crate
  exists yet; `ps-graphics` and `ps-fonts` will build on these types.
- No user-visible behaviour yet.
