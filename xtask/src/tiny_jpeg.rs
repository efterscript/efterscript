// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask tiny-jpeg [--corpus]`: the project's own baseline JPEG
//! for the trivial case, so the `DCTDecode` passthrough has a stream of
//! known provenance to carry. The image is 16 by 16 grey samples made
//! of four flat 8 by 8 blocks. A flat block's forward DCT (ITU-T T.81
//! A.3.3) is its DC coefficient alone, eight times the level-shifted
//! value, so with a quantisation table of ones every block codes as one
//! DC difference and an end-of-block; the Huffman tables (Annex C, K.2)
//! hold just the difference categories the four blocks use and the
//! end-of-block symbol. Segments in order: SOI, DQT, SOF0, DHT for the
//! DC and AC tables, SOS, the entropy-coded data with `0xFF` bytes
//! stuffed, EOI (Annex B). Without `--corpus` the stream's hexadecimal
//! form is printed; with it, the corpus file that embeds it.

use std::process::ExitCode;

/// The four blocks' grey levels in raster order: top left, top right,
/// bottom left, bottom right. The last difference is negative and its
/// extra bits fall on a byte boundary, so the data holds a stuffed byte.
pub const GREYS: [u8; 4] = [128, 160, 224, 160];

/// Bits most-significant first, a `0x00` stuffed after every `0xFF`
/// byte, the last byte padded with ones (T.81 B.1.1.5, F.1.2.3).
struct Bits {
    out: Vec<u8>,
    acc: u32,
    count: u32,
}

impl Bits {
    fn put(&mut self, value: u32, width: u32) {
        for i in (0..width).rev() {
            self.acc = self.acc << 1 | (value >> i) & 1;
            self.count += 1;
            if self.count == 8 {
                self.out.push(self.acc as u8);
                if self.acc == 0xFF {
                    self.out.push(0x00);
                }
                self.acc = 0;
                self.count = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        while self.count != 0 {
            self.put(1, 1);
        }
        self.out
    }
}

/// The category of a DC difference: the bit count of its magnitude
/// (T.81 Table F.1).
fn category(diff: i32) -> u32 {
    32 - diff.unsigned_abs().leading_zeros()
}

/// The extra bits carrying a difference within its category: the value
/// itself when positive, one less than its two's complement otherwise.
fn extra(diff: i32, category: u32) -> u32 {
    if diff >= 0 {
        diff as u32
    } else {
        (diff + (1 << category) - 1) as u32
    }
}

/// A DHT segment for a table whose symbols, in order, get codes of one,
/// two, three… bits: 0, 10, 110… (T.81 B.2.4.2, C.2).
fn huffman_table(class_and_id: u8, symbols: &[u8]) -> Vec<u8> {
    let mut segment = vec![0xFF, 0xC4];
    let length = 2 + 1 + 16 + symbols.len();
    segment.extend_from_slice(&(length as u16).to_be_bytes());
    segment.push(class_and_id);
    for length in 1..=16 {
        segment.push(u8::from(length <= symbols.len()));
    }
    segment.extend_from_slice(symbols);
    segment
}

/// The stream for four flat blocks of the given greys.
pub fn encode(greys: &[u8; 4]) -> Vec<u8> {
    let dc: Vec<i32> = greys.iter().map(|&g| 8 * (i32::from(g) - 128)).collect();
    let diffs: Vec<i32> = dc
        .iter()
        .scan(0, |prev, &v| {
            let diff = v - *prev;
            *prev = v;
            Some(diff)
        })
        .collect();
    let mut categories: Vec<u8> = diffs.iter().map(|&d| category(d) as u8).collect();
    categories.sort_unstable();
    categories.dedup();

    let mut stream = vec![0xFF, 0xD8];
    // DQT: 8-bit table 0, all ones, in zigzag order (every entry alike).
    stream.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    stream.extend_from_slice(&[1; 64]);
    // SOF0: 8-bit samples, 16 lines of 16 samples, one component with
    // sampling factors 1 by 1 using table 0.
    stream.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
    ]);
    stream.extend(huffman_table(0x00, &categories));
    stream.extend(huffman_table(0x10, &[0x00]));
    // SOS: one component using DC table 0 and AC table 0, the full
    // coefficient range, no successive approximation.
    stream.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    let mut bits = Bits {
        out: Vec::new(),
        acc: 0,
        count: 0,
    };
    for &diff in &diffs {
        let category = category(diff);
        let index = categories
            .iter()
            .position(|&c| u32::from(c) == category)
            .expect("category listed");
        // The symbol's code: `index` ones then a zero.
        bits.put((1 << (index + 1)) - 2, index as u32 + 1);
        bits.put(extra(diff, category), category);
        // End of block: the AC table's only code.
        bits.put(0, 1);
    }
    stream.extend(bits.finish());
    stream.extend_from_slice(&[0xFF, 0xD9]);
    stream
}

