// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The outline assets behind the resident set: Liberation TrueType
//! files for the Helvetica, Times, and Courier families and TeX Gyre
//! Type 1 files for the twenty-one LaserWriter faces, embedded behind
//! the `resident-outlines` feature and parsed once per thread on first
//! use. A glyph is found by name — a Type 1 charstring, a TrueType
//! `post` name, else the name's Unicode value through the `(3,1)` cmap —
//! and answered in the 1000-unit space the metrics use, with the advance
//! taken from the face's metrics rather than from the asset.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::glyph_list::unicode;
use crate::outline::Glyph;
use crate::program::{FontError, Program, ProgramKind};
use crate::resident::ResidentFace;
use crate::truetype::TrueTypeProgram;
use crate::type1::parse_file;

/// Which shipped file a face's outlines come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutlineAsset {
    /// A Liberation TrueType face, by file stem.
    Liberation(&'static str),
    /// A TeX Gyre Type 1 face, by file stem.
    TexGyre(&'static str),
}

impl OutlineAsset {
    pub fn kind(self) -> ProgramKind {
        match self {
            OutlineAsset::Liberation(_) => ProgramKind::TrueType,
            OutlineAsset::TexGyre(_) => ProgramKind::Type1,
        }
    }

    /// The file's name under the crate's `data/outlines` directory.
    pub fn file_name(self) -> String {
        match self {
            OutlineAsset::Liberation(stem) => format!("liberation/{stem}.ttf"),
            OutlineAsset::TexGyre(stem) => format!("tex-gyre/{stem}.pfb"),
        }
    }

    #[cfg(feature = "resident-outlines")]
    fn bytes(self) -> &'static [u8] {
        macro_rules! liberation {
            ($stem:literal) => {
                include_bytes!(concat!("../data/outlines/liberation/", $stem, ".ttf"))
            };
        }
        macro_rules! tex_gyre {
            ($stem:literal) => {
                include_bytes!(concat!("../data/outlines/tex-gyre/", $stem, ".pfb"))
            };
        }
        match self {
            OutlineAsset::Liberation("LiberationSans-Regular") => {
                liberation!("LiberationSans-Regular")
            }
            OutlineAsset::Liberation("LiberationSans-Bold") => liberation!("LiberationSans-Bold"),
            OutlineAsset::Liberation("LiberationSans-Italic") => {
                liberation!("LiberationSans-Italic")
            }
            OutlineAsset::Liberation("LiberationSans-BoldItalic") => {
                liberation!("LiberationSans-BoldItalic")
            }
            OutlineAsset::Liberation("LiberationSerif-Regular") => {
                liberation!("LiberationSerif-Regular")
            }
            OutlineAsset::Liberation("LiberationSerif-Bold") => liberation!("LiberationSerif-Bold"),
            OutlineAsset::Liberation("LiberationSerif-Italic") => {
                liberation!("LiberationSerif-Italic")
            }
            OutlineAsset::Liberation("LiberationSerif-BoldItalic") => {
                liberation!("LiberationSerif-BoldItalic")
            }
            OutlineAsset::Liberation("LiberationMono-Regular") => {
                liberation!("LiberationMono-Regular")
            }
            OutlineAsset::Liberation("LiberationMono-Bold") => liberation!("LiberationMono-Bold"),
            OutlineAsset::Liberation("LiberationMono-Italic") => {
                liberation!("LiberationMono-Italic")
            }
            OutlineAsset::Liberation("LiberationMono-BoldItalic") => {
                liberation!("LiberationMono-BoldItalic")
            }
            OutlineAsset::TexGyre("qagr") => tex_gyre!("qagr"),
            OutlineAsset::TexGyre("qagri") => tex_gyre!("qagri"),
            OutlineAsset::TexGyre("qagb") => tex_gyre!("qagb"),
            OutlineAsset::TexGyre("qagbi") => tex_gyre!("qagbi"),
            OutlineAsset::TexGyre("qbkr") => tex_gyre!("qbkr"),
            OutlineAsset::TexGyre("qbkri") => tex_gyre!("qbkri"),
            OutlineAsset::TexGyre("qbkb") => tex_gyre!("qbkb"),
            OutlineAsset::TexGyre("qbkbi") => tex_gyre!("qbkbi"),
            OutlineAsset::TexGyre("qcsr") => tex_gyre!("qcsr"),
            OutlineAsset::TexGyre("qcsri") => tex_gyre!("qcsri"),
            OutlineAsset::TexGyre("qcsb") => tex_gyre!("qcsb"),
            OutlineAsset::TexGyre("qcsbi") => tex_gyre!("qcsbi"),
            OutlineAsset::TexGyre("qplr") => tex_gyre!("qplr"),
            OutlineAsset::TexGyre("qplri") => tex_gyre!("qplri"),
            OutlineAsset::TexGyre("qplb") => tex_gyre!("qplb"),
            OutlineAsset::TexGyre("qplbi") => tex_gyre!("qplbi"),
            OutlineAsset::TexGyre("qzcmi") => tex_gyre!("qzcmi"),
            OutlineAsset::TexGyre("qhvcr") => tex_gyre!("qhvcr"),
            OutlineAsset::TexGyre("qhvcri") => tex_gyre!("qhvcri"),
            OutlineAsset::TexGyre("qhvcb") => tex_gyre!("qhvcb"),
            OutlineAsset::TexGyre("qhvcbi") => tex_gyre!("qhvcbi"),
            _ => unreachable!("every asset the face table names is embedded"),
        }
    }
}

