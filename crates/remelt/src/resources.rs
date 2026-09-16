// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The objects a page's resources become. Colour spaces outside the device
//! families are written into the page's `ColorSpace` resource dictionary
//! as `/CSn` (n the IR's `SpaceRef` index); a Separation or DeviceN tint
//! transform is a Type 4 function stream (ISO 32000-1 §7.10.5) whose body
//! is the captured PostScript source verbatim, unchecked against the
//! calculator subset; a calibrated space is the array form of ISO
//! 32000-1 §8.6.5 (`[/CalGray dict]`, `[/CalRGB dict]`, `[/Lab dict]`)
//! whose dictionary carries only the entries that differ from their
//! defaults beside the required `WhitePoint`; a pattern space with an
//! underlying space is the array form `[/Pattern base]`, while one
//! without is selected by its family name like a device space and
//! listed nowhere. Images are image
//! XObjects `/Imn` (§8.9.5) with their samples in the Flate container,
//! reduced first when the downsampling parameters say so (see
//! `downsample`); an image that arrived as a DCT stream keeps its bytes
//! verbatim under the `DCTDecode` filter.
//!
//! A shading resource is a shading object `/Shn` (§8.7.4.3): a
//! dictionary for types 1 to 3, a Flate stream for the mesh types 4 to
//! 7 holding the packed vertex or patch data as the IR carries it (the
//! layout of §8.7.4.5 is the one the language prescribes, so the bytes
//! go out unchanged); entries at their default values are left out. Its
//! colour space is written inline — a family name or the array form,
//! which a shading dictionary accepts as an image dictionary does — so
//! the page's `ColorSpace` resources are not touched. Its function is
//! written as a separate object per function of §7.10: a type 0 as a
//! Flate stream of the samples in their packed layout, a type 2 or 3 as
//! a dictionary, the parts of a stitching function as objects of their
//! own; a shading given an array of functions refers to them as an
//! array. Shading objects are written before the fonts, so a glyph
//! procedure can paint one.
//!
//! A pattern resource is a tiling pattern stream `/Pn` (§8.7.3) carrying
//! its paint and tiling types, box, steps, and matrix, or a shading
//! pattern dictionary `/Pn` (§8.7.4.1) carrying its shading's object and
//! its matrix; a form resource is a form XObject `/Fmn` (§8.10) with its
//! box and the identity matrix. A tiling cell and a form each hold their
//! content rendered through the content writer over the page's
//! resources and a `Resources` dictionary of what that content names
//! directly (§7.8.3) — a form placed inside it or a pattern it paints
//! with has a dictionary of its own. Their ids are allocated before the
//! fonts are written, so a glyph procedure can name them, and the
//! streams are written after, so they can name the fonts.
//!
//! An overprint setting anywhere on the page — its own operations, a
//! cell, a body, or a glyph procedure — has an extended graphics state
//! dictionary `/GSn` (ISO 32000-1 §8.4.5) carrying `OP` and `op` with
//! the value, one per distinct value used, listed under `ExtGState` by
//! the page and by every content that selects it.
//!
//! The objects a page needs are written before the page itself, so a
//! `Resources` dictionary only ever refers to objects already in the file.
//! Fonts are `/Fn` (n the `FontIndex`), written once per document by
//! `fonts`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use pdf_out::{DictBuilder, Document, Filter, Ref, Val};
use ps_graphics::{
    FontIndex, FormIndex, Image, ImageRef, Page, PatternIndex, PatternSpec, ShadingIndex, SpaceRef,
};
use ps_vm::{
    Bounds, Encoded, FunctionSpec, ImageSpec, Matrix, ShadingKind, ShadingSpec, SpaceSpec,
};

use crate::content::{self, Recode};
use crate::downsample::{self, Outcome, Tally};
use crate::fonts::{FontTable, Refs, put_bounds, write_fonts};
use crate::params::Params;

pub(crate) fn space_name(space: SpaceRef) -> String {
    format!("CS{}", space.0)
}

pub(crate) fn image_name(image: ImageRef) -> String {
    format!("Im{}", image.0)
}

pub(crate) fn font_name(font: FontIndex) -> String {
    format!("F{}", font.0)
}

pub(crate) fn pattern_name(pattern: PatternIndex) -> String {
    format!("P{}", pattern.0)
}

