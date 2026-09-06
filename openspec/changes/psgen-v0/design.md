# Design: psgen v0

See proposal.md. This document fixes the generator's model, the random
source, the property runner, the shrinker, and the budget.

## Context

- `difftest` runs programs in process through `Interp` with capture
  streams and the graphics backend, and its oracle subcommand takes
  paths. `ps_graphics::dump` gives a canonical IR text. `remelt::distill`
  produces a PDF; `pdf-out`'s structural checks exist as test helpers.
- `Limits` covers stacks only; nothing bounds execution.
- `tools/psgen` is a stub; `corpus/generated/` is empty.

## Goals / Non-Goals

**Goals:** programs that are well formed and mostly well typed, cheap
to generate and run by the thousand, checkable without an oracle, and
shrinkable; a seed-file contract CI can run.

**Non-Goals:** fonts beyond the resident set, images, composite text,
syntax fuzzing, coverage guidance.

## Decisions

**D1. Grammar with a static stack model.** The generator keeps a model
of the operand stack as a list of abstract types (int, real, bool,
string, name, array, proc, dict, mark, any) and of the dictionary
stack's definitions (name → type). Each production consumes and
produces model types; an operator is chosen from those whose inputs
the model can satisfy, else a literal is pushed. Procedures are
generated with their own model and a declared signature so calls type
check. Loops are `repeat` and `for` with small literal counts, `loop`
only with an `exit` reached by a counter; `stopped` wraps a share of
blocks. An `ill_typed` share replaces one operand's type at a chosen
operation so the error machinery runs; the program ends in `stopped`
handling in that case so it terminates normally. *Alternative:* a
purely random token stream — nearly every program dies on its first
operator and exercises nothing.

**D2. Own random source.** A 64-bit xorshift seeded from the seed and
index; no dependency; deterministic across platforms because the
generator uses only integer arithmetic.

**D3. Program shape.** A program is a list of statements, each a line;
the `graphics` profile prefixes a page size and ends with `showpage`;
the `core` profile prints its final stack with `pstack` so output
carries the computation. Statements are the unit for shrinking and
for the metamorphic wrappers.

**D4. Runner and properties.** `psgen check` runs a program through
`Interp` with capture streams, the graphics backend with a collecting
sink, and the budget; then the variants: rerun (determinism);
`save`/`restore` around the statements chosen as the "block" (all but
the final print), comparing the output produced after the block;
`gsave`/`grestore` around one paint statement, comparing IR; `N M
translate` prepended, comparing IR with every coordinate shifted
(stroke matrices and image matrices translate their offsets; text
matrices too); two `def` statements with disjoint names swapped,
comparing output; `remelt::distill` to memory and the structural check
from `pdf-out`'s test support promoted to a small `pdf_out::check`
module behind a `check` feature (or duplicated into psgen if the
feature is undesirable — decide at implementation, record). A failure
is a `Failure { property, seed, index, detail }`.

**D5. Shrinker.** Statement-level delta debugging (ddmin over the
statement list, then single-statement removal until fixed point) under
a predicate: a named property re-evaluated in process, or an external
command given the file path and returning non-zero for "still fails"
(for oracle failures). Removing statements can make a program ill
typed; that is fine, since the predicate decides. The result keeps the
header and adds `% psgen: shrunk from seed S index I property P`.

**D6. Budget in `ps-vm`.** `Limits { steps: Option<u64> }`, counted in
the execution loop per object executed; exceeding raises
`VmError::LimitCheck` on the current object through the normal error
path (so `stopped` can catch it — the generator's programs must not
loop in a handler; the budget is checked again after the handler, so a
handler that loops is stopped too, recorded). `psgen` sets it from the
profile (default 200 000).

**D7. Seed files and the xtask.** `corpus/generated/core.seeds` and
`graphics.seeds` hold `seed count` lines with `#` comments;
`cargo xtask fuzz-round [--profile p] [--oracle name]` generates to
`target/psgen/<profile>/<seed>/`, checks, and with `--oracle` (private
tier) invokes `difftest oracle` over the directory; exit non-zero on
any failure; summary per profile. Initial seed files: ten seeds of one
hundred programs each per profile, chosen so the run stays under a
minute in debug builds.

**D8. First fuzz round is part of the change.** The implementer runs
both profiles with more seeds than the files list (thousands of
programs), shrinks every failure, and records the minimal programs and
their classification in the notes; any interpreter bug found is
reported, not fixed here, unless trivial and clearly in scope. In the
private tier, the oracle over the generated output adds the reference
comparison; its failures are triaged the same way.