/// A face's parsed asset: the program (shared with the PDF writer for
/// the extras), the name the file gives itself, and the glyphs looked
/// up so far, scaled to the 1000-unit space of the metrics.
pub struct ResidentOutlines {
    program: Rc<Program>,
    font_name: Vec<u8>,
    /// The `(3,1)` cmap of a TrueType asset, for names the `post` table
    /// lacks.
    unicode_map: Option<BTreeMap<u32, u16>>,
    /// Font units to thousandths of the em.
    scale: f32,
    cache: RefCell<HashMap<Vec<u8>, Option<Rc<Glyph>>>>,
}

impl ResidentOutlines {
    /// Reads `bytes` as the file `asset` names.
    pub fn parse(asset: OutlineAsset, bytes: &[u8]) -> Result<Self, FontError> {
        let (program, font_name, unicode_map, scale) = match asset {
            OutlineAsset::TexGyre(_) => {
                let font = parse_file(bytes)?;
                (Program::Type1(font.program), font.font_name, None, 1.0)
            }
            OutlineAsset::Liberation(stem) => {
                let program = TrueTypeProgram::parse(bytes.to_vec())?;
                let cmap = program.cmap(3, 1)?;
                let scale = 1000.0 / f32::from(program.units_per_em());
                (
                    Program::TrueType(program),
                    stem.as_bytes().to_vec(),
                    cmap,
                    scale,
                )
            }
        };
        Ok(ResidentOutlines {
            program: Rc::new(program),
            font_name,
            unicode_map,
            scale,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// The parsed program, in its own glyph space.
    pub fn program(&self) -> &Rc<Program> {
        &self.program
    }

    /// The name the asset gives itself (`TeXGyrePagella-Regular`,
    /// `LiberationSans-Regular`).
    pub fn font_name(&self) -> &[u8] {
        &self.font_name
    }

    /// The glyph index a TrueType asset draws for `name`: its `post`
    /// name, else the name's single Unicode value through the cmap;
    /// `None` for a name the asset lacks and for a Type 1 asset.
    pub fn glyph_index(&self, name: &[u8]) -> Option<u16> {
        let Program::TrueType(program) = &*self.program else {
            return None;
        };
        if let Some(gid) = program.gid(name) {
            return Some(gid);
        }
        let chars = unicode(name)?;
        let [c] = chars.as_slice() else {
            return None;
        };
        self.unicode_map.as_ref()?.get(&u32::from(*c)).copied()
    }

    /// The outline of `name` in thousandths of the em with `advance` as
    /// its width; `None` for a glyph the asset lacks.
    pub fn glyph(&self, name: &[u8], advance: f32) -> Result<Option<Rc<Glyph>>, FontError> {
        if let Some(found) = self.cache.borrow().get(name) {
            return Ok(found.clone());
        }
        let outline = match &*self.program {
            Program::Type1(program) => program.glyph(name)?.map(|g| g.outline.clone()),
            Program::Cff(program) => program.glyph(name)?.map(|g| g.outline.clone()),
            Program::Type1Cid(_) => None,
            Program::TrueType(program) => match self.glyph_index(name) {
                Some(gid) => Some(program.outline(gid)?),
                None => None,
            },
        };
        let scale = self.scale;
        let glyph = outline.map(|outline| {
            Rc::new(Glyph {
                advance: (advance, 0.0),
                outline: outline.map(|x, y| (x * scale, y * scale)),
            })
        });
        self.cache.borrow_mut().insert(name.to_vec(), glyph.clone());
        Ok(glyph)
    }

    /// The advance of the asset's `.notdef` glyph in thousandths of the
    /// em: glyph 0 of a TrueType asset, the charstring of that name in
    /// a Type 1 one; `None` when the asset has neither.
    pub fn notdef_advance(&self) -> Option<f32> {
        let advance = match &*self.program {
            Program::TrueType(program) => program.glyph_by_index(0).ok()?.advance.0,
            program => program.glyph(b".notdef").ok()??.advance.0,
        };
        Some(advance * self.scale)
    }
}

thread_local! {
    static OUTLINES: RefCell<[Option<Rc<ResidentOutlines>>; ResidentFace::COUNT]> =
        const { RefCell::new([const { None }; ResidentFace::COUNT]) };
}

impl ResidentFace {
    /// The face's parsed outline asset, parsed once per thread; `None`
    /// for a face without one and for a build without the feature.
    pub fn outlines(self) -> Option<Rc<ResidentOutlines>> {
        let asset = self.outline_asset()?;
        if let Some(found) = OUTLINES.with(|o| o.borrow()[self.index()].clone()) {
            return Some(found);
        }
        let parsed = Rc::new(parse_asset(asset)?);
        OUTLINES.with(|o| o.borrow_mut()[self.index()] = Some(parsed.clone()));
        Some(parsed)
    }

    /// Whether `charpath` can outline this face in this build.
    pub fn has_outlines(self) -> bool {
        crate::has_resident_outlines() && self.outline_asset().is_some()
    }

    /// The outline of the glyph named `name` in thousandths of the em,
    /// its advance the metrics' width (0 when the metrics lack the name);
    /// `Ok(None)` when the asset has no such glyph or there is no asset.
    pub fn outline(self, name: &[u8]) -> Result<Option<Rc<Glyph>>, FontError> {
        let Some(outlines) = self.outlines() else {
            return Ok(None);
        };
        let advance = std::str::from_utf8(name)
            .ok()
            .and_then(|n| self.width(n))
            .map_or(0.0, f32::from);
        outlines.glyph(name, advance)
    }

    /// The advance of a code whose encoding name the face lacks, in
    /// thousandths of the em: the metrics' `.notdef` width, else the
    /// outline asset's `.notdef` advance, else 0.
    pub fn notdef_width(self) -> f32 {
        if let Some(width) = self.width(".notdef") {
            return f32::from(width);
        }
        self.outlines()
            .and_then(|outlines| outlines.notdef_advance())
            .unwrap_or(0.0)
    }
}

#[cfg(feature = "resident-outlines")]
fn parse_asset(asset: OutlineAsset) -> Option<ResidentOutlines> {
    // A shipped file that does not parse is an intake fault the asset
    // tests catch; at run time the face simply has no outlines.
    ResidentOutlines::parse(asset, asset.bytes()).ok()
}

#[cfg(not(feature = "resident-outlines"))]
fn parse_asset(_: OutlineAsset) -> Option<ResidentOutlines> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_know_their_kind_and_file() {
        assert_eq!(
            OutlineAsset::Liberation("LiberationSans-Regular").kind(),
            ProgramKind::TrueType
        );
        assert_eq!(OutlineAsset::TexGyre("qplr").kind(), ProgramKind::Type1);
        assert_eq!(
            OutlineAsset::Liberation("LiberationSans-Regular").file_name(),
            "liberation/LiberationSans-Regular.ttf"
        );
        assert_eq!(
            OutlineAsset::TexGyre("qplr").file_name(),
            "tex-gyre/qplr.pfb"
        );
        assert!(ResidentFace::Symbol.outlines().is_none());
        assert!(!ResidentFace::ZapfDingbats.has_outlines());
        assert_eq!(ResidentFace::Symbol.outline(b"alpha").unwrap(), None);
    }

    #[test]
    fn notdef_widths_come_from_the_table_then_the_asset_then_nothing() {
        assert_eq!(ResidentFace::PalatinoRoman.notdef_width(), 500.0);
        assert_eq!(ResidentFace::BookmanLight.notdef_width(), 280.0);
        assert_eq!(ResidentFace::Symbol.notdef_width(), 0.0);
        assert_eq!(ResidentFace::ZapfDingbats.notdef_width(), 0.0);
        for face in [
            ResidentFace::Helvetica,
            ResidentFace::TimesRoman,
            ResidentFace::Courier,
        ] {
            assert_eq!(face.width(".notdef"), None);
            let width = face.notdef_width();
            if crate::has_resident_outlines() {
                let asset = face.outlines().unwrap().notdef_advance().unwrap();
                assert_eq!(width, asset);
                assert!(width > 590.0 && width < 780.0, "{width}");
            } else {
                assert_eq!(width, 0.0);
            }
        }
    }

    #[cfg(feature = "resident-outlines")]
    #[test]
    fn a_synthesised_asset_scales_and_falls_back_through_unicode() {
        use crate::testing::{TrueTypeFont, Type1Font, rectangle};
        use crate::truetype::write::{Table, assemble, cmap};
        let tt = TrueTypeFont::new(2048)
            .glyph(
                "A",
                1024,
                vec![vec![(0, 0, true), (2048, 0, true), (2048, 2048, true)]],
            )
            .glyph(
                "uni20AC",
                1024,
                vec![vec![(0, 0, true), (1024, 0, true), (1024, 1024, true)]],
            );
        // The builder writes a (3,0) cmap; the fallback reads (3,1).
        let unicode: BTreeMap<u32, u16> = [(0x41, 1), (0x20AC, 2)].into_iter().collect();
        let mut tables: Vec<Table> = tt
            .tables()
            .into_iter()
            .filter(|t| &t.tag != b"cmap")
            .collect();
        tables.push(Table::new(b"cmap", cmap(&[(3, 1, &unicode)])));
        let outlines =
            ResidentOutlines::parse(OutlineAsset::Liberation("Syn"), &assemble(tables)).unwrap();
        assert_eq!(outlines.font_name(), b"Syn");
        let a = outlines.glyph(b"A", 722.0).unwrap().unwrap();
        assert_eq!(a.advance, (722.0, 0.0));
        assert_eq!(a.outline.control_box(), Some([0.0, 0.0, 1000.0, 1000.0]));
        // `Euro` is not a post name here; its Unicode value is mapped.
        let euro = outlines.glyph(b"Euro", 556.0).unwrap().unwrap();
        assert_eq!(euro.outline.control_box(), Some([0.0, 0.0, 500.0, 500.0]));
        assert_eq!(outlines.glyph(b"nosuchglyph", 0.0).unwrap(), None);
        assert_eq!(outlines.glyph(b"f_i", 0.0).unwrap(), None);
        assert_eq!(outlines.glyph_index(b"A"), Some(1));
        assert_eq!(outlines.glyph_index(b"Euro"), Some(2));
        assert_eq!(outlines.glyph_index(b"nosuchglyph"), None);
        assert!(Rc::ptr_eq(&outlines.glyph(b"A", 1.0).unwrap().unwrap(), &a));
        // Glyph 0's advance, in thousandths of the 2048-unit em.
        let notdef = outlines.notdef_advance().unwrap();
        assert!((0.0..=1000.0).contains(&notdef), "{notdef}");

        let t1 = Type1Font::new("SynOne").glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0));
        let outlines = ResidentOutlines::parse(OutlineAsset::TexGyre("syn"), &t1.pfb()).unwrap();
        assert_eq!(outlines.font_name(), b"SynOne");
        assert_eq!(outlines.program().kind(), ProgramKind::Type1);
        let a = outlines.glyph(b"a", 500.0).unwrap().unwrap();
        assert_eq!(a.advance, (500.0, 0.0));
        assert_eq!(a.outline.control_box(), Some([50.0, 0.0, 550.0, 500.0]));
        assert_eq!(outlines.glyph(b"b", 0.0).unwrap(), None);
        assert_eq!(
            outlines.glyph_index(b"a"),
            None,
            "a Type 1 asset has no glyph indices"
        );
        assert_eq!(
            outlines.notdef_advance().is_some(),
            outlines.program().has_glyph(b".notdef")
        );
    }

