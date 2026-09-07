# distillation-policy

## Purpose

The distillation parameters: the operators that set and read them, the
keys honoured with their meaning, tolerance for the rest, and the
precedence between the embedder's options and the job's settings.

## ADDED Requirements

### Requirement: Parameter operators

`setdistillerparams` SHALL merge a dictionary into the current
parameters, type-checking recognised keys and accepting unknown ones;
`currentdistillerparams` SHALL return the current dictionary. Both
SHALL be defined with or without a graphics backend. Each change SHALL
be handed to the backend as values.

#### Scenario: Round trip

- **GIVEN** `<< /CompressPages false /Foo 1 >> setdistillerparams
  currentdistillerparams /CompressPages get = currentdistillerparams
  /Foo get =`
- **THEN** the output is `false` then `1`

#### Scenario: Ill-typed key

- **GIVEN** `<< /CompressPages 3 >> setdistillerparams`
- **THEN** the error is `typecheck`

### Requirement: Honoured parameters

The writer SHALL honour `CompressPages`, `EmbedAllFonts`,
`SubsetFonts`, `CompatibilityLevel` (1.3 to 1.7, header only),
`DownsampleColorImages`, `DownsampleGrayImages`,
`DownsampleMonoImages` with `ColorImageResolution`,
`GrayImageResolution`, `MonoImageResolution`, and the three
`…ImageDownsampleType` keys (`/Average`, `/Subsample`; `/Bicubic` as
average), and `ColorConversionStrategy /LeaveColorUnchanged`; any other
documented key SHALL be accepted and reported as not honoured, and any
other value of a honoured key SHALL be reported.

#### Scenario: Compression off

- **GIVEN** `<< /CompressPages false >> setdistillerparams` before a
  page
- **THEN** the page's content stream has no filter

#### Scenario: Not honoured, reported

- **GIVEN** `<< /AutoRotatePages /All >> setdistillerparams`
- **THEN** the job runs and the report lists the key as not honoured

### Requirement: Precedence

Options given by the embedder or the command line SHALL be the initial
parameters; a job's `setdistillerparams` SHALL override them except for
keys the embedder locks; the report SHALL state the parameters in
effect at finish.

#### Scenario: Locked key

- **GIVEN** the command line `--no-compress --lock CompressPages` and a
  job setting `CompressPages true`
- **THEN** the content streams are unfiltered and the report shows the
  job's value as overridden