## Risks / Trade-offs

- [Properties too weak to find anything] → the first round tells; the
  translation and save/restore relations are strong for the graphics
  and VM layers respectively.
- [Generated programs are unreadable when they fail] → shrinking is in
  v0 for this reason.
- [Budget changes error semantics] → opt-in, unset by default, and
  raised as an ordinary `limitcheck`.

## Implementation notes

### Part 1

Covers the budget (1.1), the generator, runner, relations, and shrinker
(2.1–2.4), and the seed files with the xtask (3.1). Part 2 runs the
first round beyond the seed files and closes the verification list.

- **The budget (D6).** `Limits::steps: Option<u64>` (default `None`) is
  charged once per object the loop executes — `Frame::Object` objects,
  procedure elements, scanned tokens — and once per loop iteration, so
  `{ } loop` with an empty body is bounded too. The first exceed raises
  `limitcheck` on the object (or the loop operator) through `raise`, so
  `stopped` catches it and `errordict` entries run; at that moment the
  limit in force grows by a grace of one sixteenth of the budget (at
  least 1000 objects). The second exceed is raised the same way, but no
  further grace is given, so every object executed after it raises
  again: a handler that loops nests error handlers until
  `MAX_NESTED_ERROR_HANDLERS` unwinds to the run boundary, and a
  `stopped` that catches and continues fails on its next object. The
  count is per interpreter, not per job. `Interp::steps()` and
  `Interp::budget_exceeded()` expose it. No corpus file sets a budget;
  `difftest run` and the goldens are unchanged.
- **Random source (D2).** `rng::Rng`: xorshift64 seeded from
  `seed·φ + index·κ + c` (64-bit wrapping multiplies), four warm-up
  steps; all generator arithmetic is integer, reals are written from
  hundredths (`Generator::real_text`).
- **The model (D1).** `model::Model` holds the operand stack as items
  `{ty, mag, id, epoch}`: a magnitude bound (the value is below
  `10^mag`; integer results are only offered below ten digits, reals
  below thirty, so nothing overflows into a real or to infinity), the
  composite the item refers to (tracked arrays know their element
  types, strings their length, dictionaries their entries, procedures
  their signature; `id: None` is an opaque composite from a procedure
  call, `cvs`, or `search`), and the save level it was created in. The
  types are `Int Real Bool String Name ExecName Array Proc Dict Mark
  Null Any`; `ExecName` is what `type` returns, stored but never read
  back through a variable since that would execute it. The dictionary
  stack is a list of tracked dictionaries, `userdict` first; `def`,
  `begin`/`end`, `put`/`undef` on tracked dictionaries, and `restore`
  (a snapshot taken at `save`) keep it exact.
- **Operator statements.** A table of signatures (`Pat` patterns with
  value constraints: a non-zero divisor, an index inside the operand,
  a byte, a shift of at most eight, a small count, coordinates below a
  thousand) drives a generic production: the longest prefix of inputs
  the stack top satisfies is taken from the stack (70 %) or a shorter
  feasible one, the rest are literals. Bodies of `if`, `ifelse`,
  `repeat`, `for`, and `loop` are generated against a copy of the model
  and accepted only when they leave the stack exactly as found (type,
  identity, magnitude, and save level of every item); bodies allowed to
  push are generated against an empty stack, so they cannot read what
  an earlier iteration pushed, and their pushes are opaque. A body that
  clears a current point it started with is rejected. Bodies are
  discarded from the model entirely (composite writes inside them are
  therefore only offered in sequential code: `put` on arrays and
  dictionaries, `undef`; `currentdict` is not offered in procedures).
  Procedures are generated against their declared inputs alone with
  no name references, no definitions, no graphics; calls require the
  inputs' types with magnitudes no larger than the body was generated
  for. Loop counters `cN` and save objects `sN` are defined by the
  statement that uses them and never referenced elsewhere.
