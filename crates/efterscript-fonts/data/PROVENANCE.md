# Provenance — crates/efterscript-fonts/data

The `core14/` metrics and `glyphlist.txt` were promoted from the project's
private vault unmodified; their SHA-256 sums are checked against the
vault's `SHA256SUMS` by `tests/provenance.rs` whenever `EFTERSCRIPT_HELLBOX`
names a checkout. The `outlines/` assets and the `cmap/` resources never
passed through the vault: they are freely redistributable releases fetched
straight from upstream by `cargo xtask fetch-fonts`, which verifies each
archive's checksum, extracts exactly the files listed here, derives the
metric tables beside the TeX Gyre programs, and audits everything against
this note (`--check`). Every
file below is hashed by `tests/provenance.rs` on every run. Nothing here
is a build dependency on the vault or the network: the files are embedded
with `include_str!`/`include_bytes!` and the crate builds without either.

Table paths are relative to this directory, except those under
`LICENSES/`, which name the repository's licence-text copies.

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

## `outlines/liberation/` — Liberation 2.1.5 TrueType outlines and their licence

- Files: `LiberationSans-{Regular,Bold,Italic,BoldItalic}.ttf`,
  `LiberationSerif-{Regular,Bold,Italic,BoldItalic}.ttf`,
  `LiberationMono-{Regular,Bold,Italic,BoldItalic}.ttf`, and `LICENSE`.
- Source: release 2.1.5 of `https://github.com/liberationfonts/liberation-fonts`,
  the TrueType tarball
  `https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz`
  (SHA-256 `7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0`).
  Retrieved 2026-09-02 by `cargo xtask fetch-fonts`; the twelve files and
  the licence are byte-identical to the tarball's members.
- Grant: SIL Open Font License 1.1 (SPDX `OFL-1.1`), with `Liberation`
  as a Reserved Font Name; the `LICENSE` file beside the fonts is the
  upstream copy — the licence text preceded by the copyright statements
  of Google (digitized data, 2010) and Red Hat (2012) — and the plain
  licence text is `LICENSES/OFL-1.1.txt`. The OFL permits use,
  bundling, redistribution, and embedding of the unmodified fonts with
  the licence file alongside; the fonts are not sold by themselves and
  are not modified here, so no Reserved Font Name question arises.
- Use: outlines for the Helvetica (Sans), Times (Serif), and Courier
  (Mono) families of the resident set, addressed by glyph name through
  the `post` table with a Unicode fallback through the `cmap`. Advances
  come from the Core 14 metrics, never from these files.

| File | SHA-256 |
|---|---|
| `outlines/liberation/LICENSE` | `93fed46019c38bbe566b479d22148e2e8a1e85ada614accb0211c37b2c61c19b` |
| `outlines/liberation/LiberationMono-Bold.ttf` | `bd62a0672d0b9b6710b01df434c80ad54fa5f0835207eb7b17b7a761463067bb` |
| `outlines/liberation/LiberationMono-BoldItalic.ttf` | `79451f3c09fe25116098853b7a2ca6e2436220ccc11af022979adbcf195be130` |
| `outlines/liberation/LiberationMono-Italic.ttf` | `605c01c711b44480a7508d349dfbf3264e81fa43d69e61cfa7d10b86e764c4d1` |
| `outlines/liberation/LiberationMono-Regular.ttf` | `f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1` |
| `outlines/liberation/LiberationSans-Bold.ttf` | `788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173` |
| `outlines/liberation/LiberationSans-BoldItalic.ttf` | `698da70fc191cc5f33ad4d6d3fe830fe4624b898ea2e3169955928b7c491f1ee` |
| `outlines/liberation/LiberationSans-Italic.ttf` | `e5bae5c4cde31f22142753855f4f8fb86da6ff39955ed3c0a11248b0d16948b0` |
| `outlines/liberation/LiberationSans-Regular.ttf` | `76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8` |
| `outlines/liberation/LiberationSerif-Bold.ttf` | `d754ba427cfe0bca54ae052384baa8f842da5bd6550ad4da024ac441e7a7d5ce` |
| `outlines/liberation/LiberationSerif-BoldItalic.ttf` | `f17db8af71e24d2066b587546021d4f0b296be389512b658dec3c09affeb11a7` |
| `outlines/liberation/LiberationSerif-Italic.ttf` | `0e3dea9f8d613e006ccfa62201f33e265d19167bd0907725c3e145368b04fc2e` |
| `outlines/liberation/LiberationSerif-Regular.ttf` | `058ea80864aef09a23f45cbec2bb5400bc3dfbdea01c3f10538a21fcb497fb74` |

