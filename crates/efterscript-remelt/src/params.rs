// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The distillation parameters the writer works from (the parameters
//! reference names them; the honoured set is the project's): typed
//! fields for the keys honoured, a map of every other key recorded, and
//! the keys an embedder locks against a job's `setdistillerparams`. The
//! embedder's values are the starting point; each request from the job
//! merges over them, and what a request could not change is returned
//! for the report rather than raised, since the job goes on either way.

use std::collections::{BTreeMap, BTreeSet};

use efterscript_graphics::dump::mark_value;
pub use efterscript_vm::MarkValue;

/// How an image class is reduced when downsampling applies to it;
/// bicubic requests are honoured as averaging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Downsample {
    Average,
    Subsample,
}

impl Downsample {
    fn name(self) -> &'static str {
        match self {
            Downsample::Average => "Average",
            Downsample::Subsample => "Subsample",
        }
    }

    fn parse(name: &[u8]) -> Option<Self> {
        match name {
            b"Average" | b"Bicubic" => Some(Downsample::Average),
            b"Subsample" => Some(Downsample::Subsample),
            _ => None,
        }
    }
}

/// The only colour conversion strategy honoured.
const LEAVE_COLOR_UNCHANGED: &str = "LeaveColorUnchanged";

/// Resolutions outside this range are refused, as the reference's legal
/// range has it.
const RESOLUTION_RANGE: std::ops::RangeInclusive<u32> = 9..=2400;

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Content and function streams go through the Flate container;
    /// applies to pages written after a change. Image data always does.
    pub compress_pages: bool,
    /// Embed the standard fourteen faces from their outline assets.
    pub embed_all_fonts: bool,
    /// Embed subsets of the glyphs used rather than whole programs.
    pub subset_fonts: bool,
    /// The version the file header names, as (major, minor); 1.3 to 1.7,
    /// features unchanged.
    pub compatibility_level: (u8, u8),
    pub downsample_color_images: bool,
    pub downsample_gray_images: bool,
    pub downsample_mono_images: bool,
    /// Target resolutions in samples per inch.
    pub color_image_resolution: u32,
    pub gray_image_resolution: u32,
    pub mono_image_resolution: u32,
    pub color_image_downsample_type: Downsample,
    pub gray_image_downsample_type: Downsample,
    pub mono_image_downsample_type: Downsample,
    /// The strategy as given; only `LeaveColorUnchanged` is honoured and
    /// any other value is reported.
    pub color_conversion_strategy: String,
    /// Every other key, recorded with its last value and reported as not
    /// honoured.
    pub others: BTreeMap<String, MarkValue>,
    /// Keys a job's request may not change.
    pub locked: BTreeSet<String>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            compress_pages: true,
            embed_all_fonts: false,
            subset_fonts: true,
            compatibility_level: (1, 7),
            downsample_color_images: false,
            downsample_gray_images: false,
            downsample_mono_images: false,
            color_image_resolution: 150,
            gray_image_resolution: 150,
            mono_image_resolution: 300,
            color_image_downsample_type: Downsample::Average,
            gray_image_downsample_type: Downsample::Average,
            mono_image_downsample_type: Downsample::Average,
            color_conversion_strategy: LEAVE_COLOR_UNCHANGED.to_string(),
            others: BTreeMap::new(),
            locked: BTreeSet::new(),
        }
    }
}

/// A key and why its value was not applied: the value as given, with a
/// reason in parentheses where the key itself is honoured.
pub type NotHonoured = (String, String);

impl Params {
    /// `self` with `key` locked against a job's requests.
    pub fn lock(mut self, key: &str) -> Self {
        self.locked.insert(key.to_string());
        self
    }

    /// Merges a request's entries in order, skipping locked keys, and
    /// returns what was not honoured.
    pub fn merge(&mut self, entries: &[(Vec<u8>, MarkValue)]) -> Vec<NotHonoured> {
        let mut refused = Vec::new();
        for (key, value) in entries {
            let key = String::from_utf8_lossy(key).into_owned();
            if self.locked.contains(&key) {
                let current = self
                    .get(&key)
                    .map_or_else(|| "not set".to_string(), |current| mark_value(&current));
                refused.push((key, format!("{} (locked at {current})", mark_value(value))));
                continue;
            }
            if let Some(reason) = self.set(&key, value) {
                refused.push((key, reason));
            }
        }
        refused
    }