- **Operator set, `core`:** `pop dup exch count add sub mul div idiv
  mod neg abs sqrt (as abs sqrt) sin cos atan round truncate floor
  ceiling cvi cvr bitshift and or xor not eq ne lt le gt ge cvs (as 32
  string cvs) cvn type xcheck length get put string search anchorsearch
  (each in an ifelse form that leaves one string) array aload aload pop
  forall (0 exch { add } forall over integer arrays, { pop } forall)
  dict maxlength known put undef currentdict countdictstack = ==
  print`, literals of every type including `[ ]`, `<< >>`, `{ }`, and
  `null`, `/vN … def` with a closed expression or `exch`, `/pN { … }
  bind def`, calls by name and as `/pN load exec`, `/vN load`, `if`,
  `ifelse`, `repeat`, `for` (integer and real controls), `loop` with a
  counter and `exit`, `stopped` blocks, `save`/`restore` blocks, `dict
  begin … end` scopes, and `pstack` last. Not offered: `exp ln log`
  (domain errors), `roll index copy` (bounded but low value for v0),
  `getinterval putinterval astore cvx cvlit`, `forall` over
  dictionaries and strings, `setglobal`.
- **Operator set, `graphics`:** the core set (graphics statements are
  chosen 65 % of the time) plus `setpagedevice` first (four page
  sizes), `moveto lineto rmoveto rlineto curveto rcurveto arc arcn
  closepath newpath fill eofill stroke rectfill rectstroke clip eoclip
  initclip setgray setrgbcolor setcmykcolor setlinewidth setlinecap
  setlinejoin setmiterlimit setflat scale rotate translate`, `matrix
  currentmatrix setmatrix`, `matrix currentmatrix pop`, `findfont
  scalefont setfont` over six resident faces, `show stringwidth
  charpath (true and false) currentpoint pop pop`, `gsave … grestore`
  blocks, then `showpage` and `pstack`. The model tracks the current
  point and whether a font is set; `setdash` and `concat` are left for
  a later profile (`setdash`'s all-zero array and `concat`'s singular
  matrices need value rules). Not offered by design: `initmatrix
  setmatrix defaultmatrix initgraphics transform itransform dtransform
  idtransform clippath pathbbox copypage erasepage nulldevice` — each
  observes or resets the device transform and would break the
  translation relation legitimately.
- **Bounds.** `core`: 12–40 statements, 1–5 per block, nesting depth
  3, loop counts 0–5, integers in ±1000, reals in ±1000.00, strings up
  to 8 letters, arrays up to 6 elements, procedures with up to 2 inputs
  and 1–5 statements, stack capped at 60. `graphics`: 15–50 statements,
  loop counts 0–4, coordinates 0–500 (integers or tenths), deltas ±50,
  radii and rectangle sides 1–120, angles in multiples of 15°, colour
  components in hundredths, line widths 0–8, miter limits 1–10,
  flatness 0.2–5, scale factors 0.5–2, rotations in ±90° by 15°, font
  sizes 6–36. Ill-typed share 5 % of eligible statements by default;
  budget 200 000 in both profiles.
- **Ill-typed statements.** All operands are literals; one whose
  pattern rejects some type is replaced by a literal of a rejected
  type; the statement becomes `{ … } stopped { (psgen: caught ) print
  $error /errorname get = clear } if clear` — the trailing `clear` runs
  whether or not the operator objected (`known` with a boolean key does
  not), so the model's empty stack is right either way. `stopped`
  blocks over well-typed code use the same handler without the trailing
  `clear`; a `psgen: caught` line from such a block is therefore a
  finding, and the well-typed scenarios assert its absence.
- **Runner (D4).** `runner::execute` builds a fresh `Interp` per run
  with capture streams and an empty readable `%stdin`, `Graphics` over
  a collecting sink, and the budget; the whole run sits in
  `catch_unwind` and a panic becomes `Run::panic` with the message,
  the interpreter being dropped inside the unwound closure and never
  reused. `runner::distill` goes through `remelt::distill` with
  `compress: false`. `psgen check` installs a silent panic hook so the
  report stays readable.
- **Structural check decision.** The writer's test support is
  assertion-style (`panic!` on every violation) and lives in
  `pdf-out/tests/common`; turning it into a library module would mean
  rewriting it to return errors anyway, and `pdf-out`'s public surface
  is better without a reader. A minimal `Result`-returning duplicate
  lives in `psgen::pdfcheck` (header and binary marker, `startxref`
  and `%%EOF`, the xref table with every in-use offset landing on
  `N 0 obj`, direct stream lengths, resolvable generation-0 references,
  `Size` and `Root`); the inflater is not duplicated since the check
  runs on uncompressed output.