## `outlines/tex-gyre/` — TeX Gyre Type 1 outlines, licence, manifests

- Files: `<face>.pfb` for the twenty-one faces
  `qag{r,ri,b,bi}` (Adventor), `qbk{r,ri,b,bi}` (Bonum),
  `qcs{r,ri,b,bi}` (Schola), `qpl{r,ri,b,bi}` (Pagella), `qzcmi`
  (Chorus), `qhvc{r,ri,b,bi}` (Heros Condensed); `GUST-FONT-LICENSE.txt`;
  and `MANIFEST-TeX-Gyre-{Adventor,Bonum,Chorus,Heros,Pagella,Schola}.txt`.
  The `.metrics` table beside each program is derived data, listed in
  the next section.
- Source: the TeX Gyre collection as distributed by CTAN,
  `https://mirrors.ctan.org/fonts/tex-gyre.zip` (SHA-256
  `1773c470f9e388e087b68e3426e115af2cd236845a7e05ceb25b2a503409a7a3`),
  members `tex-gyre/type1/*.pfb` and `tex-gyre/doc/*.txt`. The
  collection carries no single version: each
  family states its own in its font headers and manifest — Adventor
  2.501, Pagella 2.501, Bonum 2.004, Schola 2.005, Chorus 2.003, Heros
  2.004 — and the provenance test checks each manifest against its
  fonts. Retrieved 2026-09-02 by `cargo xtask fetch-fonts`; byte-identical
  to the archive's members.
- Grant: the GUST Font License, which distributes the fonts under the
  LaTeX Project Public License 1.3c or later (SPDX `LPPL-1.3c`; the text
  is `LICENSES/LPPL-1.3c.txt`) with one added clause: a request — not a
  condition — that derived works rename the fonts listed in the
  accompanying manifests. The fonts ship here unmodified under their own
  names, with the licence and the manifests of the six families beside
  them, so the request does not apply. Copyright 2007–2018 for the TeX
  Gyre extensions by B. Jackowski, J.M. Nowacki, et al. (GUST e-foundry),
  as each file's `Notice` states.
- Use: outlines for the twenty-one LaserWriter faces of the resident set
  (AvantGarde, Bookman, NewCenturySchlbk, Palatino, ZapfChancery,
  Helvetica-Narrow). The `.pfb` programs are parsed by
  `type1::parse_file`, embedded as subsets in the PDF output under their
  TeX Gyre names (with the subroutines no kept glyph reaches replaced by
  stubs), and are the source of the derived metric tables below, which
  are the width authority for those faces.

