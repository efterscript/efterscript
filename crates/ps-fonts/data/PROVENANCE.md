# Provenance — crates/ps-fonts/data

The `core14/` metrics and `glyphlist.txt` were promoted from the project's
private vault unmodified; their SHA-256 sums are checked against the
vault's `SHA256SUMS` by `tests/provenance.rs` whenever `EFTERSCRIPT_HELLBOX`
names a checkout. The `outlines/` assets never passed through the vault:
they are freely redistributable releases fetched straight from upstream by
`cargo xtask fetch-fonts`, which verifies each archive's checksum, extracts
exactly the files listed here, and audits them against this note
(`--check`). Every file below is hashed by `tests/provenance.rs` on every
run. Nothing here is a build dependency on the vault or the network: the
files are embedded with `include_str!`/`include_bytes!` and the crate
builds without either.

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

## `outlines/tex-gyre/` — TeX Gyre Type 1 outlines, metrics, licence, manifests

- Files: `<face>.pfb` and `<face>.afm` for the twenty-one faces
  `qag{r,ri,b,bi}` (Adventor), `qbk{r,ri,b,bi}` (Bonum),
  `qcs{r,ri,b,bi}` (Schola), `qpl{r,ri,b,bi}` (Pagella), `qzcmi`
  (Chorus), `qhvc{r,ri,b,bi}` (Heros Condensed); `GUST-FONT-LICENSE.txt`;
  and `MANIFEST-TeX-Gyre-{Adventor,Bonum,Chorus,Heros,Pagella,Schola}.txt`.
- Source: the TeX Gyre collection as distributed by CTAN,
  `https://mirrors.ctan.org/fonts/tex-gyre.zip` (SHA-256
  `1773c470f9e388e087b68e3426e115af2cd236845a7e05ceb25b2a503409a7a3`),
  members `tex-gyre/type1/*.pfb`, `tex-gyre/afm/*.afm`, and
  `tex-gyre/doc/*.txt`. The collection carries no single version: each
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
- Use: outlines and metrics for the twenty-one LaserWriter faces of the
  resident set (AvantGarde, Bookman, NewCenturySchlbk, Palatino,
  ZapfChancery, Helvetica-Narrow). The `.afm` widths are the width
  authority for those faces; the `.pfb` programs are parsed by
  `type1::parse_file` and embedded as subsets in the PDF output under
  their TeX Gyre names.

