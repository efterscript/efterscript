# vm-object-model Specification

## Purpose
Defines how EfterScript represents PostScript objects and VM memory: the
fixed-size handle-based object value, where the executable and access
attributes live, the two arenas (local and global) with their snapshot-based
`save`/`restore`, the rule that global VM may not reference local VM,
deterministic dictionaries, numeric representation, and capability-gated
file access. Language semantics are the PostScript Language Reference's;
this spec records the representation decisions and the behaviours the
corpus pins down. The design rationale lives in the archived change
`2026-08-29-vm-object-model`.

## Requirements

### Requirement: Object representation

The VM SHALL represent every PostScript object as a fixed-size, copyable
value carrying its type, executable/literal attribute, and payload. Composite
objects SHALL reference VM-owned storage by handle, offset, and length, never
by pointer.

#### Scenario: Composite copies share storage

- **GIVEN** an array object `a`
- **WHEN** a copy `b` of the object is made (e.g. by `dup`)
- **THEN** `a b eq` is true and a `put` through `b` is visible through `a`

#### Scenario: Sub-intervals alias their parent

- **GIVEN** a string `s` and `t` obtained by `s 2 3 getinterval`
- **WHEN** `t 0 65 put` is executed
- **THEN** element 2 of `s` reads as 65 and `s t eq` is false

### Requirement: Attribute placement

The executable/literal attribute SHALL be a property of the object. Access
attributes SHALL be a property of the object for arrays, packed arrays,
strings, and files, and a property of the shared storage for dictionaries.

#### Scenario: readonly array leaves earlier copies writable

- **GIVEN** an array `a` and a copy `b`
- **WHEN** `b readonly pop` is executed
- **THEN** `a 0 1 put` succeeds and `b 0 1 put` raises `invalidaccess`

#### Scenario: readonly dictionary affects all references

- **GIVEN** a dictionary `d` and a copy `e`
- **WHEN** `e readonly pop` is executed
- **THEN** `d /k 1 put` raises `invalidaccess`

### Requirement: Save and restore

`save` SHALL snapshot the local VM in constant time. `restore` SHALL revert
every local composite object to its value at the corresponding `save`,
invalidate every local composite object and save object created after it,
and raise `invalidrestore` if any stack still references such an object.
Global VM SHALL be unaffected by `restore`.

#### Scenario: Local values revert

- **GIVEN** `/a [1 2 3] def save`
- **WHEN** `a 0 9 put restore` is executed
- **THEN** `a 0 get` is 1

#### Scenario: Objects created after save are rejected on the stack

- **GIVEN** `save` followed by `[1 2]` left on the operand stack
- **WHEN** `restore` is executed with that array still on the stack
- **THEN** `invalidrestore` is raised

#### Scenario: Global values persist

- **GIVEN** `true setglobal /g [1] def false setglobal save`
- **WHEN** `g 0 2 put restore` is executed
- **THEN** `g 0 get` is 2

### Requirement: Global VM may not reference local VM

Storing a local composite object into a global composite object SHALL raise
`invalidaccess`, through every storing operator and through the scanner.

#### Scenario: def into a global dictionary

- **GIVEN** a global dictionary `gd` and a local array `la`
- **WHEN** `gd /k la put` is executed
- **THEN** `invalidaccess` is raised and `gd` is unchanged

### Requirement: Deterministic dictionaries

Dictionary enumeration order SHALL be insertion order. Keys SHALL compare with
`eq` semantics, with integer-valued reals equal to the corresponding integer.

#### Scenario: forall order

- **GIVEN** `<< /b 1 /a 2 >>`
- **WHEN** enumerated with `forall`
- **THEN** keys are visited in the order `/b`, `/a`

### Requirement: Numeric representation

Integers SHALL be 32-bit signed; reals SHALL be single-precision. Integer
arithmetic that overflows SHALL yield a real.

#### Scenario: Overflow promotes

- **GIVEN** `2147483647 1 add`
- **THEN** the result is a real equal to 2147483648.0

### Requirement: No ambient file access

File objects SHALL reference streams issued by the embedder; the VM SHALL have
no built-in path from a program to the host filesystem.

#### Scenario: file with no capability

- **GIVEN** a VM constructed with no file capability
- **WHEN** `(/etc/passwd) (r) file` is executed
- **THEN** `undefinedfilename` is raised and no host file is opened