pub(crate) fn form_name(form: FormIndex) -> String {
    format!("Fm{}", form.0)
}

pub(crate) fn shading_name(shading: ShadingIndex) -> String {
    format!("Sh{}", shading.0)
}

/// The extended graphics state selecting overprint `on`: `GS0` turns
/// it off, `GS1` on.
pub(crate) fn ext_gstate_name(on: bool) -> String {
    format!("GS{}", u8::from(on))
}

/// A colour space with its function streams already written.
enum Form {
    Device(&'static str),
    Separation {
        name: Vec<u8>,
        alternate: Box<Form>,
        function: Ref,
    },
    DeviceN {
        names: Vec<Vec<u8>>,
        alternate: Box<Form>,
        function: Ref,
    },
    Indexed {
        base: Box<Form>,
        hival: u16,
        lookup: Vec<u8>,
    },
    CalGray {
        white: [f32; 3],
        black: [f32; 3],
        gamma: f32,
    },
    CalRGB {
        white: [f32; 3],
        black: [f32; 3],
        gamma: [f32; 3],
        matrix: [f32; 9],
    },
    Lab {
        white: [f32; 3],
        black: [f32; 3],
        range: [f32; 4],
    },
    Pattern {
        base: Option<Box<Form>>,
    },
}

const IDENTITY_3X3: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const DEFAULT_LAB_RANGE: [f32; 4] = [-100.0, 100.0, -100.0, 100.0];

/// Writes `key` as an array of `values`.
fn put_reals(d: &mut DictBuilder<'_>, key: &str, values: &[f32]) {
    d.key(key).array(|a| {
        for &value in values {
            a.real(value);
        }
    });
}

/// The entries every calibrated space's dictionary opens with: the white
/// point, and the black point when it is not the default zero.
fn put_points(d: &mut DictBuilder<'_>, white: &[f32; 3], black: &[f32; 3]) {
    put_reals(d, "WhitePoint", white);
    if *black != [0.0; 3] {
        put_reals(d, "BlackPoint", black);
    }
}

impl Form {
    /// Whether the space is selected by its family name, which the
    /// content writer does directly, so it is never listed as a resource.
    fn is_direct(&self) -> bool {
        matches!(self, Form::Device(_) | Form::Pattern { base: None })
    }