| File | SHA-256 |
|---|---|
| `outlines/tex-gyre/GUST-FONT-LICENSE.txt` | `2bd69affc3da00715116f713f57eab9707e96daf3562ad0215987b15b9c16f73` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Adventor.txt` | `22ed76ceb3942e4e19b7c4ef30673d0fc17d44f9e3174347f4951cd52a4b30ce` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Bonum.txt` | `a3f8dc563dddc96267c39e58b35492a2321de179144c36d954b0e5bde8f84365` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Chorus.txt` | `f605b07ca7d34a5beff322c4177db1dbad02163b77b24d99c0d33124f102f633` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Heros.txt` | `3263a067e409258be34027de883e618cc2c76c70135897835f65f3c569dec5d1` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Pagella.txt` | `6ac4bc1448a1d71a3a7ad5fd13566f00855b37ca4db5b20451c88806ac3f5242` |
| `outlines/tex-gyre/MANIFEST-TeX-Gyre-Schola.txt` | `db7d5f23bc3e684e81ed3f95e65b888b9e184f7002cdf6eb772ad8d7879ad5f6` |
| `outlines/tex-gyre/qagb.afm` | `24846d8b63a478971f4dbc36e0461a0f131d21d828fd7e1d5cd327dc5a717cd0` |
| `outlines/tex-gyre/qagb.pfb` | `39c2d6300620d8189915351d0e07276f3bbf95f9f346d1dbf46b8d2045aaec69` |
| `outlines/tex-gyre/qagbi.afm` | `4863a874b631c7eacf04ca1410a44efe3e2849c6533256bfda4f173ed8c8db43` |
| `outlines/tex-gyre/qagbi.pfb` | `c79838ffb851220844f4ddbed0d80a4c370f1a32643ba0479c7e6b89a40cde0a` |
| `outlines/tex-gyre/qagr.afm` | `835fbfb312a485a8125b9712b135b2f089c0a948dc900385f9efa35045e5dd44` |
| `outlines/tex-gyre/qagr.pfb` | `3ec36824b2b8d1e97657318b6b335e9189286d0995b1278d369dc088545fdd4d` |
| `outlines/tex-gyre/qagri.afm` | `da0a83f2fa5bed6fdb7c7a42ca851a21abc12cad00ad71ea0f08cfa91ffe1f70` |
| `outlines/tex-gyre/qagri.pfb` | `5b5d46d12373ab0da8121efd4e476a25178eb645b8d8e35988e3637e06888aa2` |
| `outlines/tex-gyre/qbkb.afm` | `94cc7af5c95b0a255e87aa6f5f734d230a38d1ce5acad8c0c3391814c4473e9c` |
| `outlines/tex-gyre/qbkb.pfb` | `53b5cf1527e3578cb04a6fb5a6ef2aa53fe8bb4fc16f6348303118144f3df29f` |
| `outlines/tex-gyre/qbkbi.afm` | `365eb1c9f2316f8f854564276b0702af701e3eaa0404447c2a2d78778aa4b9a6` |
| `outlines/tex-gyre/qbkbi.pfb` | `cae45a6941167a9179b6036e01befaf9270ce8cee525f4cef2dc99e4a0498dde` |
| `outlines/tex-gyre/qbkr.afm` | `6e62d462c5975f0931e77b62a8c23f500dc147b3b16d33e52ade14a4318ee438` |
| `outlines/tex-gyre/qbkr.pfb` | `ba3698cd56b3434239d415d2a4f4aa0fbafc83738d6887736cfc992f592fe326` |
| `outlines/tex-gyre/qbkri.afm` | `8a3be3939f4ca993c7d5a83c222a012793783cd43234a4ac9dca9233a9884968` |
| `outlines/tex-gyre/qbkri.pfb` | `610db05006d413f0b321114d781594f6f952b4d505d9f2988ef6b536d4a728c6` |
| `outlines/tex-gyre/qcsb.afm` | `fc3f9d443526e1460abad0d78d2c0bc01b5a6d66f257325cb490de54efa843b6` |
| `outlines/tex-gyre/qcsb.pfb` | `b7c14c4c57aba88e4a3f25fe29aa65c8fb7b6ec68550043b5fd12e47974dcf9b` |
| `outlines/tex-gyre/qcsbi.afm` | `db15ff0b3aa519f1a8239fb7cfa4fc7be6a7ebd44dfee6feead03bb92bc6fea6` |
| `outlines/tex-gyre/qcsbi.pfb` | `f769e241ec293960caf05b2507887c32ca3674c0196949d0c032d3e833b629e0` |
| `outlines/tex-gyre/qcsr.afm` | `9118a311df0bbb368ff677e08f07ee1c24a7d8bdd24c154b1c8bebe6abcf0ad8` |
| `outlines/tex-gyre/qcsr.pfb` | `c246b0bb606a1a5bb72df1967c550c107ffb4fbfecb005c855773880f7af7d74` |
| `outlines/tex-gyre/qcsri.afm` | `edda57b310bb4f124f6147a75cee223cff6c72a0ffcc2c2a7da197174769be09` |
| `outlines/tex-gyre/qcsri.pfb` | `4014a5b92b6004bdd4d8dc677031c763f1410bca07601e7aa0d7cc305e85056a` |
| `outlines/tex-gyre/qhvcb.afm` | `d49bba56b61b289bc4f69e4f7ba4a7d8d9c4315e64f33f5b4a966427394a5788` |
| `outlines/tex-gyre/qhvcb.pfb` | `757a354a8fbfaeae65bfa14f6dc46e23e0462fa986ed7307bc7f1394ac972a8d` |
| `outlines/tex-gyre/qhvcbi.afm` | `bcbde7dc87058c5ba92631f0e1a0d73fd4a9e2bd1fc703fde5fa9d8a5e458e58` |
| `outlines/tex-gyre/qhvcbi.pfb` | `a4599bb73b1100184169adf12c729563d78883d12f7629b530542e26c0d58553` |
| `outlines/tex-gyre/qhvcr.afm` | `cf125e65d8b2a6c06854602bcec8767fdc9f208ea967cd763c6482ad8708b444` |
| `outlines/tex-gyre/qhvcr.pfb` | `3733c6ce08b3278df4ea179d40faa7802eefe6b70ceb40fcb345b4e50f1613ae` |
| `outlines/tex-gyre/qhvcri.afm` | `a5c6ce208609e36ad741871b9a60a5ebc88c320e37ccf2390b61f0a1b9c39dff` |
| `outlines/tex-gyre/qhvcri.pfb` | `1814794ac4c10e3d2128fb9aa3e560ca7568bf273bd87b6f3002b0d80c76d3e9` |
| `outlines/tex-gyre/qplb.afm` | `9ad0d982c76c1062c24ef58bce34f048558ca35e679e7687872ad61e3be3df03` |
| `outlines/tex-gyre/qplb.pfb` | `0d8fafc4afb8ae7118cec522cd7c0d3016051feeb246c20b3a52d8b39a297aa1` |
| `outlines/tex-gyre/qplbi.afm` | `22bc2fffd4600855d1aa115fda89dbe82277abb4a995a6235a3486aa66dfc7c1` |
| `outlines/tex-gyre/qplbi.pfb` | `78d47ab576f36bce872987fac87aa34a3db32fbbc111e3a553937ae3a323635e` |
| `outlines/tex-gyre/qplr.afm` | `74f75c9dc72bceec700dc1d800ea360d5d06a495cf1bb2c96b065097110f6d0f` |
| `outlines/tex-gyre/qplr.pfb` | `8d422717983976fc42ddaf796ab001d0ac611dd5f161df5ffd32a1a30288e9d8` |
| `outlines/tex-gyre/qplri.afm` | `bc31b48f7ed0009ecce1e892a3c510c6f9aa5dc7281f87aad08e56f3cb59d27d` |
| `outlines/tex-gyre/qplri.pfb` | `3d815ef31b565c14fc620886f033fdb900e7b026afba64fd3dc91b543f0aa622` |
| `outlines/tex-gyre/qzcmi.afm` | `bea53a868327b464bb9f48f38016544e3e92d3a7e2fef3f0072235f9a210f394` |
| `outlines/tex-gyre/qzcmi.pfb` | `c0dd65cb3380efc9b5b0ae8db9b17280bcfcb20f3582994f49eb6e68e4d20327` |

## `LICENSES/` — the licence texts for REUSE

- Files: `LICENSES/OFL-1.1.txt` from the SPDX licence list,
  `https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/OFL-1.1.txt`;
  `LICENSES/LPPL-1.3c.txt` from the LaTeX project,
  `https://www.latex-project.org/lppl/lppl-1-3c.txt`. Both retrieved
  2026-09-02 by `cargo xtask fetch-fonts`, unmodified.
- The repository's `REUSE.toml` annotates the two asset directories with
  these identifiers; the files sit at the repository root, not here.

| File | SHA-256 |
|---|---|
| `LICENSES/OFL-1.1.txt` | `8eea8287e5876b539670cadb82e99f9a7afddec6f6730811be1daf25d2e9bcfd` |
| `LICENSES/LPPL-1.3c.txt` | `3d262cdf34dafa6955f703c634a8c238ec44109bc8dd6ef34fb7aa54809f7e66` |