| File | SHA-256 |
|---|---|
| `outlines/tex-gyre/GUST-FONT-LICENSE.txt` | `2bd69affc3da00715116f713f57eab9707e96daf3562ad0215987b15b9c16f73` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Adventor.txt` | `22ed76ceb3942e4e19b7c4ef30673d0fc17d44f9e3174347f4951cd52a4b30ce` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Bonum.txt` | `a3f8dc563dddc96267c39e58b35492a2321de179144c36d954b0e5bde8f84365` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Chorus.txt` | `f605b07ca7d34a5beff322c4177db1dbad02163b77b24d99c0d33124f102f633` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Heros.txt` | `3263a067e409258be34027de883e618cc2c76c70135897835f65f3c569dec5d1` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Pagella.txt` | `6ac4bc1448a1d71a3a7ad5fd13566f00855b37ca4db5b20451c88806ac3f5242` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Schola.txt` | `db7d5f23bc3e684e81ed3f95e65b888b9e184f7002cdf6eb772ad8d7879ad5f6` |
| `outlines/tex-gyre/qagb.pfb` | `39c2d6300620d8189915351d0e07276f3bbf95f9f346d1dbf46b8d2045aaec69` |
| `outlines/tex-gyre/qagbi.pfb` | `c79838ffb851220844f4ddbed0d80a4c370f1a32643ba0479c7e6b89a40cde0a` |
| `outlines/tex-gyre/qagr.pfb` | `3ec36824b2b8d1e97657318b6b335e9189286d0995b1278d369dc088545fdd4d` |
| `outlines/tex-gyre/qagri.pfb` | `5b5d46d12373ab0da8121efd4e476a25178eb645b8d8e35988e3637e06888aa2` |
| `outlines/tex-gyre/qbkb.pfb` | `53b5cf1527e3578cb04a6fb5a6ef2aa53fe8bb4fc16f6348303118144f3df29f` |
| `outlines/tex-gyre/qbkbi.pfb` | `cae45a6941167a9179b6036e01befaf9270ce8cee525f4cef2dc99e4a0498dde` |
| `outlines/tex-gyre/qbkr.pfb` | `ba3698cd56b3434239d415d2a4f4aa0fbafc83738d6887736cfc992f592fe326` |
| `outlines/tex-gyre/qbkri.pfb` | `610db05006d413f0b321114d781594f6f952b4d505d9f2988ef6b536d4a728c6` |
| `outlines/tex-gyre/qcsb.pfb` | `b7c14c4c57aba88e4a3f25fe29aa65c8fb7b6ec68550043b5fd12e47974dcf9b` |
| `outlines/tex-gyre/qcsbi.pfb` | `f769e241ec293960caf05b2507887c32ca3674c0196949d0c032d3e833b629e0` |
| `outlines/tex-gyre/qcsr.pfb` | `c246b0bb606a1a5bb72df1967c550c107ffb4fbfecb005c855773880f7af7d74` |
| `outlines/tex-gyre/qcsri.pfb` | `4014a5b92b6004bdd4d8dc677031c763f1410bca07601e7aa0d7cc305e85056a` |
| `outlines/tex-gyre/qhvcb.pfb` | `757a354a8fbfaeae65bfa14f6dc46e23e0462fa986ed7307bc7f1394ac972a8d` |
| `outlines/tex-gyre/qhvcbi.pfb` | `a4599bb73b1100184169adf12c729563d78883d12f7629b530542e26c0d58553` |
| `outlines/tex-gyre/qhvcr.pfb` | `3733c6ce08b3278df4ea179d40faa7802eefe6b70ceb40fcb345b4e50f1613ae` |
| `outlines/tex-gyre/qhvcri.pfb` | `1814794ac4c10e3d2128fb9aa3e560ca7568bf273bd87b6f3002b0d80c76d3e9` |
| `outlines/tex-gyre/qplb.pfb` | `0d8fafc4afb8ae7118cec522cd7c0d3016051feeb246c20b3a52d8b39a297aa1` |
| `outlines/tex-gyre/qplbi.pfb` | `78d47ab576f36bce872987fac87aa34a3db32fbbc111e3a553937ae3a323635e` |
| `outlines/tex-gyre/qplr.pfb` | `8d422717983976fc42ddaf796ab001d0ac611dd5f161df5ffd32a1a30288e9d8` |
| `outlines/tex-gyre/qplri.pfb` | `3d815ef31b565c14fc620886f033fdb900e7b026afba64fd3dc91b543f0aa622` |
| `outlines/tex-gyre/qzcmi.pfb` | `c0dd65cb3380efc9b5b0ae8db9b17280bcfcb20f3582994f49eb6e68e4d20327` |

## `outlines/tex-gyre/*.metrics` — metric tables derived from the programs

- Files: `<face>.metrics` for the twenty-one faces above, one beside
  each `.pfb`.
- Source: generated by `cargo xtask fetch-fonts` from the `.pfb` of the
  same stem (its SHA-256 is in the table above): the program's
  `FontBBox`, its `Encoding`, and every charstring's advance as the
  charstring interpreter computes it — format `metrics/1`, read and
  written by `efterscript_fonts::metrics`. Thirty-four advances in ten faces are
  computed with `div` and are not integral; the tables carry them
  rounded to the nearest unit (`tie`, `undertie`, `undertieinverted` in
  the Bonum, Schola, and Heros Condensed italics; `hyphen.alt`,
  `hyphendbl.alt` in the Schola and Heros Condensed faces), which is the
  value the upstream `.afm` files gave them. Every width, box, and
  encoding entry equals the upstream `.afm` files, which are therefore
  no longer shipped. Generated 2026-09-03.
