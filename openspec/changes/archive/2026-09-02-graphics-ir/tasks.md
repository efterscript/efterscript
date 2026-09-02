# Tasks: graphics-ir

## 1. Boundary (ps-vm)

- [x] 1.1 `GraphicsBackend` trait and installation on `Interp`; group registered only with a backend
- [x] 1.2 `ops/graphics.rs`: state, matrix, path, paint, clip, page operators popping/checking operands
- [x] 1.3 `image`/`imagemask`: dict form, data acquisition from procedures/strings/files via loop frames
- [x] 1.4 `setpagedevice`/`currentpagedevice` with tolerant recording
- [x] 1.5 save/restore ↔ gsave/grestore plumbing (depth already passes through)

## 2. Graphics state (ps-graphics)

- [x] 2.1 `GState` stack, CTM math, defaults; current path beside the stack
- [x] 2.2 Path building incl. arc-to-Bézier; `currentpoint`, `pathbbox`
- [x] 2.3 Colour spaces as resources: device spaces, Separation, DeviceN, Indexed; captured tint-transform source
- [x] 2.4 Clip stack; clip ops into the IR

## 3. IR and delivery

- [x] 3.1 `Page`, `IrOp`, `Seg`, `Resources`; spans on ops
- [x] 3.2 Lazy state emission with dedup; paint ops carry their path
- [x] 3.3 `PageSink`; `showpage`/`copypage`/`erasepage` semantics
- [x] 3.4 Versioned canonical dump (`ir/1`) with shared number formatting

## 4. Tooling and corpus

- [x] 4.1 `efterscript ir <file>`
- [x] 4.2 `difftest run`: sidecar `.ir` golden comparison under `corpus/golden/ir/`
- [x] 4.3 Corpus files under `corpus/unit/graphics/` for every scenario, with goldens

## 5. Verification

- [x] 5.1 Unit tests per scenario; backend tested without an interpreter
- [x] 5.2 Property tests: CTM round trips (`transform`/`itransform`), dump determinism