pub fn bytes() -> Vec<u8> {
    encode(&GREYS)
}

/// Upper-case hexadecimal in lines of 64 digits.
pub fn hex() -> String {
    let digits: String = bytes().iter().map(|b| format!("{b:02X}")).collect();
    digits
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).expect("ascii"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The corpus file: an image whose data source is a `DCTDecode` filter
/// over a hexadecimal one over the job, so the stream stays text.
pub fn corpus_file() -> String {
    let hex = hex();
    let lines = hex.lines().count() + 3;
    format!(
        "%!PS\n\
         % SPDX-FileCopyrightText: 2026 EfterScript contributors\n\
         % SPDX-License-Identifier: MIT\n\
         % Scenario: a 16x16 gray image whose data is a baseline JPEG stream\n\
         % read through currentfile /ASCIIHexDecode filter /DCTDecode filter.\n\
         % The samples are not decoded: the image reads the stream's bytes up\n\
         % to its end-of-image marker, the hexadecimal layer is read through\n\
         % its `>`, and the stream reaches the PDF verbatim under DCTDecode.\n\
         % The four 8x8 blocks are greys {}, {}, {}, and {} in raster order.\n\
         % The stream is generated by `cargo xtask tiny-jpeg --corpus`;\n\
         % regenerate rather than edit.\n\
         100 100 translate\n\
         200 200 scale\n\
         %%BeginData: {lines} ASCII Lines\n\
         << /ImageType 1 /Width 16 /Height 16 /BitsPerComponent 8\n\
         \x20  /ImageMatrix [16 0 0 -16 0 16]\n\
         \x20  /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
         {hex}>\n\
         %%EndData\n\
         showpage\n",
        GREYS[0], GREYS[1], GREYS[2], GREYS[3]
    )
}

pub fn run(args: &[String]) -> ExitCode {
    match args {
        [] => {
            println!("{}", hex());
            ExitCode::SUCCESS
        }
        [flag] if flag == "--corpus" => {
            print!("{}", corpus_file());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: cargo xtask tiny-jpeg [--corpus]");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const CORPUS_FILE: &str = "corpus/unit/filters/dct-image.ps";
    const GOLDEN_PDF: &str = "corpus/golden/pdf/filters/dct-image.pdf";

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    /// The stream worked out by hand for `GREYS`: DC values 0, 256,
    /// 768, 256 give differences 0, 256, 512, -512 in categories 0, 9,
    /// 10, 10, coded 0, 10, 110 with 0, 9, and 10 extra bits each, then
    /// the one-bit end-of-block; 42 bits padded to six bytes, the fifth
    /// being 0xFF and stuffed.
    #[test]
    fn the_fixed_image_is_the_hand_derived_stream() {
        let mut want = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00];
        want.extend_from_slice(&[1; 64]);
        want.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
        ]);
        want.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x16, 0x00, 1, 1, 1]);
        want.extend_from_slice(&[0; 13]);
        want.extend_from_slice(&[0x00, 0x09, 0x0A]);
        want.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x10, 1]);
        want.extend_from_slice(&[0; 15]);
        want.push(0x00);
        want.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        want.extend_from_slice(&[0x28, 0x03, 0x40, 0x0C, 0xFF, 0x00, 0xBF]);
        want.extend_from_slice(&[0xFF, 0xD9]);
        assert_eq!(bytes(), want);
        assert_eq!(bytes().len(), 149);
    }

    #[test]
    fn segment_lengths_add_up_and_the_walker_ends_at_the_last_byte() {
        let stream = bytes();
        // SOI, then segments each of 2 + declared length, up to SOS,
        // whose data runs to EOI.
        let mut at = 2;
        let mut markers = Vec::new();
        loop {
            assert_eq!(stream[at], 0xFF);
            let code = stream[at + 1];
            markers.push(code);
            let length = usize::from(u16::from_be_bytes([stream[at + 2], stream[at + 3]]));
            at += 2 + length;
            if code == 0xDA {
                break;
            }
        }
        assert_eq!(markers, [0xDB, 0xC0, 0xC4, 0xC4, 0xDA]);
        assert_eq!(
            &stream[at..],
            [0x28, 0x03, 0x40, 0x0C, 0xFF, 0x00, 0xBF, 0xFF, 0xD9]
        );
        assert_eq!(
            efterscript_vm::jpeg::stream_len(&stream),
            Some(stream.len())
        );
        let mut longer = stream.clone();
        longer.extend_from_slice(b"after");
        assert_eq!(
            efterscript_vm::jpeg::stream_len(&longer),
            Some(stream.len())
        );
        // Other greys code other categories; the walker still finds the end.
        let other = encode(&[0, 255, 128, 129]);
        assert_eq!(efterscript_vm::jpeg::stream_len(&other), Some(other.len()));
        assert_eq!(category(0), 0);
        assert_eq!(category(-1), 1);
        assert_eq!(category(-512), 10);
        assert_eq!(category(1016), 10);
        assert_eq!(extra(-512, 10), 0b0111111111);
        assert_eq!(extra(256, 9), 0b100000000);
    }

    #[test]
    fn hex_is_wrapped_and_the_corpus_file_is_current() {
        let hex = hex();
        assert_eq!(hex.lines().count(), 5);
        assert!(hex.lines().take(4).all(|l| l.len() == 64));
        assert!(hex.starts_with("FFD8FFDB0043"));
        let path = root().join(CORPUS_FILE);
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run `cargo xtask tiny-jpeg --corpus`",
                path.display()
            )
        });
        assert!(
            committed == corpus_file(),
            "{} differs from the generator's output; rerun \
             `cargo xtask tiny-jpeg --corpus > {CORPUS_FILE}`",
            path.display()
        );
    }

    /// The golden document carries the stream verbatim under the
    /// `DCTDecode` filter.
    #[test]
    fn the_golden_pdf_carries_the_stream_verbatim() {
        let path = root().join(GOLDEN_PDF);
        let pdf = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run `difftest run --update-pdf`", path.display()));
        let dict = b"/Filter /DCTDecode";
        let at = pdf
            .windows(dict.len())
            .position(|w| w == dict)
            .expect("an image XObject with the DCT filter");
        let head = &pdf[..at];
        let key = b"/Length ";
        let length_at = head
            .windows(key.len())
            .rposition(|w| w == key)
            .expect("a Length before the filter");
        let length: usize = std::str::from_utf8(&head[length_at + key.len()..])
            .unwrap()
            .trim_end()
            .parse()
            .expect("the stream length");
        let marker = b"stream\n";
        let start = at
            + pdf[at..]
                .windows(marker.len())
                .position(|w| w == marker)
                .expect("the stream keyword")
            + marker.len();
        assert_eq!(&pdf[start..start + length], bytes());
    }
}