- Grant: the tables are the project's own derived data, under the
  repository's MIT licence, with the SPDX header in their comment lines;
  they carry widths, not outlines. `REUSE.toml` names the upstream files
  by extension so the tables keep their own licence.
- Use: glyph widths, font bounding box, and file encoding for the
  twenty-one LaserWriter faces, included whether or not the outlines
  are. `tests/resident_assets.rs` regenerates every table from its
  program and requires byte identity; `cargo xtask fetch-fonts --check`
  audits the same way.

| File | SHA-256 | Derived from |
|---|---|---|
| `outlines/tex-gyre/qagb.metrics` | `52e766ccd0cf4be228cf508a4216a14ebba5233c6d632e25fb1292e2f4752dd1` | `qagb.pfb` |
| `outlines/tex-gyre/qagbi.metrics` | `5b889cc1d0a103003f882e1262a71803e04d8622810cdf6d929073ed327b43e2` | `qagbi.pfb` |
| `outlines/tex-gyre/qagr.metrics` | `8f3687e7211764d9fb650c5402dd32aa1054e4a98365bfc3dd0787268b8ababe` | `qagr.pfb` |
| `outlines/tex-gyre/qagri.metrics` | `2b06149ab0172aadf95508942c9ca5b2c556548a9f5939be6f37a2e4cd404da8` | `qagri.pfb` |
| `outlines/tex-gyre/qbkb.metrics` | `a3761501147b293876a2f1055261cc0525b5b945a6a0b03659bd64492e229263` | `qbkb.pfb` |
| `outlines/tex-gyre/qbkbi.metrics` | `f81861883d7016e180909417334b2c6bb3ac0046d478e501128ad7a78403ce71` | `qbkbi.pfb` |
| `outlines/tex-gyre/qbkr.metrics` | `cdaec53bac5daeaaf5cc8f43af6a827b3cf9832e58b91edd97b1848674fc6799` | `qbkr.pfb` |
| `outlines/tex-gyre/qbkri.metrics` | `0ff6c0e1faa1db0d9940a25367b80f2f1f0d01cdb4556b28189fb611ef5bc41c` | `qbkri.pfb` |
| `outlines/tex-gyre/qcsb.metrics` | `dfa375fac58b2a01b186bce466129a3cf613f113c436dd4b73d3ad30f6aae767` | `qcsb.pfb` |
| `outlines/tex-gyre/qcsbi.metrics` | `11dba4a75d043fef3731c6e53a2d771d7b73a9bbf383e278d07dce490589ddd6` | `qcsbi.pfb` |
| `outlines/tex-gyre/qcsr.metrics` | `92b3ce4acc3a0e79d17a54fcb7d47cfb58e14c7f8ded5db490a23ac47bc62eea` | `qcsr.pfb` |
| `outlines/tex-gyre/qcsri.metrics` | `fc0890038c5660b266ca14897a527355d440766a30ac4c84ace74afb215aab42` | `qcsri.pfb` |
| `outlines/tex-gyre/qhvcb.metrics` | `b0a4d9405b77af98ad7bc9c1158f5c4a56089ad7abca221ab9fd8e0fd1608c26` | `qhvcb.pfb` |
| `outlines/tex-gyre/qhvcbi.metrics` | `aa2244ffeb0969d9ffefe032086bfe69f238fafefde522770db53180716bacfb` | `qhvcbi.pfb` |
| `outlines/tex-gyre/qhvcr.metrics` | `2ea3503a1efa590a4da7630d3e14f1ecd84cedcfe1e5c24199e9581cb95cede5` | `qhvcr.pfb` |
| `outlines/tex-gyre/qhvcri.metrics` | `29e6c01b6dd7631cb25e76dfb97ae7c9fa3c6b6db99b2133e2dffd479dc41c13` | `qhvcri.pfb` |
| `outlines/tex-gyre/qplb.metrics` | `0d0c8c918b6b9748f5dbc2f3c03e8b8ab501b4fa4ed7160c708f48a15c47fba9` | `qplb.pfb` |
| `outlines/tex-gyre/qplbi.metrics` | `48a52a91ee873c523eabc3adbdc12dc2639c43c90a567c5ac9ef046bacd4ea2a` | `qplbi.pfb` |
| `outlines/tex-gyre/qplr.metrics` | `313ce72e3d76088e953f9b2a4f5f238c2495456062d715943e6ef3c1659ab772` | `qplr.pfb` |
| `outlines/tex-gyre/qplri.metrics` | `0c1f52f7496bf2db0643478a980a3a46f9140162ec135bf3342668a49cc6a9f2` | `qplri.pfb` |
| `outlines/tex-gyre/qzcmi.metrics` | `212eb2b6c266e7498fb0d4878a15599d31ef2fab519d4dbcbcc1e704fdcb9694` | `qzcmi.pfb` |