    /// The colour-space value: a family name or the array form.
    fn put(&self, v: Val<'_>) {
        match self {
            Form::Device(family) => v.name(family),
            Form::Separation {
                name,
                alternate,
                function,
            } => v.array(|a| {
                a.name("Separation");
                a.item().name_bytes(name);
                alternate.put(a.item());
                a.reference(*function);
            }),
            Form::DeviceN {
                names,
                alternate,
                function,
            } => v.array(|a| {
                a.name("DeviceN");
                a.array(|names_array| {
                    for name in names {
                        names_array.item().name_bytes(name);
                    }
                });
                alternate.put(a.item());
                a.reference(*function);
            }),
            Form::Indexed {
                base,
                hival,
                lookup,
            } => v.array(|a| {
                a.name("Indexed");
                base.put(a.item());
                a.int(i64::from(*hival));
                a.hex_string(lookup);
            }),
            Form::CalGray {
                white,
                black,
                gamma,
            } => v.array(|a| {
                a.name("CalGray");
                a.dict(|d| {
                    put_points(d, white, black);
                    if *gamma != 1.0 {
                        d.key("Gamma").real(*gamma);
                    }
                });
            }),
            Form::CalRGB {
                white,
                black,
                gamma,
                matrix,
            } => v.array(|a| {
                a.name("CalRGB");
                a.dict(|d| {
                    put_points(d, white, black);
                    if *gamma != [1.0; 3] {
                        put_reals(d, "Gamma", gamma);
                    }
                    if *matrix != IDENTITY_3X3 {
                        put_reals(d, "Matrix", matrix);
                    }
                });
            }),
            Form::Lab {
                white,
                black,
                range,
            } => v.array(|a| {
                a.name("Lab");
                a.dict(|d| {
                    put_points(d, white, black);
                    if *range != DEFAULT_LAB_RANGE {
                        put_reals(d, "Range", range);
                    }
                });
            }),
            Form::Pattern { base: None } => v.name("Pattern"),
            Form::Pattern { base: Some(base) } => v.array(|a| {
                a.name("Pattern");
                base.put(a.item());
            }),
        }
    }
}

fn write_function<W: Write>(
    doc: &mut Document<W>,
    source: &[u8],
    inputs: usize,
    outputs: usize,
    filter: Filter,
) -> Result<Ref, pdf_out::Error> {
    let function = doc.alloc();
    doc.write_stream(function, filter, source, |d| {
        d.key("FunctionType").int(4);
        d.key("Domain").array(|a| {
            for _ in 0..inputs {
                a.int(0).int(1);
            }
        });
        d.key("Range").array(|a| {
            for _ in 0..outputs {
                a.int(0).int(1);
            }
        });
    })?;
    Ok(function)
}

fn write_space<W: Write>(
    doc: &mut Document<W>,
    spec: &SpaceSpec,
    filter: Filter,
) -> Result<Form, pdf_out::Error> {
    Ok(match spec {
        SpaceSpec::DeviceGray | SpaceSpec::DeviceRGB | SpaceSpec::DeviceCMYK => {
            Form::Device(spec.family())
        }
        SpaceSpec::Separation {
            name,
            alternate,
            tint_source,
        } => {
            let alternate_form = write_space(doc, alternate, filter)?;
            let function = write_function(
                doc,
                tint_source,
                spec.components(),
                alternate.components(),
                filter,
            )?;
            Form::Separation {
                name: name.clone(),
                alternate: Box::new(alternate_form),
                function,
            }
        }
        SpaceSpec::DeviceN {
            names,
            alternate,
            tint_source,
        } => {
            let alternate_form = write_space(doc, alternate, filter)?;
            let function = write_function(
                doc,
                tint_source,
                spec.components(),
                alternate.components(),
                filter,
            )?;
            Form::DeviceN {
                names: names.clone(),
                alternate: Box::new(alternate_form),
                function,
            }
        }
        SpaceSpec::Indexed {
            base,
            hival,
            lookup,
        } => Form::Indexed {
            base: Box::new(write_space(doc, base, filter)?),
            hival: *hival,
            lookup: lookup.clone(),
        },
        SpaceSpec::CalGray {
            white,
            black,
            gamma,
        } => Form::CalGray {
            white: *white,
            black: *black,
            gamma: *gamma,
        },
        SpaceSpec::CalRGB {
            white,
            black,
            gamma,
            matrix,
        } => Form::CalRGB {
            white: *white,
            black: *black,
            gamma: *gamma,
            matrix: *matrix,
        },
        SpaceSpec::Lab {
            white,
            black,
            range,
        } => Form::Lab {
            white: *white,
            black: *black,
            range: *range,
        },
        SpaceSpec::Pattern { base } => Form::Pattern {
            base: match base {
                Some(base) => Some(Box::new(write_space(doc, base, filter)?)),
                None => None,
            },
        },
    })
}

/// A box as the four numbers of a PDF rectangle.
fn corners(b: Bounds) -> [f32; 4] {
    [b.llx, b.lly, b.urx, b.ury]
}

/// The `Decode` a reader assumes when none is written (ISO 32000-1
/// Table 90): the full sample range for an Indexed space, `[0 100]` and
/// the a*/b* range for a Lab space, `[0 1]` per component otherwise, and
/// `[0 1]` for a mask.
fn default_decode(spec: &ImageSpec) -> Vec<f32> {
    match &spec.color_space {
        Some(SpaceSpec::Indexed { .. }) => {
            vec![0.0, 2f32.powi(i32::from(spec.bits_per_component)) - 1.0]
        }
        Some(SpaceSpec::Lab { range, .. }) => {
            vec![0.0, 100.0, range[0], range[1], range[2], range[3]]
        }
        Some(space) => [0.0, 1.0].repeat(space.components()),
        None => vec![0.0, 1.0],
    }
}

fn write_image<W: Write>(
    doc: &mut Document<W>,
    image: &Image,
    space: Option<&Form>,
) -> Result<Ref, pdf_out::Error> {
    let spec = &image.spec;
    let filter = match spec.encoded {
        Some(Encoded::Dct) => Filter::Dct,
        None => Filter::Flate,
    };
    let xobject = doc.alloc();
    doc.write_stream(xobject, filter, &image.data, |d| {
        d.key("Type").name("XObject");
        d.key("Subtype").name("Image");
        d.key("Width").int(i64::from(spec.width));
        d.key("Height").int(i64::from(spec.height));
        match space {
            Some(space) => space.put(d.key("ColorSpace")),
            None => d.key("ImageMask").boolean(true),
        }
        d.key("BitsPerComponent")
            .int(i64::from(spec.bits_per_component));
        if spec.decode != default_decode(spec) {
            d.key("Decode").array(|a| {
                for &value in &spec.decode {
                    a.real(value);
                }
            });
        }
        if spec.interpolate {
            d.key("Interpolate").boolean(true);
        }
    })?;
    Ok(xobject)
}

/// The `Encode` a reader assumes for a sampled function (ISO 32000-1
/// Table 39): the whole index range of every input dimension.
fn default_encode(size: &[u32]) -> Vec<f32> {
    size.iter().flat_map(|&n| [0.0, n as f32 - 1.0]).collect()
}

fn put_matrix(d: &mut DictBuilder<'_>, key: &str, matrix: Matrix) {
    d.key(key).array(|a| {
        for value in matrix.0 {
            a.real(value);
        }
    });
}

/// Writes `spec` as a function object (ISO 32000-1 §7.10): a stream of
/// the samples for type 0, a dictionary for types 2 and 3 with the parts
/// of a stitching function written first as objects of their own.
/// Entries at their default values are omitted.
fn write_function_spec<W: Write>(
    doc: &mut Document<W>,
    spec: &FunctionSpec,
) -> Result<Ref, pdf_out::Error> {
    match spec {
        FunctionSpec::Sampled {
            domain,
            range,
            size,
            bits,
            order,
            encode,
            decode,
            samples,
        } => {
            let object = doc.alloc();
            doc.write_stream(object, Filter::Flate, samples, |d| {
                d.key("FunctionType").int(0);
                put_reals(d, "Domain", domain);
                put_reals(d, "Range", range);
                d.key("Size").array(|a| {
                    for &n in size {
                        a.int(i64::from(n));
                    }
                });
                d.key("BitsPerSample").int(i64::from(*bits));
                if *order != 1 {
                    d.key("Order").int(i64::from(*order));
                }
                if *encode != default_encode(size) {
                    put_reals(d, "Encode", encode);
                }
                if decode != range {
                    put_reals(d, "Decode", decode);
                }
            })?;
            Ok(object)
        }
        FunctionSpec::Exponential {
            domain,
            range,
            c0,
            c1,
            n,
        } => {
            let object = doc.alloc();
            doc.write_obj(object, |v| {
                v.dict(|d| {
                    d.key("FunctionType").int(2);
                    put_reals(d, "Domain", domain);
                    if !range.is_empty() {
                        put_reals(d, "Range", range);
                    }
                    if c0.as_slice() != [0.0] {
                        put_reals(d, "C0", c0);
                    }
                    if c1.as_slice() != [1.0] {
                        put_reals(d, "C1", c1);
                    }
                    d.key("N").real(*n);
                })
            })?;
            Ok(object)
        }
        FunctionSpec::Stitching {
            domain,
            range,
            functions,
            bounds,
            encode,
        } => {
            let parts = functions
                .iter()
                .map(|part| write_function_spec(doc, part))
                .collect::<Result<Vec<_>, _>>()?;
            let object = doc.alloc();
            doc.write_obj(object, |v| {
                v.dict(|d| {
                    d.key("FunctionType").int(3);
                    put_reals(d, "Domain", domain);
                    if !range.is_empty() {
                        put_reals(d, "Range", range);
                    }
                    d.key("Functions").array(|a| {
                        for &part in &parts {
                            a.reference(part);
                        }
                    });
                    put_reals(d, "Bounds", bounds);
                    put_reals(d, "Encode", encode);
                })
            })?;
            Ok(object)
        }
    }
}

/// The `Function` entry: one reference for a single function, an array
/// of them for one function per component, nothing for a mesh without.
fn put_function(d: &mut DictBuilder<'_>, functions: &[Ref]) {
    match functions {
        [] => {}
        [one] => d.key("Function").reference(*one),
        many => d.key("Function").array(|a| {
            for &f in many {
                a.reference(f);
            }
        }),
    }
}

/// Writes `spec` as a shading object (ISO 32000-1 §8.7.4.3): its
/// functions first, then a dictionary for types 1 to 3 or a stream of
/// the packed mesh data for types 4 to 7. `filter` applies to a
/// calculator function the colour space may need.
fn write_shading<W: Write>(
    doc: &mut Document<W>,
    spec: &ShadingSpec,
    filter: Filter,
) -> Result<Ref, pdf_out::Error> {
    let space = write_space(doc, &spec.space, filter)?;
    let functions = spec
        .kind
        .function()
        .iter()
        .map(|f| write_function_spec(doc, f))
        .collect::<Result<Vec<_>, _>>()?;
    let common = |d: &mut DictBuilder<'_>| {
        d.key("ShadingType")
            .int(i64::from(spec.kind.shading_type()));
        space.put(d.key("ColorSpace"));
        if let Some(background) = &spec.background {
            put_reals(d, "Background", background);
        }
        if let Some(bbox) = spec.bbox {
            d.key("BBox").array(|a| put_bounds(a, corners(bbox)));
        }
        if spec.antialias {
            d.key("AntiAlias").boolean(true);
        }
    };
    let object = doc.alloc();
    match &spec.kind {
        ShadingKind::Function { domain, matrix, .. } => doc.write_obj(object, |v| {
            v.dict(|d| {
                common(d);
                if *domain != [0.0, 1.0, 0.0, 1.0] {
                    put_reals(d, "Domain", domain);
                }
                if *matrix != Matrix::IDENTITY {
                    put_matrix(d, "Matrix", *matrix);
                }
                put_function(d, &functions);
            })
        })?,
        ShadingKind::Axial {
            coords,
            domain,
            extend,
            ..
        } => doc.write_obj(object, |v| {
            v.dict(|d| {
                common(d);
                put_reals(d, "Coords", coords);
                put_axis(d, domain, extend, &functions);
            })
        })?,
        ShadingKind::Radial {
            coords,
            domain,
            extend,
            ..
        } => doc.write_obj(object, |v| {
            v.dict(|d| {
                common(d);
                put_reals(d, "Coords", coords);
                put_axis(d, domain, extend, &functions);
            })
        })?,
        ShadingKind::Mesh {
            ty,
            bits_per_coordinate,
            bits_per_component,
            bits_per_flag,
            decode,
            vertices_per_row,
            data,
            ..
        } => doc.write_stream(object, Filter::Flate, data, |d| {
            common(d);
            d.key("BitsPerCoordinate")
                .int(i64::from(*bits_per_coordinate));
            d.key("BitsPerComponent")
                .int(i64::from(*bits_per_component));
            if *ty != 5 {
                d.key("BitsPerFlag").int(i64::from(*bits_per_flag));
            }
            if let Some(per_row) = vertices_per_row {
                d.key("VerticesPerRow").int(i64::from(*per_row));
            }
            put_reals(d, "Decode", decode);
            put_function(d, &functions);
        })?,
    }
    Ok(object)
}