- **Relations as built (D4).** *save-restore*: the body is every
  statement after a leading `setpagedevice` and before the trailing
  `showpage`/`pstack`/`stack` lines; the variant wraps it in
  `/psgen_sr save def … psgen_sr restore` (a named save, so `count`,
  `pstack`, and `clear` inside the body see the same stack). Outcome,
  output, error report, and IR must agree — or `restore` itself ends
  the job with `invalidrestore`, which is what a body that leaves
  objects created inside it on the stack must produce; in that case the
  output before it must be a prefix of the original's, and a second
  variant with `clear` before `restore` must succeed with the output of
  the original minus its tail and the original's IR. (Because the body
  starts at the top of the program, every object on the stack at
  `restore` was created inside it, so the `invalidrestore` branch is the
  usual one for `core`; the graphics state restored by `restore` never
  changes the IR, since `showpage`'s closing of open clips depends only
  on what the last paint left open.) *gsave-grestore*: candidates are
  `fill`/`eofill`/`stroke` lines preceded by a run of path-construction
  lines (`moveto lineto rmoveto rlineto curveto rcurveto arc arcn
  closepath charpath`) that starts after a `newpath`, a paint, the page
  setup, or the start of the body — a point with no current point, so
  `grestore` restores none — and self-contained `rectfill`/`rectstroke`
  lines; the first and last candidate are wrapped in `gsave`/`grestore`
  lines and outcome, output, and IR must agree. Under correct semantics
  nothing hand-written can violate this relation, so its violation
  test exercises the comparison. *translate*: `7 -3 translate` is
  inserted after the page setup; the IR dumps are normalised (each
  `stroke-ctm` is attached to its `S`, the identity when absent) and
  compared line by line with the `m l c` coordinates, the `S` and
  `text` matrix translations, and the `Do` image matrix translation
  shifted by (7, −3) within `0.002 + 10⁻⁵·|v|` (the dump's six
  significant digits); everything else must match exactly. *reorder-
  defs*: adjacent lines of the form `/name <closed expression> def`
  with distinct names are swapped (first and last such pair); a closed
  expression is literals, `[ ]`, `<< >>`, `{ }`, and pure operators
  with a net stack effect of one push and nothing read from below —
  bare names disqualify the statement. Dictionary enumeration follows
  insertion order, so a program that `forall`s its dictionary observes
  the swap; the generator never enumerates dictionaries. Determinism
  compares outcome, output, error report, and IR of two runs; no
  operator of the interpreter is non-deterministic (`rand`,
  `realtime`, `usertime` are not registered), so the planted
  determinism failure is exercised at the comparison.
- **Shrinker (D5).** ddmin over the statement list then single
  removal to a fixed point; a compound statement is one line and one
  unit. `--predicate` writes each candidate to `<out>.candidate.ps`
  and runs `sh -c "<command> <path>"`, non-zero meaning still failing;
  the file is removed afterwards. The result is `<file>.min.ps`
  (`--out` overrides) with `% psgen: shrunk from profile=… seed=…
  index=… property=…` (or `predicate=…`) appended to the header.
- **CLI.** `psgen gen --profile <p> --seed <n> --count <k> --out <dir>
  [--ill-typed <share>]` writes `<p>-<seed>-<index:04>.ps`; `psgen
  check` with the same options (`--out` optional) or with files; a
  failure line is `fail <property> <path> seed=<n> index=<i>: <detail>`
  (`seed=- index=-` for a file without a header) and the summary
  `psgen: <n> programs checked, <f> failed`; exit 1 on failures, 2 on
  usage. `cargo xtask fuzz-round` runs `cargo run -q -p psgen -- check
  … --out target/psgen/<profile>/<seed>` per seed line, sums the
  summaries, and with `--oracle <name>` runs `cargo run -q -p difftest
  -- oracle --profile <name> <dirs…>` per profile with the environment
  inherited.
- **Seed files and timing (D7).** Ten seeds of 100 programs per
  profile; the round over both files (2000 programs, each run five to
  nine times) takes 26 s wall in a debug build on the development
  container, all passing. A sweep of 1500 further programs per profile
  (seeds 77 and 78, default ill-typed share) before staging also found
  nothing; the generator's own well-typed scenarios were swept over 800
  programs per profile while the grammar was being corrected. The
  properties have therefore not yet found an interpreter bug: part 2's
  round should widen the grammar (the additions listed below) as much
  as it widens the seeds.
- **Grammar corrections the sweeps forced** (recorded so part 2 does
  not repeat them): `load` takes the literal name; `and`/`or`/`xor`
  need the same type on both sides, not the same class; a loop body
  that may push must not read the base stack, and must not clear a
  current point it started with; `if` takes its condition before its
  body is generated; body effects on tracked composites must not leak
  into the model; a balanced body must preserve save levels, or a
  new string in an old string's place survives to `restore`.