## `cmap/` — the Identity CMap resources and their licence

- Files: `Identity-H`, `Identity-V`, and `LICENSE.md`.
- Source: `https://github.com/adobe-type-tools/cmap-resources` at commit
  `f5cf3bca7fdfeaceb77aa82847e974f2306c20b4` (the `master` head when
  retrieved), the files `Adobe-Identity-0/CMap/Identity-H`,
  `Adobe-Identity-0/CMap/Identity-V`, and `LICENSE.md`. Retrieved
  2026-09-03 by `cargo xtask fetch-fonts` from the raw files at that
  commit; byte-identical to upstream. Nothing else is taken from that
  repository: the Unicode CMaps of the CJK orderings are a separate
  asset decision.
- Grant: Adobe's BSD-style licence (SPDX `BSD-3-Clause`; the plain text
  is `LICENSES/BSD-3-Clause.txt`), stated in `LICENSE.md` and repeated
  in each CMap file's `%%Copyright:` header: redistribution in source
  and binary forms, with or without modification, provided the
  copyright notice, the conditions, and the disclaimer are retained (in
  the file for source form, in accompanying documentation for binary
  form) and Adobe's name is not used to endorse derived products. The
  files ship unmodified with their headers and the licence file beside
  them, which satisfies the source condition; a binary embedding this
  crate should carry the notice in its documentation.
- Use: the predefined `Identity-H` and `Identity-V` CMap resources,
  embedded with `include_str!` and run through the interpreter's
  `CIDInit` operators on first `findresource`.

| File | SHA-256 |
|---|---|
| `cmap/Identity-H` | `a06aff40c5e4393829d572b3771e5cafcf450ec4fa6ef3df5ae4024f16fc6efa` |
| `cmap/Identity-V` | `c03430489caf73dc71c723d9ae0413a31132f6ac9f16149d9a1d19e5d913af0f` |
| `cmap/LICENSE.md` | `742665db9c8e1bc72603c6d319ca3e90b83bd6d95202f0cd4ef11068a07a9c29` |

## `LICENSES/` — the licence texts for REUSE

- Files: `LICENSES/OFL-1.1.txt` and `LICENSES/BSD-3-Clause.txt` from the
  SPDX licence list,
  `https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/OFL-1.1.txt`
  and `…/text/BSD-3-Clause.txt`; `LICENSES/LPPL-1.3c.txt` from the LaTeX
  project, `https://www.latex-project.org/lppl/lppl-1-3c.txt`. The first
  two retrieved 2026-09-02, the BSD text 2026-09-03, all by `cargo xtask
  fetch-fonts`, unmodified.
- The repository's `REUSE.toml` annotates the three asset directories
  with these identifiers; the files sit at the repository root, not
  here.

| File | SHA-256 |
|---|---|
| `LICENSES/OFL-1.1.txt` | `8eea8287e5876b539670cadb82e99f9a7afddec6f6730811be1daf25d2e9bcfd` |
| `LICENSES/LPPL-1.3c.txt` | `3d262cdf34dafa6955f703c634a8c238ec44109bc8dd6ef34fb7aa54809f7e66` |
| `LICENSES/BSD-3-Clause.txt` | `5a93d5831e1297ab10fe643e1a631e83be392896da14ee2951285a79012df69d` |
