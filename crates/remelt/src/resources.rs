// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The objects a page's resources become. Colour spaces outside the device
//! families are written into the page's `ColorSpace` resource dictionary
//! as `/CSn` (n the IR's `SpaceRef` index); a Separation or DeviceN tint
//! transform is a Type 4 function stream (ISO 32000-1 §7.10.5) whose body
//! is the captured PostScript source verbatim, unchecked against the
//! calculator subset; a pattern space with an underlying space is the
//! array form `[/Pattern base]`, while one without is selected by its
//! family name like a device space and listed nowhere. Images are image
//! XObjects `/Imn` (§8.9.5) with their samples in the Flate container,
//! reduced first when the downsampling parameters say so (see
//! `downsample`); an image that arrived as a DCT stream keeps its bytes
//! verbatim under the `DCTDecode` filter.
//!
//! A pattern resource is a tiling pattern stream `/Pn` (§8.7.3) carrying
//! its paint and tiling types, box, steps, and matrix, and a form
//! resource a form XObject `/Fmn` (§8.10) with its box and the identity
//! matrix; each holds its content rendered through the content writer
//! over the page's resources and a `Resources` dictionary of what that
//! content names directly (§7.8.3) — a form placed inside it or a pattern
//! it paints with has a dictionary of its own. Their ids are allocated
//! before the fonts are written, so a glyph procedure can name them,
//! and the streams are written after, so they can name the fonts.
//!
//! The objects a page needs are written before the page itself, so a
//! `Resources` dictionary only ever refers to objects already in the file.
//! Fonts are `/Fn` (n the `FontIndex`), written once per document by
//! `fonts`.

use std::io::Write;

use pdf_out::{DictBuilder, Document, Filter, Ref, Val};
use ps_graphics::{FontIndex, FormIndex, Image, ImageRef, Page, PatternIndex, SpaceRef};
use ps_vm::{Bounds, Encoded, ImageSpec, SpaceSpec};

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
    Pattern {
        base: Option<Box<Form>>,
    },
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
/// Table 90): the full sample range for an Indexed space, `[0 1]` per
/// component otherwise, and `[0 1]` for a mask.
fn default_decode(spec: &ImageSpec) -> Vec<f32> {
    match &spec.color_space {
        Some(SpaceSpec::Indexed { .. }) => {
            vec![0.0, 2f32.powi(i32::from(spec.bits_per_component)) - 1.0]
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

/// The written objects behind one page's resources, indexed like the IR.
pub(crate) struct Objects {
    spaces: Vec<Form>,
    images: Vec<Ref>,
    fonts: Vec<Ref>,
    patterns: Vec<Ref>,
    forms: Vec<Ref>,
    recode: Recode,
}

impl Objects {
    /// Writes the function streams, image XObjects, font objects, pattern
    /// streams, and form XObjects `page` needs (fonts the document
    /// already has are reused through `fonts`); `filter` applies to the
    /// text streams (image data is Flate, or the DCT stream it arrived
    /// as). Images are downsampled as `params` asks, counted in `tally`;
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
        // Allocated before the fonts are written and written after them:
        // a glyph procedure may name a pattern, a cell may show text.
        let mut objects = Objects {
            spaces,
            images,
            fonts: Vec::new(),
            patterns: resources.patterns.iter().map(|_| doc.alloc()).collect(),
            forms: resources.forms.iter().map(|_| doc.alloc()).collect(),
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
            let name = pattern_name(PatternIndex(index));
            let rendered = content::render(&spec.ops, resources, &objects.recode);
            let refs = Refs::of(&spec.ops, resources);
            notes.extend(
                rendered
                    .notes
                    .into_iter()
                    .map(|note| format!("pattern {name}: {note}")),
            );
            doc.write_stream(objects.patterns[index], filter, &rendered.bytes, |d| {
                d.key("Type").name("Pattern");
                d.key("PatternType").int(1);
                d.key("PaintType").int(i64::from(spec.paint_type));
                d.key("TilingType").int(i64::from(spec.tiling_type));
                d.key("BBox").array(|a| put_bounds(a, corners(spec.bbox)));
                d.key("XStep").real(spec.xstep);
                d.key("YStep").real(spec.ystep);
                d.key("Matrix").array(|a| {
                    for value in spec.matrix.0 {
                        a.real(value);
                    }
                });
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
                d.key("Matrix").array(|a| {
                    for value in ps_vm::Matrix::IDENTITY.0 {
                        a.real(value);
                    }
                });
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
    /// non-device spaces, `XObject` for the images, and `Font` for the
    /// fonts, each only when there is something to list.
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

    fn listed_patterns<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.patterns.len())
            .filter(move |&i| only.is_none_or(|refs| refs.patterns.contains(&i)))
    }

    fn listed_forms<'a>(&'a self, only: Option<&'a Refs>) -> impl Iterator<Item = usize> + 'a {
        (0..self.forms.len()).filter(move |&i| only.is_none_or(|refs| refs.forms.contains(&i)))
    }

    /// Whether a resources dictionary restricted to `only` would list
    /// anything.
    pub(crate) fn names_anything(&self, only: &Refs, fonts: &[Ref]) -> bool {
        self.listed_spaces(Some(only)).next().is_some()
            || self.listed_images(Some(only)).next().is_some()
            || self.listed_fonts(fonts, Some(only)).next().is_some()
            || self.listed_patterns(Some(only)).next().is_some()
            || self.listed_forms(Some(only)).next().is_some()
    }

    /// A resources dictionary over the page's objects, restricted to the
    /// indices in `only` when given; `fonts` are the page's font objects
    /// by index. `ColorSpace` lists the spaces selected by name,
    /// `XObject` the images and forms, `Font` the fonts, and `Pattern`
    /// the patterns, each only when there is something to list.
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