- **What part 2 must know.** Task 4.1's round beyond the seed files
  is `psgen check --profile <p> --seed <s> --count <k>` per seed (a
  thousand programs take about fifteen seconds per profile in a debug
  build) and `psgen shrink … --property <name>` per failure; the
  private tier is `cargo xtask fuzz-round --oracle <name>` with
  `EFTERSCRIPT_HELLBOX` set, and oracle failures shrink with
  `--predicate`. The grammar does not yet emit `setdash`, `concat`,
  `roll`, `index`, `copy`, `getinterval`, `astore`, dictionary
  `forall`, or images; each is a bounded addition to the tables in
  `grammar.rs` (`Pat` value rules and a result function). The
  well-typed scenarios can be widened with `PSGEN_PROGRAMS=<n>` on the
  `profiles` integration test. `openspec validate psgen-v0` and the
  design's amendment for any relation that the round shows to be
  unsound remain for 4.2.

### Part 2 — rounds

Covers 4.1 and 4.2: the grammar widened as part 1 asked, the rounds
beyond the seed files, the private-tier round, and the gates.

- **Grammar additions (D8), `core`.** Fixed stack forms, one
  permutation table each: `3 1 roll`, `3 -1 roll`, `4 1 roll`,
  `4 2 roll`, `1 index`, `2 index`, `2 copy`, `3 copy`. `getinterval`
  on tracked arrays and strings: the result shares its source's
  storage, so both become read-only for the grammar (`aliased`), and
  it is as old as its source — a sub-array of storage created before a
  `save` survives the `restore`, which a probe confirmed, so the model
  gives it the source's save level. `putinterval` with a literal that
  fits from the index. `0 array astore` to `3 array astore`. Dictionary
  `forall` as `{ pop pop } forall` on any dictionary and as the count
  `0 exch { pop pop 1 add } forall` on a tracked one — never on
  userdict, whose entry count the save/restore relation changes with
  its named save (the same held for `length`, a hole open since part
  1; see G1). String `forall` (`{ pop } forall`, `0 exch { add }
  forall`). `token` only on literal text — numeric (`token pop exch
  pop`, an integer) or words (`token { pop pop } if`) — since a byte
  `put` into a tracked string could open a string or procedure the
  scanner never sees closed. `cvrs` in radix 10 for any integer and in
  radix 16 after `abs`, the digits of a negative being the declared
  divergence `cvrs-negative-unsigned`. `abs 1 add ln`, `abs 1 add log`,
  `abs 0.5 exp`, `abs 2 exp`, with the domain guarded by construction.
  `cvn`, `search`, `anchorsearch` were already offered.
- **Grammar additions, `graphics`.** `setdash` with an empty array or
  one to three non-negative lengths not all zero and a phase of 0–5;
  `concat` with a diagonal of 0.5–2, skews of 0 or ±0.25 dropped when
  the diagonal-dominance bound on the smallest singular value would
  fall below 0.25, and a translation of ±50; `arcto` as one statement
  with its own `moveto` and a corner between 45° and 135° that is never
  a right angle (I1 and I2 below); `xshow` with one advance per byte of
  a tracked string; `kshow` with `{ pop pop }`; `image` as a
  `gsave … grestore` block placing a 1×1 to 4×4 gray 8-bit sample block
  from a string or a procedure source; and the readings `currentpoint`
  and `pathbbox` (raw, and `pathbbox pop pop pop pop`) where the model
  knows a current point. `eoclip` and `setmiterlimit` were already
  offered. Coordinates and deltas taken from the stack may now be of
  magnitude 4 so readings can feed path construction.
- **Excluded, with reasons.** `transform`, `itransform`, `dtransform`,
  `idtransform`, and the values of `currentmatrix` read device
  coordinates, which a translation of the program changes
  legitimately; the `matrix currentmatrix setmatrix` and `… pop` forms
  stay. `currentpoint` and `pathbbox` read *user-space* coordinates,
  which the translation moves with the program, so the relation
  tolerates them up to rounding (next item). `cvx`/`exec` of arbitrary
  objects, `yshow`/`xyshow`/`ashow`/`widthshow` (the `xshow` rule would
  serve them; left for a later profile), and `imagemask` are not
  offered; `setdash` with an all-zero array and `concat` with a singular
  matrix raise errors by design and are not generated.