    /// Applies one entry; `Some(text)` says why it was not honoured.
    /// A key not honoured is still recorded under `others`.
    fn set(&mut self, key: &str, value: &MarkValue) -> Option<String> {
        let shown = mark_value(value);
        let refuse = |why: &str| Some(format!("{shown} ({why})"));
        let bool_of = |slot: &mut bool| match value.as_bool() {
            Some(b) => {
                *slot = b;
                None
            }
            None => refuse("not a boolean"),
        };
        let resolution_of = |slot: &mut u32| match value.as_number() {
            Some(v) if v.is_finite() && RESOLUTION_RANGE.contains(&(v.round() as u32)) => {
                *slot = v.round() as u32;
                None
            }
            _ => refuse("9 to 2400"),
        };
        let type_of = |slot: &mut Downsample| match value.as_name().and_then(Downsample::parse) {
            Some(kind) => {
                *slot = kind;
                None
            }
            None => refuse("Average, Bicubic, or Subsample"),
        };
        match key {
            "CompressPages" => bool_of(&mut self.compress_pages),
            "EmbedAllFonts" => bool_of(&mut self.embed_all_fonts),
            "SubsetFonts" => bool_of(&mut self.subset_fonts),
            "DownsampleColorImages" => bool_of(&mut self.downsample_color_images),
            "DownsampleGrayImages" => bool_of(&mut self.downsample_gray_images),
            "DownsampleMonoImages" => bool_of(&mut self.downsample_mono_images),
            "ColorImageResolution" => resolution_of(&mut self.color_image_resolution),
            "GrayImageResolution" => resolution_of(&mut self.gray_image_resolution),
            "MonoImageResolution" => resolution_of(&mut self.mono_image_resolution),
            "ColorImageDownsampleType" => type_of(&mut self.color_image_downsample_type),
            "GrayImageDownsampleType" => type_of(&mut self.gray_image_downsample_type),
            "MonoImageDownsampleType" => type_of(&mut self.mono_image_downsample_type),
            "CompatibilityLevel" => match value.as_number() {
                Some(v) if v.is_finite() && (13..=17).contains(&((v * 10.0).round() as i32)) => {
                    self.compatibility_level = (1, ((v * 10.0).round() as i32 - 10) as u8);
                    None
                }
                _ => refuse("1.3 to 1.7"),
            },
            "ColorConversionStrategy" => match value.as_name() {
                Some(name) => {
                    self.color_conversion_strategy = String::from_utf8_lossy(name).into_owned();
                    if name == LEAVE_COLOR_UNCHANGED.as_bytes() {
                        None
                    } else {
                        refuse("only LeaveColorUnchanged")
                    }
                }
                None => refuse("not a name"),
            },
            _ => {
                self.others.insert(key.to_string(), value.clone());
                Some(shown)
            }
        }
    }

    /// The value of `key` as a job would read it: the honoured keys from
    /// their fields, the rest from `others`.
    pub fn get(&self, key: &str) -> Option<MarkValue> {
        let name = |text: &str| MarkValue::Name(text.as_bytes().to_vec());
        let resolution = |v: u32| MarkValue::Int(v as i32);
        Some(match key {
            "CompressPages" => MarkValue::Bool(self.compress_pages),
            "EmbedAllFonts" => MarkValue::Bool(self.embed_all_fonts),
            "SubsetFonts" => MarkValue::Bool(self.subset_fonts),
            "CompatibilityLevel" => {
                let (major, minor) = self.compatibility_level;
                MarkValue::Real(f32::from(major) + f32::from(minor) / 10.0)
            }
            "DownsampleColorImages" => MarkValue::Bool(self.downsample_color_images),
            "DownsampleGrayImages" => MarkValue::Bool(self.downsample_gray_images),
            "DownsampleMonoImages" => MarkValue::Bool(self.downsample_mono_images),
            "ColorImageResolution" => resolution(self.color_image_resolution),
            "GrayImageResolution" => resolution(self.gray_image_resolution),
            "MonoImageResolution" => resolution(self.mono_image_resolution),
            "ColorImageDownsampleType" => name(self.color_image_downsample_type.name()),
            "GrayImageDownsampleType" => name(self.gray_image_downsample_type.name()),
            "MonoImageDownsampleType" => name(self.mono_image_downsample_type.name()),
            "ColorConversionStrategy" => name(&self.color_conversion_strategy),
            _ => return self.others.get(key).cloned(),
        })
    }

    /// Every key with its value, the honoured ones first in a fixed
    /// order and then `others`: what seeds a job's view of the
    /// parameters.
    pub fn entries(&self) -> Vec<(Vec<u8>, MarkValue)> {
        HONOURED_KEYS
            .iter()
            .map(|key| (key.as_bytes().to_vec(), self.get(key).expect("honoured")))
            .chain(
                self.others
                    .iter()
                    .map(|(key, value)| (key.as_bytes().to_vec(), value.clone())),
            )
            .collect()
    }
}