    #[cfg(feature = "resident-outlines")]
    #[test]
    fn faces_answer_from_their_assets_once_per_thread() {
        let h = ResidentFace::Helvetica.outline(b"H").unwrap().unwrap();
        assert_eq!(h.advance, (722.0, 0.0));
        // Metric-compatible in advance; the outline's width is within
        // twenty units (two at size 100) of the Helvetica AFM's H box.
        let [llx, _, urx, _] = h.outline.control_box().unwrap();
        let [allx, _, aurx, _] = crate::resident::StdFont::Helvetica
            .metrics()
            .glyph("H")
            .unwrap()
            .bbox;
        assert!(((urx - llx) - (aurx - allx)).abs() < 20.0, "{llx} {urx}");
        assert!(ResidentFace::Helvetica.has_outlines());
        let first = ResidentFace::Helvetica.outlines().unwrap();
        let second = ResidentFace::Helvetica.outlines().unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        let p = ResidentFace::PalatinoRoman.outline(b"a").unwrap().unwrap();
        assert_eq!(p.advance, (500.0, 0.0));
        assert_eq!(
            ResidentFace::PalatinoRoman.outlines().unwrap().font_name(),
            b"TeXGyrePagella-Regular"
        );
        assert_eq!(
            ResidentFace::TimesRoman.outline(b"nosuchglyph").unwrap(),
            None
        );
        // The Liberation and TeX Gyre `.notdef` advances, scaled to the
        // 1000-unit space.
        let serif = ResidentFace::TimesRoman.outlines().unwrap();
        assert!((serif.notdef_advance().unwrap() - 777.832).abs() < 0.01);
        assert_eq!(
            ResidentFace::PalatinoRoman
                .outlines()
                .unwrap()
                .notdef_advance(),
            Some(500.0)
        );
    }
}