- **Inexact readings.** A number read back through the CTM carries a
  rounding error near `10⁻⁷·|t|/s` in user space (t the device
  translation, s the scale), and a translation prepended to the
  program changes that error. The model marks such numbers `inexact`
  (`Item::inexact`) and never hands one — or anything computed from one
  — to an operator whose result jumps at a boundary or amplifies the
  error (`Op::exact_inputs`: `cvi round truncate floor ceiling cvs cvrs
  eq ne lt le gt ge atan mul div`, the root and the square); `add sub
  neg abs sin cos cvr ln log` and the stack forms propagate the mark;
  procedures never take one (their bodies were generated without
  knowing); loop bodies must preserve it (`Item::same_as`). To bound
  the error itself the model keeps the CTM's scale band (`Gfx::lo`,
  `Gfx::hi`, thousandths) inside [1/8, 8]: `scale` and `concat` refuse
  a factor that would leave it and appear only in sequential code, so
  a loop cannot compound them; `gsave` and `save` blocks hand the band
  back. The `translate` relation then compares output numbers within
  `0.01 + 10⁻⁵·|v|` (`Numbers::Close`, only there; every other relation
  stays exact), reporting the first line beyond tolerance; the IR
  tolerance is unchanged, since the device-space error of a reading fed
  back to a path is `10⁻⁷·|t|`, below its 0.002.
- **Throughput.** Release build, one process: `core` 1 000 programs in
  0.95–0.97 s (about 1 050 programs/s), `graphics` 1 000 in 1.64–1.65 s
  (about 610/s), each program run five to nine times by the properties.
  Debug build: the committed seeds (2 600 programs) in about 30 s.
- **Batches.** Release build, four processes: `core` and `graphics`
  at the default ill-typed share over seeds 100–119 (20 000 programs
  each) and at 0.25 over seeds 200–219 (20 000 each). First pass, before
  the fixes below: 79 000 programs (one seed lost to G2), 24 failures.
  Final pass over the same seeds: **80 000 programs, 0 failures, 32 s
  wall.** Before those, the committed seeds under the widened grammar
  (2 000 programs, 26 failures, all T1) and a probe of seed 50 (1 000
  programs, one failure, G1).
- **Failure classes**, every one shrunk with `psgen shrink`:
  - *T1, property tolerance (26 + 13).* `translate`, output differing
    in the last digit: `-45 rotate 0.80 0.89 scale 12.3 45.6 moveto
    currentpoint = =` prints `441.00003` against `441.0` once
    translated. Fixed in the relation and the model as above; test
    `translated_output_tolerates_rounding_of_readings`.
  - *G1, generator, userdict counted (6).* `currentdict length =` or
    `currentdict 0 exch { pop pop 1 add } forall =` — the save/restore
    variant's `/psgen_sr save def` is one more entry; the count also
    reached `rcurveto`, so the IR differed too. Fixed: `length` and the
    count refuse userdict; test `userdict_is_never_counted`.
  - *G2, generator panic (1, seed 113 index 252).* `/s0 save def pop
    0 0 getinterval s0 restore`: the sub-array kept its source's save
    level, rightly, with a composite id the block's snapshot lacked, and
    the model indexed past its composites. Fixed: after a save block,
    items whose composite the snapshot lacks become opaque; test
    `sub_arrays_of_old_storage_outlive_a_save_block`. `psgen check`
    silences panics, so a generator panic looks like an empty log: the
    round's driver counts summaries, which is how it was noticed.
  - *G3, generator, ill-typed block patterns (2).* `{ 304 arcto }
    stopped …` right after a well-typed `arcto` had left four readings:
    the wrong literal replaced the whole corner, `arcto` took the
    readings as its other operands, succeeded on a near-degenerate
    corner with tangent points at 10⁹, and the translation could not
    hold. Fixed: `Corner` and `ImageBlock` are never ill-typed; test
    `block_patterns_are_never_ill_typed`.
  - *I1, interpreter (1, seed 8 index 32),*
    `target/psgen/findings/arcto-quarter-sweep-split.ps`: `-45 rotate
    0.80 0.89 scale 1.50 1.16 scale 7 8 moveto 23 8 23 57 45 arcto` is
    emitted as one Bézier piece or two depending on the translation.
    `ps-graphics` counts pieces as ⌈sweep/90°⌉ and `arcto` recomputes
    the sweep from the current point read back through the CTM, so an
    exact quarter turn lands a rounding error either side of 90°. Both
    renderings are the same arc (PLRM3 §8.2 `arcto`). The grammar's
    corners are never right angles. Not fixed here.
  - *I2, interpreter precision (2, seeds 119/20 and 107/498),*
    `target/psgen/findings/arcto-acute-tangent-precision.ps`: `-45
    rotate 440 404 moveto 464 404 371 414 50 arcto` — an acute corner
    whose tangent points lie some 930 units out — moves its first
    tangent point by (0.007, 0.009) beyond the translation, where every
    other path operator moves exactly: the tangent distance
    r/tan(θ/2) is evaluated from a single-precision angle and the long
    lever amplifies it. The grammar keeps corners between 45° and 135°.
    Not fixed here.
  - *L1, limitation.* Under long scale chains (ten halvings) readings
    reach user-space errors of 0.4 and no tolerance is sound; the scale
    band above is the answer, recorded rather than fixed.
  - Not a failure but probed and recorded: a degenerate `arcto` (the
    current point at the first point, coincident or collinear points,
    radius 0) never raises here and degrades to a line; whether the
    reference raises `undefinedresult` there is for the oracle round to
    say (none of the generated programs is degenerate).
