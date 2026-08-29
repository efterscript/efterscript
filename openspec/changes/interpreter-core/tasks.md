# Tasks: interpreter-core

## 1. State and loop

- [x] 1.1 `Interp` with the three stacks, limits, `Io`, `Capabilities`; push helpers raising the overflow errors
- [x] 1.2 `Frame` enum and the execution loop (`Proc`, `Object`, `Source`, `Loop`, `Stopped`, `Marker`)
- [x] 1.3 Name lookup through the dictionary stack; literal vs executable handling per object type
- [x] 1.4 `Interp::run` with run boundary and `Outcome`; `NeedMore` → `Suspended`

## 2. Operator table

- [x] 2.1 `OpEntry`, signature prologue, table macro; `systemdict` population
- [x] 2.2 Standard dictionaries and constants (`userdict`, `globaldict`, `errordict`, `$error`, `statusdict` stub, `languagelevel`)

## 3. Operators

- [x] 3.1 Stack: `pop dup exch copy index roll clear count mark cleartomark counttomark`
- [x] 3.2 Arithmetic and math with overflow-to-real; relational, boolean, bitwise
- [x] 3.3 Dictionary: `dict begin end def load store known where get put undef length maxlength currentdict countdictstack dictstack cleardictstack`
- [ ] 3.4 Array/packed array/string: `array [ ] astore aload getinterval putinterval length get put copy forall string search anchorsearch packedarray setpacking currentpacking`
- [ ] 3.5 Type/attribute/conversion: `type cvlit cvx xcheck executeonly readonly noaccess rcheck wcheck cvi cvr cvn cvs cvrs`
- [x] 3.6 Control: `exec if ifelse for repeat loop exit stop stopped countexecstack execstack quit`
- [ ] 3.7 VM: `save restore setglobal currentglobal vmstatus gcheck`
- [ ] 3.8 Files/output: `file closefile read write readline readstring readhexstring writestring token currentfile flush print = == pstack stack`; `eexec` registered as unimplemented
- [x] 3.9 `bind`, `handleerror`, default `errordict` entries

## 4. Tooling

- [ ] 4.1 `efterscript run <file>`: executes with capture streams to host stdout/stderr, exit code from outcome
- [ ] 4.2 `difftest run`: expectation headers, per-file pass/fail, summary
- [ ] 4.3 Add expectation headers to every existing corpus file; all pass

## 5. Verification

- [ ] 5.1 Unit tests per spec scenario; corpus mirrors under `corpus/unit/interp/`
- [x] 5.2 Property test: random control nesting keeps host recursion depth constant
- [x] 5.3 Deep-recursion and stack-limit tests
