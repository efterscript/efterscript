# Change: CFF fonts — Type 2 charstrings, FontSet loading, Type1C embedding

## Why

The glyph engine reads Type 1 and TrueType programs; the third format
that reaches a PostScript interpreter, the Compact Font Format with
Type 2 charstrings, is missing. It arrives in two ways: as FontType 2
fonts loaded through the `FontSetInit` procedure set, and as the glyph
data of CID-keyed fonts, which is how every CJK job carries its
outlines. The composite-font change that handles the latter needs a
complete CFF engine underneath it, including the CID-keyed dictionary
structures, so this change builds the engine and the name-keyed path
end to end first: parse, measure, draw, `charpath`, subset, and embed as
`FontFile3`. The shapes fixed here — a CFF program in the engine's enum,
a CFF writer for subsets, a `FontSet` resource category, and the
procedure-set category that `CIDInit` will join — are what the composite
change lands on.

## What Changes

- **CFF parsing** (`ps-fonts`): header, the name, top-dictionary,
  string, and global-subroutine indexes; top and private dictionaries;
  charsets in all three formats; standard, expert, and custom encodings
  with supplements; local subroutines with the bias rule; a Type 2
  charstring interpreter (path operators, width parsing with default
  and nominal widths, hint operators and hint masks consumed, the four
  flex forms, the accent form of `endchar`, subroutine calls); CID-keyed
  fonts (registry-ordering-supplement, font dictionary array and
  select) parsed to per-glyph private data with glyph lookup by CID as
  well as by name. Outlines and advances land in the same `Glyph` type
  the other programs use.
- **Loading** (`ps-vm`): the `ProcSet` resource category with the
  `FontSetInit` procedure set; its `StartData` reads the declared number
  of binary bytes from the current file, parses the CFF, defines one
  FontType 2 font per name-keyed font in it (font matrix, bounding box,
  encoding, a `CharStrings` name-to-index dictionary, and the program
  cached on the interpreter), and defines the `FontSet` resource;
  `definefont` accepts FontType 2. `stringwidth`, the `show` family,
  and `charpath` work through the engine as for Type 1.
- **Subsetting and embedding** (`ps-fonts`, `remelt`): a CFF writer
  producing a name-keyed subset with the used charstrings, a matching
  charset, no encoding (the PDF encoding carries it), and local and
  global subroutines pruned by trace and renumbered with the bias
  accounted for; embedded as `FontFile3` with subtype `Type1C` under a
  `Type1` font dictionary, widths from the program, ToUnicode from glyph
  names.
- **Test fonts**: a CFF builder in the testing module (name-keyed and
  CID-keyed, a Type 2 charstring encoder) for corpus FontSet files; an
  optional check against a TeX Gyre OpenType file fetched into the
  target directory by the fetch tool when asked, skipped when absent.
- **Out of scope, with triggers**: CID-keyed fonts exposed to PostScript,
  CMaps, Type 0 fonts, and `CIDFontType0C` embedding (`composite-fonts`,
  next); OpenType fonts carrying CFF inside `sfnts` (when a job in the
  corpus uses one); Type 1 to CFF conversion on embedding (a policy);
  the CFF2 variable format (no PostScript path to it).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `font-programs`: ADDED requirements for CFF programs measuring and
  drawing, FontSet loading, and Type1C subsets.
- `text`: "Encodings and resource categories" gains the `ProcSet` and
  `FontSet` categories.
- `remelt`: ADDED requirement for `FontFile3` embedding.

## Impact

- Code: `crates/ps-fonts` (`cff` module: parser, Type 2 interpreter,
  writer, subsetter; `Program::Cff`; testing builder), `crates/ps-vm`
  (`ProcSet` and `FontSet` categories, `FontSetInit`/`StartData`,
  FontType 2 in `definefont` and the snapshot path), `crates/remelt`
  (`FontFile3`), `xtask` (optional test-asset extraction), corpus files
  under `corpus/unit/fonts/` with goldens.
- Dependencies: none new.
- Depends on `font-programs` and `font-asset-trim` (archived).
