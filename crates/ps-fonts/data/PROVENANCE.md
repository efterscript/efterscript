# Provenance — crates/ps-fonts/data

Every file in this directory was promoted from the project's private vault
unmodified; the SHA-256 sums below are checked against the vault's
`SHA256SUMS` by `tests/provenance.rs` whenever `EFTERSCRIPT_HELLBOX` names a
checkout. Nothing here is a build dependency on the vault: the files are
embedded with `include_str!` and the crate builds without it.

## `core14/` — Core 14 AFM metric files and their licence

- Files: `Courier.afm`, `Courier-Bold.afm`, `Courier-Oblique.afm`,
  `Courier-BoldOblique.afm`, `Helvetica.afm`, `Helvetica-Bold.afm`,
  `Helvetica-Oblique.afm`, `Helvetica-BoldOblique.afm`, `Times-Roman.afm`,
  `Times-Bold.afm`, `Times-Italic.afm`, `Times-BoldItalic.afm`,
  `Symbol.afm`, `ZapfDingbats.afm`, and `LICENSE`.
- Source: `https://github.com/tecnickcom/tc-font-core14-afms` at commit
  `0675784d24b28a55c607cad6b74596ce19ce333c`, a mirror of the
  `Core14_AFMs.zip` archive Adobe once published. Retrieved into the vault
  2026-08-29; promoted here unmodified (CRLF line endings and all).
- Grant: the `LICENSE` file beside the AFMs is Adobe's own notice. It
  permits use, copying, and distribution of the fourteen AFM files for any
  purpose, with or without modification, on four conditions: every
  copyright notice is retained, the AFM files are not distributed without
  the licence file, any modification to the licence or to an AFM file is
  prominently noted in the modified file, and the licence paragraph itself
  is not modified. This directory satisfies all four: the files are
  byte-identical to the upstream archive, the licence sits beside them, and
  nothing is modified. Adobe accepts no support obligation for the files.
- Use: glyph widths, font bounding boxes, and the built-in encodings of
  Symbol and ZapfDingbats for the resident standard fonts. The kerning
  sections are present in the files but not parsed.

| File | SHA-256 |
|---|---|
| `core14/Courier-Bold.afm` | `ad0150d4bedcc8877742bf94251fcec13e348dd599d4603f679d92027d1e6e99` |
| `core14/Courier-BoldOblique.afm` | `cb82e69ef5f6d421e8f404fe00bb0d993425aae79725331d0ebf847e94e97e92` |
| `core14/Courier-Oblique.afm` | `b27103b2a2ef6030c110626597e2ab47bb8279075a039ab9170facb0aa1f70e1` |
| `core14/Courier.afm` | `521e0d7c7521efd4be78a5a9c5398e4c67d0771e396115b0346bc4ef74ada53d` |
| `core14/Helvetica-Bold.afm` | `b880d96baf56d0cc059f258f60b4d764ef49b555ab9db294b959c0016dee41f2` |
| `core14/Helvetica-BoldOblique.afm` | `69984a35ca26973a39f261cf83e0d367ea2e6517c590b0b030d4d4a219d9c269` |
| `core14/Helvetica-Oblique.afm` | `b4609b71b660a392ac09df35060271a876c2ce66617dd83bf826f742bb9d9721` |
| `core14/Helvetica.afm` | `da33f1870474c8e68bfe3e2353ff107ab6c6eea1f9836ce2aaf1e1a07b17982f` |
| `core14/LICENSE` | `8618df77fef76491116b941327bfeddf4976ab7ca98f24fe97385761ba8e09f9` |
| `core14/Symbol.afm` | `3d2128a820375a10de9bc8bf6cfb15ded482c01ca0f95cc0b3277f37ec8bde66` |
| `core14/Times-Bold.afm` | `b4a000ed85cb22c6cdd985aa0fd3f6f78ed5079b7c6860dc4f0234e0d0e3c522` |
| `core14/Times-BoldItalic.afm` | `93c4744ba955215de02c4aae0b777133442ada2f7ab5a2af30a040b792b3c55d` |
| `core14/Times-Italic.afm` | `ed37fa2e6a67b5b17dfd47f36fc7e90df32891a4408860dbc8d4cbbe9959e242` |
| `core14/Times-Roman.afm` | `768e1cabea085d489a63da3e80b96bc5abf0ec98d3073c9b4d6ba76e7bccba64` |
| `core14/ZapfDingbats.afm` | `a32565c90afd1b57a7008fc567b78d95cf1c22adff5e086094d666d88b039859` |

## `glyphlist.txt` — the Adobe Glyph List

- Source: `https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master/glyphlist.txt`
  (table version 2.0, dated 2002-09-20 in its header). Retrieved into the
  vault 2026-08-29; promoted here unmodified, header included.
- Grant: the file's own header carries Adobe's BSD-style licence:
  redistribution in source and binary forms, with or without modification,
  is permitted provided the copyright notice, the conditions, and the
  disclaimer are retained (in the file for source form, in accompanying
  documentation for binary form) and Adobe's name is not used to endorse
  derived products. The header is kept verbatim, which satisfies the source
  condition; a binary embedding this crate should carry the notice in its
  documentation.
- Use: glyph-name to Unicode mapping for ToUnicode CMaps and text
  extraction.

| File | SHA-256 |
|---|---|
| `glyphlist.txt` | `a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789` |