/// The entries an axial or radial shading shares after `Coords`
/// (Tables 80 and 81), the defaults left out.
fn put_axis(d: &mut DictBuilder<'_>, domain: &[f32; 2], extend: &[bool; 2], functions: &[Ref]) {
    if *domain != [0.0, 1.0] {
        put_reals(d, "Domain", domain);
    }
    put_function(d, functions);
    if *extend != [false, false] {
        d.key("Extend").array(|a| {
            a.boolean(extend[0]).boolean(extend[1]);
        });
    }
}

/// The overprint values selected anywhere on `page`: its operations
/// and those of every cell, body, and glyph procedure it holds.
fn overprints_used(page: &Page) -> BTreeSet<bool> {
    let resources = &page.resources;
    let mut used = Refs::of(&page.ops, resources).overprints;
    for spec in &resources.patterns {
        used.extend(Refs::of(spec.ops(), resources).overprints);
    }
    for spec in &resources.forms {
        used.extend(Refs::of(&spec.ops, resources).overprints);
    }
    for spec in &resources.fonts {
        used.extend(crate::fonts::references(spec, resources, false).overprints);
    }
    used
}

fn write_ext_gstate<W: Write>(doc: &mut Document<W>, on: bool) -> Result<Ref, pdf_out::Error> {
    let object = doc.alloc();
    doc.write_obj(object, |v| {
        v.dict(|d| {
            d.key("Type").name("ExtGState");
            d.key("OP").boolean(on);
            d.key("op").boolean(on);
        })
    })?;
    Ok(object)
}

