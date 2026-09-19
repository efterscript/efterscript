## MODIFIED Requirements

### Requirement: Command-line distillation

`efterscript pdf <in.ps> [<out.pdf>]` SHALL distil the input to the named
file, defaulting to the input's path with a `.pdf` extension; `-` as the
output SHALL write the PDF to standard output and move the program's own
output to standard error. Every mode of the tool SHALL bound the run by
an execution budget, by default a documented allowance of objects
executed, settable with `--budget <n>` and removable with
`--budget unlimited`; a non-integer or zero value SHALL be a usage error.
The exit status SHALL be 0 when the job ended normally, 1 when it ended
in an error, and 3 when it spent its budget, in every case after the PDF
is written; a run ended by the budget SHALL say so on standard error.

#### Scenario: Default output name

- **WHEN** `efterscript pdf corpus/unit/graphics/stroked-line.ps` runs in a
  writable directory copy
- **THEN** `stroked-line.pdf` exists beside the input and the exit status is 0

#### Scenario: Corpus sidecar goldens

- **GIVEN** a corpus file with a sidecar under `corpus/golden/pdf/`
- **WHEN** `difftest run` executes
- **THEN** the distilled bytes are compared exactly with the golden, a
  mismatch is reported as a diff of the text lines, and `--update-pdf`
  rewrites the golden with uncompressed streams and a generator comment

#### Scenario: A runaway document ends

- **WHEN** `efterscript run` is given a program that loops forever
- **THEN** the run ends by itself, standard error reports that the budget was spent, and the exit status is 3

#### Scenario: The budget is adjustable

- **WHEN** the same program runs with `--budget 1000` and then with `--budget unlimited` under an external timeout
- **THEN** the first ends promptly with exit status 3 and the second is still running when the timeout ends it
