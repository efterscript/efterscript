// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Stream objects (ISO 32000-1 §7.3.8): the data is encoded into a buffer
//! first, so the dictionary's `Length` is the exact encoded byte count and
//! the file is written in one pass. `Length` excludes the end-of-line bytes
//! framing the data.

use std::borrow::Cow;

use crate::obj::DictBuilder;

/// Stream encoding. `Flate` emits a zlib container holding a real DEFLATE
/// stream (see `crate::flate`); `Dct` declares data that already is a
/// JPEG stream and writes it as given; the variant set is the stable API
/// surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    Flate,
    Dct,
}

impl Filter {
    fn name(self) -> Option<&'static str> {
        match self {
            Filter::None => None,
            Filter::Flate => Some("FlateDecode"),
            Filter::Dct => Some("DCTDecode"),
        }
    }
}

/// Serializes a whole stream object body (dict, `stream`…`endstream`) into
/// `body`. Extra dictionary entries come after `Length` and `Filter`.
/// Returns false iff a dictionary key or name in `extra` contained NUL.
#[must_use]
pub(crate) fn put_stream(
    body: &mut Vec<u8>,
    filter: Filter,
    data: &[u8],
    extra: impl FnOnce(&mut DictBuilder<'_>),
) -> bool {
    let mut bad_name = false;
    let encoded: Cow<'_, [u8]> = match filter {
        Filter::None | Filter::Dct => Cow::Borrowed(data),
        Filter::Flate => Cow::Owned(crate::flate::compress(data)),
    };
    body.extend_from_slice(b"<<");
    {
        let mut d = DictBuilder::new(body, &mut bad_name);
        d.key("Length").int(encoded.len() as i64);
        if let Some(name) = filter.name() {
            d.key("Filter").name(name);
        }
        extra(&mut d);
    }
    body.extend_from_slice(b" >>\nstream\n");
    body.extend_from_slice(&encoded);
    body.extend_from_slice(b"\nendstream");
    !bad_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncompressed_stream_layout() {
        let mut body = Vec::new();
        assert!(put_stream(&mut body, Filter::None, b"BT ET", |_| {}));
        assert_eq!(body, b"<< /Length 5 >>\nstream\nBT ET\nendstream");
    }

    #[test]
    fn flate_stream_declares_filter_and_exact_length() {
        let mut body = Vec::new();
        assert!(put_stream(&mut body, Filter::Flate, b"xy", |d| {
            d.key("Type").name("XObject");
        }));
        let encoded = crate::flate::compress(b"xy");
        let head = format!(
            "<< /Length {} /Filter /FlateDecode /Type /XObject >>\nstream\n",
            encoded.len()
        );
        assert!(body.starts_with(head.as_bytes()));
        let data = &body[head.len()..body.len() - b"\nendstream".len()];
        assert_eq!(data, encoded);
    }

    #[test]
    fn dct_stream_names_the_filter_and_keeps_the_bytes() {
        let mut body = Vec::new();
        assert!(put_stream(
            &mut body,
            Filter::Dct,
            b"\xFF\xD8\xFF\xD9",
            |_| {}
        ));
        assert_eq!(
            body,
            b"<< /Length 4 /Filter /DCTDecode >>\nstream\n\xFF\xD8\xFF\xD9\nendstream"
        );
    }
}