/// The written objects behind one page's resources, indexed like the IR.
pub(crate) struct Objects {
    spaces: Vec<Form>,
    images: Vec<Ref>,
    shadings: Vec<Ref>,
    fonts: Vec<Ref>,
    patterns: Vec<Ref>,
    forms: Vec<Ref>,
    ext_gstates: BTreeMap<bool, Ref>,
    recode: Recode,
}

impl Objects {
    /// Writes the function streams, image XObjects, shading objects,
    /// font objects, pattern objects, and form XObjects `page` needs
    /// (fonts the document already has are reused through `fonts`);
    /// `filter` applies to the text streams (image data, samples, and
    /// mesh data are Flate, or the DCT stream an image arrived as).
    /// Images are downsampled as `params` asks, counted in `tally`;
    /// text the fonts could not carry and images left as they are for a
    /// reason are noted in `notes`.
    pub(crate) fn write<W: Write>(
        doc: &mut Document<W>,
        page: &Page,
        filter: Filter,
        params: &Params,
        fonts: &mut FontTable,
        notes: &mut Vec<String>,
        tally: &mut Tally,
    ) -> Result<Self, pdf_out::Error> {
        let mut spaces = Vec::with_capacity(page.resources.color_spaces.len());
        for spec in &page.resources.color_spaces {
            spaces.push(write_space(doc, spec, filter)?);
        }
        let painted = downsample::painted(page);
        let mut images = Vec::with_capacity(page.resources.images.len());
        for (index, image) in page.resources.images.iter().enumerate() {
            let space = image.color_space.map(|r| &spaces[r.0]);
            let reduced;
            let image = match downsample::reduce(image, &painted[index], params) {
                Outcome::Unchanged => image,
                Outcome::Reduced {
                    image: smaller,
                    mono_subsampled,
                } => {
                    tally.images += 1;
                    tally.mono_subsampled |= mono_subsampled;
                    reduced = smaller;
                    &reduced
                }
                Outcome::Unsupported(why) => {
                    notes.push(format!("image {index} ({why}) is not downsampled"));
                    image
                }
            };
            images.push(write_image(doc, image, space)?);
        }
        let resources = &page.resources;
        let mut shadings = Vec::with_capacity(resources.shadings.len());
        for spec in &resources.shadings {
            shadings.push(write_shading(doc, spec, filter)?);
        }
        let mut ext_gstates = BTreeMap::new();
        for on in overprints_used(page) {
            ext_gstates.insert(on, write_ext_gstate(doc, on)?);
        }
        // Allocated before the fonts are written and written after them:
        // a glyph procedure may name a pattern, a cell may show text.
        let mut objects = Objects {
            spaces,
            images,
            shadings,
            fonts: Vec::new(),
            patterns: resources.patterns.iter().map(|_| doc.alloc()).collect(),
            forms: resources.forms.iter().map(|_| doc.alloc()).collect(),
            ext_gstates,
            recode: Recode::new(),
        };
        (objects.fonts, objects.recode) = write_fonts(
            doc,
            page,
            filter,
            params.embed_all_fonts,
            fonts,
            &objects,
            notes,
        )?;
        for (index, spec) in resources.patterns.iter().enumerate() {
            let object = objects.patterns[index];
            let PatternSpec::Tiling {
                matrix,
                bbox,
                xstep,
                ystep,
                paint_type,
                tiling_type,
                ops,
            } = spec
            else {
                let PatternSpec::Shading { matrix, shading } = spec else {
                    continue;
                };
                doc.write_obj(object, |v| {
                    v.dict(|d| {
                        d.key("Type").name("Pattern");
                        d.key("PatternType").int(2);
                        d.key("Shading").reference(objects.shadings[shading.0]);
                        put_matrix(d, "Matrix", *matrix);
                    })
                })?;
                continue;
            };
            let name = pattern_name(PatternIndex(index));
            let rendered = content::render(ops, resources, &objects.recode);
            let refs = Refs::of(ops, resources);
            notes.extend(
                rendered
                    .notes
                    .into_iter()
                    .map(|note| format!("pattern {name}: {note}")),
            );
            doc.write_stream(object, filter, &rendered.bytes, |d| {
                d.key("Type").name("Pattern");
                d.key("PatternType").int(1);
                d.key("PaintType").int(i64::from(*paint_type));
                d.key("TilingType").int(i64::from(*tiling_type));
                d.key("BBox").array(|a| put_bounds(a, corners(*bbox)));
                d.key("XStep").real(*xstep);
                d.key("YStep").real(*ystep);
                put_matrix(d, "Matrix", *matrix);
                d.key("Resources")
                    .dict(|res| objects.resources_dict(res, &objects.fonts, Some(&refs)));
            })?;
        }
        for (index, spec) in resources.forms.iter().enumerate() {
            let name = form_name(FormIndex(index));
            let rendered = content::render(&spec.ops, resources, &objects.recode);
            let refs = Refs::of(&spec.ops, resources);
            notes.extend(
                rendered
                    .notes
                    .into_iter()
                    .map(|note| format!("form {name}: {note}")),
            );
            doc.write_stream(objects.forms[index], filter, &rendered.bytes, |d| {
                d.key("Type").name("XObject");
                d.key("Subtype").name("Form");
                d.key("BBox").array(|a| put_bounds(a, corners(spec.bbox)));
                put_matrix(d, "Matrix", Matrix::IDENTITY);
                d.key("Resources")
                    .dict(|res| objects.resources_dict(res, &objects.fonts, Some(&refs)));
            })?;
        }
        Ok(objects)
    }

