// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Entry points for the fuzz targets under `fuzz/`, and nothing else: a
//! decode filter driven over bytes without a file table around it, so
//! the decoders behind the `filter` operator can be fed arbitrary input
//! directly. Compiled under the `fuzzing` feature and for the crate's
//! own tests; not part of the crate's API.

use efterscript_codec::predictor::Predictor;

use crate::decoders::{Decoder, Fed};
use crate::error::VmError;

/// The `Predictor`, `Colors`, `BitsPerComponent`, and `Columns` entries
/// of a Flate or LZW parameter dictionary, unchecked: values outside
/// their ranges are `rangecheck` when the filter is built, as the
/// operator raises it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PredictorParams {
    pub predictor: i64,
    pub colors: i64,
    pub bits_per_component: i64,
    pub columns: i64,
}

impl Default for PredictorParams {
    fn default() -> Self {
        PredictorParams {
            predictor: 1,
            colors: 1,
            bits_per_component: 8,
            columns: 1,
        }
    }
}

/// A decode filter as the `filter` operator builds it from a name and
/// its parameters, plus the `eexec` layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Eexec,
    AsciiHex,
    Ascii85,
    RunLength,
    Flate(PredictorParams),
    Lzw {
        early_change: bool,
        predictor: PredictorParams,
    },
    SubFile {
        count: usize,
        pattern: Vec<u8>,
    },
}

fn predictor(params: PredictorParams) -> Result<Option<Predictor>, VmError> {
    Predictor::new(
        params.predictor,
        params.colors,
        params.bits_per_component,
        params.columns,
    )
    .map_err(|_| VmError::RangeCheck)
}

fn decoder(filter: &Filter) -> Result<Decoder, VmError> {
    Ok(match filter {
        Filter::Eexec => Decoder::eexec(),
        Filter::AsciiHex => Decoder::ascii_hex(),
        Filter::Ascii85 => Decoder::ascii85(),
        Filter::RunLength => Decoder::run_length(),
        Filter::Flate(params) => Decoder::flate(predictor(*params)?),
        Filter::Lzw {
            early_change,
            predictor: params,
        } => Decoder::lzw(*early_change, predictor(*params)?),
        Filter::SubFile { count, pattern } => Decoder::sub_file(*count, pattern.clone()),
    })
}

/// Runs `bytes` through `filters` in order, each over the previous
/// one's output, as a stack of layered files would: bytes are fed until
/// the decoder reports its end marker, and a decoder whose base runs
/// dry first is finished, as the file table finishes it. The errors are
/// the operator's: `rangecheck` for parameters out of range, `ioerror`
/// for malformed data.
pub fn decode_chain(filters: &[Filter], bytes: &[u8]) -> Result<Vec<u8>, VmError> {
    let mut data = bytes.to_vec();
    for filter in filters {
        let mut decoder = decoder(filter)?;
        let mut out = Vec::new();
        let mut ended = false;
        for &byte in &data {
            match decoder.push(byte, &mut out)? {
                Fed::More => {}
                Fed::End | Fed::EndBefore => {
                    ended = true;
                    break;
                }
            }
        }
        if !ended {
            decoder.finish(&mut out)?;
        }
        data = out;
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_decodes_each_layer_over_the_previous_one() {
        let chain = [Filter::AsciiHex, Filter::RunLength];
        // Hex of a one-byte literal run (`00 41`) and the end code (`80`).
        assert_eq!(decode_chain(&chain, b"004180> trailing").unwrap(), b"A");
        assert_eq!(
            decode_chain(&[Filter::AsciiHex], b"48656C6C6F").unwrap(),
            b"Hello"
        );
        assert_eq!(decode_chain(&[], b"as is").unwrap(), b"as is");
    }

    #[test]
    fn parameters_out_of_range_are_rangecheck() {
        let params = PredictorParams {
            predictor: 3,
            ..PredictorParams::default()
        };
        assert_eq!(
            decode_chain(&[Filter::Flate(params)], b"x"),
            Err(VmError::RangeCheck)
        );
        let sub = Filter::SubFile {
            count: 0,
            pattern: b"END".to_vec(),
        };
        assert_eq!(decode_chain(&[sub], b"keep END drop").unwrap(), b"keep ");
    }
}
