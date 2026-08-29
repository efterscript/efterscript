# Tasks: vm-object-model

## 1. Object type

- [x] 1.1 `Object` value: header layout, payload union, `Copy`, `size_of == 16` test
- [x] 1.2 Constructors and accessors per type; executable/literal and access bits
- [x] 1.3 `eq` semantics (identity for composites, cross-type numeric equality)

## 2. Name table

- [x] 2.1 Interned atoms, append-only, stable ids
- [x] 2.2 Name length handling per PLRM3 implementation limits

## 3. Arena and persistent slot map

- [x] 3.1 Array-mapped trie keyed by `u32` handle with path-copying insert/replace
- [x] 3.2 `Slot` variants and `Shared<T>` alias
- [x] 3.3 Allocation in the arena selected by `setglobal`
- [x] 3.4 Property tests: snapshot isolation, structural sharing, no cycles

## 4. save / restore

- [x] 4.1 `SaveRecord` and the save stack with nesting limit
- [x] 4.2 `restore` validity checks (stack watermark scan, save-object liveness)
- [x] 4.3 Reversion, gstate depth, file closing, invalidation of newer saves
- [x] 4.4 Corpus files under `corpus/unit/vm/` for every scenario in the spec

## 5. Global/local rule

- [x] 5.1 Store-time check shared by `put`, `def`, `putinterval`, `copy`, scanner
- [x] 5.2 Corpus files for `invalidaccess` cases

## 6. Dictionaries and strings

- [x] 6.1 Insertion-ordered dict with `eq`-consistent hashing
- [x] 6.2 String storage, sub-interval aliasing tests

## 7. Files

- [x] 7.1 File table and capability-issued stream trait
- [x] 7.2 `restore` closes entries above the watermark