/// The honoured keys, in the order [`Params::entries`] lists them.
pub const HONOURED_KEYS: [&str; 14] = [
    "CompressPages",
    "EmbedAllFonts",
    "SubsetFonts",
    "CompatibilityLevel",
    "DownsampleColorImages",
    "DownsampleGrayImages",
    "DownsampleMonoImages",
    "ColorImageResolution",
    "GrayImageResolution",
    "MonoImageResolution",
    "ColorImageDownsampleType",
    "GrayImageDownsampleType",
    "MonoImageDownsampleType",
    "ColorConversionStrategy",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: MarkValue) -> (Vec<u8>, MarkValue) {
        (key.as_bytes().to_vec(), value)
    }

    fn name(text: &str) -> MarkValue {
        MarkValue::Name(text.as_bytes().to_vec())
    }

    #[test]
    fn honoured_keys_apply_and_refused_values_keep_the_old_ones() {
        let mut params = Params::default();
        let refused = params.merge(&[
            entry("CompressPages", MarkValue::Bool(false)),
            entry("CompatibilityLevel", MarkValue::Real(1.4)),
            entry("CompatibilityLevel", MarkValue::Int(2)),
            entry("GrayImageResolution", MarkValue::Real(72.4)),
            entry("MonoImageResolution", MarkValue::Int(5)),
            entry("ColorImageDownsampleType", name("Bicubic")),
            entry("MonoImageDownsampleType", name("Subsample")),
            entry("GrayImageDownsampleType", name("Nearest")),
            entry("ColorConversionStrategy", name("sRGB")),
            entry("AutoRotatePages", name("All")),
        ]);
        assert!(!params.compress_pages);
        assert_eq!(params.compatibility_level, (1, 4));
        assert_eq!(params.gray_image_resolution, 72);
        assert_eq!(params.mono_image_resolution, 300);
        assert_eq!(params.color_image_downsample_type, Downsample::Average);
        assert_eq!(params.mono_image_downsample_type, Downsample::Subsample);
        assert_eq!(params.gray_image_downsample_type, Downsample::Average);
        assert_eq!(params.color_conversion_strategy, "sRGB");
        assert_eq!(params.others.get("AutoRotatePages"), Some(&name("All")));
        assert_eq!(
            refused,
            vec![
                (
                    "CompatibilityLevel".to_string(),
                    "2 (1.3 to 1.7)".to_string()
                ),
                (
                    "MonoImageResolution".to_string(),
                    "5 (9 to 2400)".to_string()
                ),
                (
                    "GrayImageDownsampleType".to_string(),
                    "/Nearest (Average, Bicubic, or Subsample)".to_string()
                ),
                (
                    "ColorConversionStrategy".to_string(),
                    "/sRGB (only LeaveColorUnchanged)".to_string()
                ),
                ("AutoRotatePages".to_string(), "/All".to_string()),
            ]
        );
    }

    #[test]
    fn locked_keys_keep_the_embedders_value_and_say_so() {
        let mut params = Params::default().lock("CompressPages");
        params.compress_pages = false;
        let refused = params.merge(&[
            entry("CompressPages", MarkValue::Bool(true)),
            entry("SubsetFonts", MarkValue::Bool(false)),
        ]);
        assert!(!params.compress_pages);
        assert!(!params.subset_fonts);
        assert_eq!(
            refused,
            vec![(
                "CompressPages".to_string(),
                "true (locked at false)".to_string()
            )]
        );
    }

    #[test]
    fn entries_agree_with_the_vm_defaults_and_read_back() {
        let ours: BTreeMap<Vec<u8>, MarkValue> = Params::default().entries().into_iter().collect();
        let vm: BTreeMap<Vec<u8>, MarkValue> = efterscript_vm::default_distiller_params()
            .into_iter()
            .collect();
        for (key, value) in &vm {
            if key == b"AutoRotatePages" {
                continue;
            }
            assert_eq!(
                ours.get(key),
                Some(value),
                "{}",
                String::from_utf8_lossy(key)
            );
        }
        assert_eq!(ours.len(), HONOURED_KEYS.len());
        let mut params = Params::default();
        params.merge(&[entry("Custom", MarkValue::Int(3))]);
        assert_eq!(params.get("Custom"), Some(MarkValue::Int(3)));
        assert_eq!(params.get("Missing"), None);
        assert_eq!(params.entries().len(), HONOURED_KEYS.len() + 1);
    }
}