- **Seed files.** `core.seeds` gains `113 300` (G2's index 252 still
  generates the save block with the sub-array, since the fix consumes
  no random draws) and `graphics.seeds` gains `50 300` (G1); each with
  a comment. The xtask test pins eleven pairs per file. `cargo xtask
  fuzz-round` over 2 600 programs is green.
- **Private-tier round.** Profile `default`, 2 000 programs per
  profile (seed 300, ill-typed share 0) under `target/psgen/oracle/`,
  run as eight `difftest oracle` processes over four chunks per profile
  (214 s wall; the harness is sequential per process). Predicates for
  `psgen shrink` live under `target/psgen/oracle-round1/pred-*.sh`
  (`--profile default`, non-zero while the report still says "pixels
  differ" or "text differs"); the shrinks took seconds to a minute.
  - *First pass, the harness as part 1 left it:* `core` 2 000 pass,
    0 fail, output 508 same / 1 492 differs; `graphics` 1 589 pass,
    411 fail, output 678 same / 1 322 differs. Of the 411: 275 pixel
    differences, 174 text differences, and three pages whose media box
    came back rotated.
  - *(c) Comparison noise, fixed in the harness:* almost every output
    difference was the same single-precision real printed with
    different digits — ours the shortest round-trip form
    (`1.9098268`), the reference nine significant digits
    (`1.90982676`) or six through `cvs` (`-1.20185`); a whole-valued
    real at or above 10⁸ as digits here and in exponent form there.
    `canonical_number` now rounds a decimal fraction to six
    significant digits (single precision's guaranteed digits), unifies
    exponent forms and whole values of 10⁸ or more into a
    six-digit exponent form, and reaches numbers inside `[…]` and
    `(…)` tokens (`canonical_token`); `normalise_text` applies the same
    rule, since a shown `cvs` result differs exactly as a printed one.
    Tests in `numbers_are_canonical_and_output_is_normalised`.
  - *Final pass with those rules:* `core` **2 000 pass, 0 fail, output
    1 751 same / 249 differs**; `graphics` **1 590 pass, 410 fail,
    output 1 668 same / 332 differs**. The corpus under the same rules
    is unchanged: 145 files, 108 pass, 0 fail, 32 expected-divergence,
    1 divergence-closed, 4 skipped; output 110 same, 31 differs, the
    same two uncovered (`type42-charpath-bbox`, `pagedevice-merges`).
  - *The 581 output differences left, by root cause* (an offline
    tally over the artefacts with the harness's rules):
    128 sixth-digit rounding from a different evaluation order or
    intermediate precision (`7.5873` vs `7.58731`; c, unavoidable at
    six digits and recorded);
    105 `bitshift` of a negative integer shifted right — ours shifts
    the 32-bit pattern with zeros coming in (`-663 -7 bitshift` gives
    33554426), the reference sign-extends (−6);
    `target/psgen/findings/oracle-bitshift-negative-right.ps` (a or an
    expected divergence, PLRM3 §8.2 `bitshift`);
    86 `sin`/`cos` of a large angle — `389 -433 mul cos` is 0.7313537
    there and 0.73138183 here, since the degrees are converted to
    radians in single precision before reduction (a million degrees is
    wrong in the third digit); `oracle-cos-large-angle.ps` (a, PLRM3
    §8.2 `sin`/`cos`);
    80 `stringwidth` advances differing beyond the sixth digit (d,
    `resident-inventory`);
    49 readings through the CTM (`arcto` results, `currentpoint`,
    `pathbbox`) differing in the sixth digit (c, the precision item I2);
    about 20 a number glued to text by `print` with no space, where the
    token rule cannot reach (`opik24.799194` vs `opik24.7992`; c);
    one `==` of a procedure holding `=`, printed `--=--` here and `=`
    there, where the reference defines `=` as a procedure (b); two
    booleans, one from the `bitshift` chain, one not traced
    (`graphics-300-1542`).
  - *The 410 document failures, by root cause.* Attribution by
    re-running each failing file with statements deleted (`target/psgen/
    oracle-round1/attrib*`): 125 pass once colour statements go, 109
    once `clip`/`eoclip` go, 71 once image blocks go, 230 once all
    three go; 181 remain, of which 104 are text-only, 20 pixel, 56
    are variants the deletion left ill-formed (unattributed), and one
    a page count. Shrunk representatives and probes gave the causes:
    - **Stroke colour lost** (the largest class, at least 125):
      `0.5 setgray 4 setlinewidth 50 50 moveto 300 300 lineto stroke`
      — the content stream sets the fill colour only (`0.5 g`, or
      `/DeviceRGB cs … rg`) and strokes, so every stroke renders in the
      default black; black strokes agree, and a one-unit line differs
      in too few pixels for the 0.5 % limit, which is why the corpus
      never saw it and why repeated thin strokes in loops crossed it.
      `oracle-stroke-colour-lost.ps` (a, `remelt`'s content writer:
      no `G`/`RG`/`K`/`SCN`).
    - **`clip` with no current path** (109): ours emits `W n` without
      segments and keeps painting (5 629 dark pixels), the reference
      paints nothing after it; its text extractor likewise reports no
      text shown under such a clip. `oracle-clip-empty-path.ps` (a or
      b, PLRM3 §4.4.3, §8.2 `clip`; the grammar offers `clip` with no
      current point, which is where it comes from).
    - **Gray image after `setcmykcolor`** (71): the five-operand
      `image` is recorded in DeviceCMYK with its samples expanded to
      four components, where the reference keeps DeviceGray; the same
      image without the preceding colour agrees.
      `oracle-image-current-colour-space.ps` (a, PLRM3 §4.10.5).
    - **Invisible text extracted** (104 text-only, and 15 of the
      pixel+text): text shown off the page or under an empty clip is
      dropped from the reference's document but stays in ours, and the
      same extractor then reports it for ours only (`rrjdvkce` against
      nothing). Rasters agree. (c; not fixable in a few harness lines —
      it needs the geometry of each glyph run against the page and the
      clip; recorded.)
    - **Page rotated to its text** (3): the reference rotates a page to
      the dominant orientation of its text (`60 rotate … show`),
      reporting 842×596 for a 595×842 page (b; a converter feature the
      profile could switch off — noted in the vault).
    - The 20 remaining pixel cases all stroke or fill under
      `repeat`/`for` or `gsave` with colour set inside the body, which
      the line-deletion attribution could not remove without breaking
      the program; two of them shrink to the stroke-colour program.
      Probed and cleared: `arc` and `arcto` geometry, zero-width,
      dashed, and joined strokes, line caps, the 1×1 image, `show`
      without a font (ours `invalidfont`, the reference a default face
      — never generated).
- **Gates (4.2).** `cargo test --workspace`, clippy with no warnings,
  `cargo fmt`, `difftest run` 145/145 with the goldens byte-identical,
  `parse-survival` 145 files, `fuzz-round` 2 600 programs green,
  `lint-strings` clean, `openspec validate psgen-v0` valid; counts in
  the report.
- **Follow-ups, in priority order.**
  1. Stroke colour operators in the content writer (every non-black
     stroke renders black in the distilled document); a corpus file
     with a wide gray stroke and a raster expectation.
  2. `clip`/`eoclip` with no current path: decide (PLRM3 §4.4.3) and
     either emit an empty clip or record a divergence.
  3. The five-operand `image` in DeviceGray regardless of the current
     colour space.
  4. `sin`/`cos`/`atan` reduce the angle in degrees (or in double
     precision) before conversion.
  5. `bitshift` right of a negative: choose sign extension or record
     the zero fill as an expected divergence.
  6. `arcto`: tangent distance from the vectors in double precision;
     the piece count of a quarter-turn sweep snapped before ⌈sweep/90°⌉.
  7. The oracle text channel: skip glyph runs outside the page and
     under an empty clip when extracting ours, or compare text only
     where the rasters show it.
  8. Grammar: `yshow`/`xyshow`/`ashow`/`widthshow`, `imagemask`, colour
     images; the transform family under a device-space-aware relation.