    /// The one-byte codes of the page's composite fonts written as Type 3
    /// fallbacks, for the content writer.
    pub(crate) fn recode(&self) -> &Recode {
        &self.recode
    }

    /// Fills the page's `Resources` dictionary: `ColorSpace` for the
    /// non-device spaces, `XObject` for the images and forms, `Shading`
    /// for the shadings, `Font` for the fonts, and `Pattern` for the
    /// patterns, each only when there is something to list.
    pub(crate) fn resources(&self, d: &mut DictBuilder<'_>) {
        self.resources_dict(d, &self.fonts, None);
    }

    fn listed_spaces<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.spaces.len()).filter(move |&i| {
            !self.spaces[i].is_direct() && only.is_none_or(|refs| refs.spaces.contains(&i))
        })
    }

    fn listed_images<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.images.len()).filter(move |&i| only.is_none_or(|refs| refs.images.contains(&i)))
    }

    fn listed_fonts<'a>(
        &'a self,
        fonts: &'a [Ref],
        only: Option<&'a Refs>,
    ) -> impl Iterator<Item = usize> + 'a {
        (0..fonts.len()).filter(move |&i| only.is_none_or(|refs| refs.fonts.contains(&i)))
    }

    fn listed_shadings<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.shadings.len())
            .filter(move |&i| only.is_none_or(|refs| refs.shadings.contains(&i)))
    }

    fn listed_patterns<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.patterns.len())
            .filter(move |&i| only.is_none_or(|refs| refs.patterns.contains(&i)))
    }

    fn listed_forms<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.forms.len()).filter(move |&i| only.is_none_or(|refs| refs.forms.contains(&i)))
    }

    fn listed_ext_gstates<'a>(
        &'a self,
        only: Option<&'a Refs>,
    ) -> impl Iterator<Item = (bool, Ref)> + 'a {
        self.ext_gstates
            .iter()
            .filter(move |(on, _)| only.is_none_or(|refs| refs.overprints.contains(on)))
            .map(|(&on, &r)| (on, r))
    }

    /// Whether a resources dictionary restricted to `only` would list
    /// anything.
    pub(crate) fn names_anything(&self, only: &Refs, fonts: &[Ref]) -> bool {
        self.listed_spaces(Some(only)).next().is_some()
            || self.listed_images(Some(only)).next().is_some()
            || self.listed_shadings(Some(only)).next().is_some()
            || self.listed_fonts(fonts, Some(only)).next().is_some()
            || self.listed_patterns(Some(only)).next().is_some()
            || self.listed_forms(Some(only)).next().is_some()
            || self.listed_ext_gstates(Some(only)).next().is_some()
    }

    /// A resources dictionary over the page's objects, restricted to the
    /// indices in `only` when given; `fonts` are the page's font objects
    /// by index. `ColorSpace` lists the spaces selected by name,
    /// `XObject` the images and forms, `Shading` the shadings, `Font`
    /// the fonts, `Pattern` the patterns, and `ExtGState` the overprint
    /// settings, each only when there is something to list.
    pub(crate) fn resources_dict(
        &self,
        d: &mut DictBuilder<'_>,
        fonts: &[Ref],
        only: Option<&Refs>,
    ) {
        if self.listed_spaces(only).next().is_some() {
            d.key("ColorSpace").dict(|cs| {
                for i in self.listed_spaces(only) {
                    self.spaces[i].put(cs.key(&space_name(SpaceRef(i))));
                }
            });
        }
        if self.listed_images(only).next().is_some() || self.listed_forms(only).next().is_some() {
            d.key("XObject").dict(|x| {
                for i in self.listed_images(only) {
                    x.key(&image_name(ImageRef(i))).reference(self.images[i]);
                }
                for i in self.listed_forms(only) {
                    x.key(&form_name(FormIndex(i))).reference(self.forms[i]);
                }
            });
        }
        if self.listed_shadings(only).next().is_some() {
            d.key("Shading").dict(|sh| {
                for i in self.listed_shadings(only) {
                    sh.key(&shading_name(ShadingIndex(i)))
                        .reference(self.shadings[i]);
                }
            });
        }
        if self.listed_fonts(fonts, only).next().is_some() {
            d.key("Font").dict(|f| {
                for i in self.listed_fonts(fonts, only) {
                    f.key(&font_name(FontIndex(i))).reference(fonts[i]);
                }
            });
        }
        if self.listed_patterns(only).next().is_some() {
            d.key("Pattern").dict(|p| {
                for i in self.listed_patterns(only) {
                    p.key(&pattern_name(PatternIndex(i)))
                        .reference(self.patterns[i]);
                }
            });
        }
        if self.listed_ext_gstates(only).next().is_some() {
            d.key("ExtGState").dict(|e| {
                for (on, r) in self.listed_ext_gstates(only) {
                    e.key(&ext_gstate_name(on)).reference(r);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ps_vm::Matrix;

    fn spec(color_space: Option<SpaceSpec>, bits: u8, decode: Vec<f32>) -> ImageSpec {
        ImageSpec {
            width: 1,
            height: 1,
            bits_per_component: bits,
            color_space,
            decode,
            matrix: Matrix::IDENTITY,
            interpolate: false,
            is_mask: false,
            encoded: None,
        }
    }

    #[test]
    fn default_decode_follows_space_and_depth() {
        assert_eq!(
            default_decode(&spec(Some(SpaceSpec::DeviceRGB), 8, Vec::new())),
            [0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
        );
        assert_eq!(default_decode(&spec(None, 1, Vec::new())), [0.0, 1.0]);
        let indexed = SpaceSpec::Indexed {
            base: Box::new(SpaceSpec::DeviceGray),
            hival: 3,
            lookup: vec![0; 4],
        };
        assert_eq!(
            default_decode(&spec(Some(indexed.clone()), 4, Vec::new())),
            [0.0, 15.0]
        );
        assert_eq!(
            default_decode(&spec(Some(indexed), 8, Vec::new())),
            [0.0, 255.0]
        );
    }

    #[test]
    fn names_follow_the_ir_index() {
        assert_eq!(space_name(SpaceRef(0)), "CS0");
        assert_eq!(image_name(ImageRef(12)), "Im12");
        assert_eq!(font_name(FontIndex(3)), "F3");
        assert_eq!(pattern_name(PatternIndex(2)), "P2");
        assert_eq!(form_name(FormIndex(1)), "Fm1");
        assert_eq!(shading_name(ShadingIndex(4)), "Sh4");
        assert_eq!(ext_gstate_name(false), "GS0");
        assert_eq!(ext_gstate_name(true), "GS1");
    }

    #[test]
    fn the_default_encode_spans_every_dimension_of_the_table() {
        assert_eq!(default_encode(&[2]), [0.0, 1.0]);
        assert_eq!(default_encode(&[21, 31]), [0.0, 20.0, 0.0, 30.0]);
        assert_eq!(default_encode(&[1]), [0.0, 0.0]);
    }

    #[test]
    fn a_pattern_space_without_a_base_is_selected_directly() {
        assert!(Form::Pattern { base: None }.is_direct());
        assert!(
            !Form::Pattern {
                base: Some(Box::new(Form::Device("DeviceRGB")))
            }
            .is_direct()
        );
        assert!(
            !Form::Indexed {
                base: Box::new(Form::Device("DeviceGray")),
                hival: 1,
                lookup: vec![0, 0]
            }
            .is_direct()
        );
    }
}
