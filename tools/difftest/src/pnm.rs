// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Raw PNM images as the profile's rasteriser writes them — `P5` grey or
//! `P6` colour, 8 bits per channel — and the pixel comparison.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// 1 for grey, 3 for colour.
    pub channels: usize,
    pub data: Vec<u8>,
}

impl Image {
    /// Pixel `index` (row-major) as colour, grey expanded.
    fn pixel(&self, index: usize) -> [u8; 3] {
        let at = index * self.channels;
        if self.channels == 1 {
            [self.data[at]; 3]
        } else {
            [self.data[at], self.data[at + 1], self.data[at + 2]]
        }
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// The next header token after whitespace and `#` comments.
fn token<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    loop {
        while *pos < bytes.len() && is_space(bytes[*pos]) {
            *pos += 1;
        }
        if bytes.get(*pos) == Some(&b'#') {
            while *pos < bytes.len() && bytes[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        break;
    }
    let start = *pos;
    while *pos < bytes.len() && !is_space(bytes[*pos]) {
        *pos += 1;
    }
    (*pos > start).then(|| &bytes[start..*pos])
}

fn number(bytes: &[u8], pos: &mut usize, what: &str) -> Result<usize, String> {
    token(bytes, pos)
        .and_then(|t| std::str::from_utf8(t).ok())
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| format!("PNM header: {what} missing or not a number"))
}

/// Parses a `P5` or `P6` image with maxval 255; anything else is an
/// unsupported-format error.
pub fn parse(bytes: &[u8]) -> Result<Image, String> {
    let mut pos = 0;
    let channels = match token(bytes, &mut pos) {
        Some(b"P5") => 1,
        Some(b"P6") => 3,
        Some(other) => {
            return Err(format!(
                "unsupported PNM kind `{}` (P5 or P6 expected)",
                String::from_utf8_lossy(other)
            ));
        }
        None => return Err("empty PNM".to_string()),
    };
    let width = number(bytes, &mut pos, "width")?;
    let height = number(bytes, &mut pos, "height")?;
    let maxval = number(bytes, &mut pos, "maxval")?;
    if maxval != 255 {
        return Err(format!("unsupported PNM maxval {maxval} (255 expected)"));
    }
    // One whitespace byte separates the header from the samples.
    pos += 1;
    let needed = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(channels))
        .ok_or("PNM dimensions overflow")?;
    let end = pos
        .checked_add(needed)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| format!("PNM truncated: {width}x{height}x{channels} samples expected"))?;
    Ok(Image {
        width,
        height,
        channels,
        data: bytes[pos..end].to_vec(),
    })
}

/// What comparing two same-sized images found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diff {
    pub differing: usize,
    pub total: usize,
    /// The largest channel difference seen.
    pub max: u8,
}

impl Diff {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.differing as f64 / self.total as f64
        }
    }
}

/// Counts the pixels of `a` and `b` in which some channel differs by
/// more than `threshold`; the images must have the same size.
pub fn compare(a: &Image, b: &Image, threshold: u8) -> Result<Diff, String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!(
            "sizes differ: {}x{} against {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }
    let total = a.width * a.height;
    let mut differing = 0;
    let mut max = 0u8;
    for index in 0..total {
        let (p, q) = (a.pixel(index), b.pixel(index));
        let delta = (0..3).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
        max = max.max(delta);
        if delta > threshold {
            differing += 1;
        }
    }
    Ok(Diff {
        differing,
        total,
        max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(width: usize, height: usize, value: u8) -> Image {
        Image {
            width,
            height,
            channels: 1,
            data: vec![value; width * height],
        }
    }

    #[test]
    fn headers_with_comments_and_both_kinds_are_read() {
        let p5 = parse(b"P5\n# made up\n2 2\n255\n\x00\x40\x80\xff").unwrap();
        assert_eq!(
            p5,
            Image {
                width: 2,
                height: 2,
                channels: 1,
                data: vec![0x00, 0x40, 0x80, 0xff]
            }
        );
        let p6 = parse(b"P6 1 1 255 \x01\x02\x03trailing").unwrap();
        assert_eq!(p6.channels, 3);
        assert_eq!(p6.data, vec![1, 2, 3]);
        assert_eq!(p6.pixel(0), [1, 2, 3]);
        assert_eq!(p5.pixel(3), [0xff; 3]);
    }

    #[test]
    fn other_formats_and_short_data_are_errors() {
        assert!(
            parse(b"P4\n8 1\n\xff")
                .unwrap_err()
                .contains("unsupported PNM kind `P4`")
        );
        assert!(
            parse(b"P5\n1 1\n65535\n\x00\x00")
                .unwrap_err()
                .contains("maxval 65535")
        );
        assert!(
            parse(b"P5\n2 2\n255\n\x00")
                .unwrap_err()
                .contains("truncated")
        );
        assert!(parse(b"P5\nx 2\n255\n").unwrap_err().contains("width"));
        assert!(parse(b"").unwrap_err().contains("empty"));
    }

    #[test]
    fn identical_images_do_not_differ() {
        let diff = compare(&grey(4, 3, 200), &grey(4, 3, 200), 0).unwrap();
        assert_eq!(
            diff,
            Diff {
                differing: 0,
                total: 12,
                max: 0
            }
        );
        assert_eq!(diff.fraction(), 0.0);
    }

    #[test]
    fn one_pixel_over_the_threshold_counts_once() {
        let a = grey(10, 10, 255);
        let mut b = a.clone();
        b.data[42] = 200;
        let diff = compare(&a, &b, 48).unwrap();
        assert_eq!(diff.differing, 1);
        assert_eq!(diff.max, 55);
        assert_eq!(diff.fraction(), 0.01);
        assert_eq!(compare(&a, &b, 55).unwrap().differing, 0);
    }

    #[test]
    fn the_fraction_reflects_every_differing_pixel() {
        let a = grey(10, 10, 0);
        let mut b = a.clone();
        for value in b.data.iter_mut().take(25) {
            *value = 255;
        }
        assert_eq!(compare(&a, &b, 48).unwrap().fraction(), 0.25);
    }

    #[test]
    fn grey_and_colour_compare_channel_by_channel() {
        let a = grey(1, 2, 100);
        let b = Image {
            width: 1,
            height: 2,
            channels: 3,
            data: vec![100, 100, 100, 100, 100, 160],
        };
        let diff = compare(&a, &b, 48).unwrap();
        assert_eq!(diff.differing, 1);
        assert_eq!(diff.max, 60);
        assert!(
            compare(&a, &grey(2, 1, 100), 0)
                .unwrap_err()
                .contains("sizes differ")
        );
        assert_eq!(
            compare(&grey(0, 0, 0), &grey(0, 0, 0), 0)
                .unwrap()
                .fraction(),
            0.0
        );
    }
}
