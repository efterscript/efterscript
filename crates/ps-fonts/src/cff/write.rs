// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The pieces a CFF program is assembled from — indexes, dictionary
//! operands and operators, charsets, encodings, and select tables — and
//! the subset writers built on them: [`subset`] writes a name-keyed
//! program holding the kept glyphs, a charset naming them, no encoding,
//! and only the subroutines those glyphs reach, renumbered densely with
//! every call's operand re-encoded against the new bias; [`subset_cid`]
//! does the same for a CID-keyed program, keeping only the font
//! dictionaries the kept glyphs run under, each with its own pruned
//! local subroutines, and the global subroutines pruned across all of
//! them. Offsets that are only known after layout are written in the
//! fixed five-byte form so a dictionary's size does not depend on their
//! values. Where a call's operand is not a literal the rewrite can see,
//! the numbering stays and the unreached subroutines are written as
//! `return` stubs instead.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::charstring::{Token, bias, encode, tokens_with_masks};
use super::{CffProgram, DictOp, PrivateDict, Privates, Reached, STANDARD_STRINGS, Site, esc, op};
use crate::program::FontError;

/// An INDEX over `items`: count, offset size, offsets, data.
pub fn index(items: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(items.len()).expect("index fits");
    let mut out = count.to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    let total: usize = items.iter().map(Vec::len).sum();
    let off_size = offset_size(total + 1);
    out.push(off_size);
    let mut offset = 1usize;
    push_offset(&mut out, offset, off_size);
    for item in items {
        offset += item.len();
        push_offset(&mut out, offset, off_size);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// The smallest offset size holding `value`.
pub fn offset_size(value: usize) -> u8 {
    match value {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    }
}

fn push_offset(out: &mut Vec<u8>, value: usize, size: u8) {
    let bytes = (value as u32).to_be_bytes();
    out.extend_from_slice(&bytes[4 - usize::from(size)..]);
}

/// An integer operand in its shortest dictionary form.
pub fn dict_int(v: i32, out: &mut Vec<u8>) {
    match v {
        -107..=107 => out.push((v + 139) as u8),
        108..=1131 => {
            let w = v - 108;
            out.push((w / 256 + 247) as u8);
            out.push((w % 256) as u8);
        }
        -1131..=-108 => {
            let w = -v - 108;
            out.push((w / 256 + 251) as u8);
            out.push((w % 256) as u8);
        }
        -32768..=32767 => {
            out.push(28);
            out.extend_from_slice(&(v as i16).to_be_bytes());
        }
        _ => dict_int5(v, out),
    }
}

/// An integer operand in the five-byte form, whatever its value.
pub fn dict_int5(v: i32, out: &mut Vec<u8>) {
    out.push(29);
    out.extend_from_slice(&v.to_be_bytes());
}

/// A real operand in nibble form.
pub fn dict_real(v: f64, out: &mut Vec<u8>) {
    let text = format!("{v}");
    let mut nibbles = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '0'..='9' => nibbles.push(c as u8 - b'0'),
            '.' => nibbles.push(0xa),
            'e' | 'E' => {
                if chars.peek() == Some(&'-') {
                    chars.next();
                    nibbles.push(0xc);
                } else {
                    nibbles.push(0xb);
                }
            }
            '-' => nibbles.push(0xe),
            _ => {}
        }
    }
    nibbles.push(0xf);
    if !nibbles.len().is_multiple_of(2) {
        nibbles.push(0xf);
    }
    out.push(30);
    for pair in nibbles.chunks(2) {
        out.push(pair[0] << 4 | pair[1]);
    }
}

/// A number operand: an integer when integral and in range, else a real.
pub fn dict_number(v: f64, out: &mut Vec<u8>) {
    if v.fract() == 0.0 && v.abs() < f64::from(i32::MAX) {
        dict_int(v as i32, out);
    } else {
        dict_real(v, out);
    }
}

/// A dictionary operator.
pub fn dict_op(op: DictOp, out: &mut Vec<u8>) {
    if op & 0x0c00 == esc(0) {
        out.push(12);
        out.push((op & 0xff) as u8);
    } else {
        out.push(op as u8);
    }
}

/// A dictionary assembled entry by entry.
#[derive(Clone, Debug, Default)]
pub struct DictWriter {
    pub bytes: Vec<u8>,
}

impl DictWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Operands in their shortest forms, then the operator.
    pub fn entry(&mut self, op: DictOp, operands: &[f64]) -> &mut Self {
        for &v in operands {
            dict_number(v, &mut self.bytes);
        }
        dict_op(op, &mut self.bytes);
        self
    }

    /// Integer operands in the five-byte form, then the operator: for
    /// offsets filled in after layout.
    pub fn fixed(&mut self, op: DictOp, operands: &[i32]) -> &mut Self {
        for &v in operands {
            dict_int5(v, &mut self.bytes);
        }
        dict_op(op, &mut self.bytes);
        self
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// A charset in format 0: the string ids (or CIDs) of glyphs 1 onward.
pub fn charset_format0(ids: &[u16]) -> Vec<u8> {
    let mut out = vec![0u8];
    for &id in ids.iter().skip(1) {
        out.extend_from_slice(&id.to_be_bytes());
    }
    out
}

/// A custom encoding in format 0 with no codes of its own and one
/// supplement per `(code, string id)`: every code names its glyph, so
/// glyphs need not be in code order.
pub fn encoding_supplements(codes: &[(u8, u16)]) -> Vec<u8> {
    let mut out = vec![0x80, 0, codes.len() as u8];
    for &(code, sid) in codes {
        out.push(code);
        out.extend_from_slice(&sid.to_be_bytes());
    }
    out
}

/// A select table in format 3 from the font dictionary index of each
/// glyph.
pub fn fd_select_format3(select: &[u8]) -> Vec<u8> {
    let mut ranges: Vec<(u16, u8)> = Vec::new();
    for (gid, &fd) in select.iter().enumerate() {
        if ranges.last().is_none_or(|&(_, last)| last != fd) {
            ranges.push((gid as u16, fd));
        }
    }
    let mut out = vec![3u8];
    out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
    for (first, fd) in ranges {
        out.extend_from_slice(&first.to_be_bytes());
        out.push(fd);
    }
    out.extend_from_slice(&(select.len() as u16).to_be_bytes());
    out
}

/// The four-byte header of a version 1 program with the given absolute
/// offset size.
pub fn header(off_size: u8) -> [u8; 4] {
    [1, 0, 4, off_size]
}

// --- subsets ------------------------------------------------------------------------------

/// `return`: the stub written for a subroutine no kept charstring reaches
/// when the numbering must stay.
const RETURN: [u8; 1] = [11];

/// The top-dictionary entries a subset copies as they are, when present.
const COPIED_TOP_ENTRIES: [DictOp; 8] = [
    op::FONT_BBOX,
    op::IS_FIXED_PITCH,
    op::ITALIC_ANGLE,
    op::UNDERLINE_POSITION,
    op::UNDERLINE_THICKNESS,
    op::PAINT_TYPE,
    op::STROKE_WIDTH,
    op::FONT_MATRIX,
];

/// The string-valued top-dictionary entries a subset carries over, their
/// strings re-interned.
const COPIED_TOP_STRINGS: [DictOp; 6] = [
    op::VERSION,
    op::NOTICE,
    op::COPYRIGHT,
    op::FULL_NAME,
    op::FAMILY_NAME,
    op::WEIGHT,
];

/// The glyphs a subset keeps for the names in `used`: those the program
/// has, `.notdef`, and the components of every accented glyph among them.
pub fn subset_names<'a>(
    program: &CffProgram,
    used: impl IntoIterator<Item = &'a [u8]>,
) -> BTreeSet<Vec<u8>> {
    let mut keep = BTreeSet::new();
    keep.insert(b".notdef".to_vec());
    for name in used {
        let Some(gid) = program.gid(name) else {
            continue;
        };
        keep.insert(name.to_vec());
        for component in program.components(gid).unwrap_or_default() {
            if let Some(name) = program.glyph_name(component) {
                keep.insert(name.to_vec());
            }
        }
    }
    keep
}

/// The string ids of a subset: standard strings by position, the
/// program's own after them in order of first use.
struct Strings {
    standard: HashMap<&'static [u8], u16>,
    own: Vec<Vec<u8>>,
}

impl Strings {
    fn new() -> Self {
        Strings {
            standard: STANDARD_STRINGS
                .iter()
                .enumerate()
                .map(|(k, s)| (s.as_bytes(), k as u16))
                .collect(),
            own: Vec::new(),
        }
    }

    fn sid(&mut self, text: &[u8]) -> u16 {
        if let Some(&sid) = self.standard.get(text) {
            return sid;
        }
        let k = match self.own.iter().position(|s| s == text) {
            Some(k) => k,
            None => {
                self.own.push(text.to_vec());
                self.own.len() - 1
            }
        };
        (STANDARD_STRINGS.len() + k) as u16
    }
}

/// A subset's subroutines and charstrings, pruned one way or the other.
struct Pruned {
    local: Vec<Vec<u8>>,
    global: Vec<Vec<u8>>,
    charstrings: Vec<Vec<u8>>,
}

/// The union of what the kept glyphs reach; `None` when a kept
/// charstring cannot be interpreted, since its reach is then unknown. An
/// empty charstring reaches nothing.
fn trace(program: &CffProgram, gids: &BTreeSet<u16>) -> Option<Reached> {
    let mut all = Reached::default();
    for &gid in gids {
        if program.charstring(gid).ok()?.is_empty() {
            continue;
        }
        let reached = program.reached_subrs(gid).ok()?;
        all.local.extend(reached.local);
        all.global.extend(reached.global);
        all.masks.extend(reached.masks);
    }
    Some(all)
}

/// The mask byte counts the trace recorded in the charstring at `site`,
/// by offset; `None` when two runs disagreed on one, which no rewrite
/// can honour.
fn mask_lengths(reached: &Reached, site: Site) -> Option<BTreeMap<usize, usize>> {
    let mut lengths = BTreeMap::new();
    for &(at_site, offset, len) in &reached.masks {
        if at_site == site
            && lengths
                .insert(offset, len)
                .is_some_and(|prior| prior != len)
        {
            return None;
        }
    }
    Some(lengths)
}

/// Old index to new for each subroutine kind, with the bias before and
/// after.
struct Numbering {
    local: BTreeMap<usize, usize>,
    global: BTreeMap<usize, usize>,
    local_bias: (i32, i32),
    global_bias: (i32, i32),
}

/// Rewrites the literal operand of every `callsubr` and `callgsubr` in
/// one charstring through the numbering. `None` when an operand is not
/// a literal the rewrite can see, or names a subroutine outside the map.
fn renumber_charstring(
    code: &[u8],
    masks: &BTreeMap<usize, usize>,
    numbering: &Numbering,
) -> Option<Vec<u8>> {
    let mut tokens = tokens_with_masks(code, |at| masks.get(&at).copied()).ok()?;
    for at in 0..tokens.len() {
        let (map, (old_bias, new_bias)) = match tokens[at] {
            Token::Op(10) => (&numbering.local, numbering.local_bias),
            Token::Op(29) => (&numbering.global, numbering.global_bias),
            _ => continue,
        };
        let Token::Num(operand) = *tokens.get(at.wrapping_sub(1))? else {
            return None;
        };
        if operand.fract() != 0.0 {
            return None;
        }
        let old = usize::try_from(operand as i32 + old_bias).ok()?;
        let new = i32::try_from(*map.get(&old)?).ok()?;
        tokens[at - 1] = Token::Num((new - new_bias) as f32);
    }
    Some(encode(&tokens))
}

/// The reached subroutines renumbered densely in index order and every
/// kept charstring and subroutine rewritten to the new numbering and
/// bias; `None` when a call cannot be rewritten.
fn renumber(
    program: &CffProgram,
    private: &PrivateDict,
    gids: &BTreeSet<u16>,
    reached: &Reached,
) -> Option<Pruned> {
    let dense = |keep: &BTreeSet<usize>| -> BTreeMap<usize, usize> {
        keep.iter()
            .enumerate()
            .map(|(new, &old)| (old, new))
            .collect()
    };
    let numbering = Numbering {
        local: dense(&reached.local),
        global: dense(&reached.global),
        local_bias: (bias(private.subrs.len()), bias(reached.local.len())),
        global_bias: (
            bias(program.global_subrs().len()),
            bias(reached.global.len()),
        ),
    };
    let rewrite = |code: &[u8], site: Site| -> Option<Vec<u8>> {
        renumber_charstring(code, &mask_lengths(reached, site)?, &numbering)
    };
    let local = reached
        .local
        .iter()
        .map(|&k| rewrite(private.subrs.get(k)?, Site::Local(k)))
        .collect::<Option<Vec<_>>>()?;
    let global = reached
        .global
        .iter()
        .map(|&k| rewrite(program.global_subrs().get(k)?, Site::Global(k)))
        .collect::<Option<Vec<_>>>()?;
    let charstrings = gids
        .iter()
        .map(|&gid| rewrite(program.charstring(gid).ok()?, Site::Glyph(gid)))
        .collect::<Option<Vec<_>>>()?;
    Some(Pruned {
        local,
        global,
        charstrings,
    })
}

/// `subrs` with every index outside `keep` replaced by the `return`
/// stub; the count, and so the bias, is unchanged.
fn stubbed(subrs: &[Vec<u8>], keep: &BTreeSet<usize>) -> Vec<Vec<u8>> {
    subrs
        .iter()
        .enumerate()
        .map(|(k, code)| {
            if keep.contains(&k) {
                code.clone()
            } else {
                RETURN.to_vec()
            }
        })
        .collect()
}

/// The subset's subroutines and charstrings: renumbered when every call
/// is a literal, stubbed when one is not, everything kept as it is when
/// a kept glyph cannot be interpreted.
fn prune(program: &CffProgram, private: &PrivateDict, gids: &BTreeSet<u16>) -> Pruned {
    let verbatim = || -> Vec<Vec<u8>> {
        gids.iter()
            .map(|&gid| {
                program
                    .charstring(gid)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default()
            })
            .collect()
    };
    let Some(reached) = trace(program, gids) else {
        return Pruned {
            local: private.subrs.clone(),
            global: program.global_subrs().to_vec(),
            charstrings: verbatim(),
        };
    };
    renumber(program, private, gids, &reached).unwrap_or_else(|| Pruned {
        local: stubbed(&private.subrs, &reached.local),
        global: stubbed(program.global_subrs(), &reached.global),
        charstrings: verbatim(),
    })
}

/// A name-keyed program holding `glyphs` (see [`subset_names`]) under
/// `font_name`: header, name index, top dictionary, string index, the
/// reached global subroutines, a format 0 charset, no encoding, the kept
/// charstrings, and the private dictionary with its reached local
/// subroutines, all renumbered (see the module notes). A CID-keyed
/// program is `Unsupported`.
pub fn subset(
    program: &CffProgram,
    font_name: &[u8],
    glyphs: &BTreeSet<Vec<u8>>,
) -> Result<Vec<u8>, FontError> {
    let private = program
        .private()
        .ok_or(FontError::Unsupported("CID-keyed subset"))?;
    let mut gids: BTreeSet<u16> = [0].into_iter().collect();
    gids.extend(glyphs.iter().filter_map(|name| program.gid(name)));
    let pruned = prune(program, private, &gids);

    let mut strings = Strings::new();
    let top_strings: Vec<(DictOp, u16)> = COPIED_TOP_STRINGS
        .iter()
        .filter_map(|&entry| Some((entry, strings.sid(program.top_string(entry)?))))
        .collect();
    let charset_ids: Vec<u16> = gids
        .iter()
        .map(|&gid| strings.sid(program.glyph_name(gid).unwrap_or(b".notdef")))
        .collect();

    let top = |charset: i32, charstrings: i32, private: (i32, i32)| -> Vec<u8> {
        let mut w = DictWriter::new();
        for &(entry, sid) in &top_strings {
            w.entry(entry, &[f64::from(sid)]);
        }
        for entry in COPIED_TOP_ENTRIES {
            if let Some(operands) = program.top().get(entry) {
                w.entry(entry, operands);
            }
        }
        w.fixed(op::CHARSET, &[charset]);
        w.fixed(op::CHARSTRINGS, &[charstrings]);
        w.fixed(op::PRIVATE, &[private.0, private.1]);
        w.bytes
    };
    let mut private_dict = DictWriter::new();
    for (entry, operands) in &private.dict.entries {
        private_dict.entry(*entry, operands);
    }
    let local_index = if pruned.local.is_empty() {
        Vec::new()
    } else {
        let at = private_dict.len() + 6;
        private_dict.fixed(op::SUBRS, &[at as i32]);
        index(&pruned.local)
    };

    let name_index = index(&[font_name.to_vec()]);
    let top_len = index(&[top(0, 0, (0, 0))]).len();
    let string_index = index(&strings.own);
    let global_index = index(&pruned.global);
    let charset = charset_format0(&charset_ids);
    let charstrings_index = index(&pruned.charstrings);
    let charset_at = 4 + name_index.len() + top_len + string_index.len() + global_index.len();
    let charstrings_at = charset_at + charset.len();
    let private_at = charstrings_at + charstrings_index.len();
    let top = top(
        charset_at as i32,
        charstrings_at as i32,
        (private_dict.len() as i32, private_at as i32),
    );
    let total = private_at + private_dict.len() + local_index.len();
    let mut out = header(offset_size(total)).to_vec();
    out.extend(name_index);
    out.extend(index(&[top]));
    out.extend(string_index);
    out.extend(global_index);
    out.extend(charset);
    out.extend(charstrings_index);
    out.extend(private_dict.bytes);
    out.extend(local_index);
    Ok(out)
}

// --- CID-keyed subsets --------------------------------------------------------------------

/// A CID-keyed subset's charstrings and subroutines: the global ones,
/// the local ones of each kept font dictionary in order, and the kept
/// charstrings in glyph order.
struct CidPruned {
    global: Vec<Vec<u8>>,
    locals: Vec<Vec<Vec<u8>>>,
    charstrings: Vec<Vec<u8>>,
}

/// The kept glyphs grouped by the font dictionary they run under, in
/// dictionary order: `(old dictionary index, its glyphs)`.
fn glyphs_by_fd(select: &[u8], gids: &BTreeSet<u16>) -> Vec<(u8, BTreeSet<u16>)> {
    let mut by_fd: BTreeMap<u8, BTreeSet<u16>> = BTreeMap::new();
    for &gid in gids {
        let fd = select.get(usize::from(gid)).copied().unwrap_or(0);
        by_fd.entry(fd).or_default().insert(gid);
    }
    by_fd.into_iter().collect()
}

/// Every kept charstring of every dictionary rewritten to the dense
/// numbering: each dictionary's local subroutines by its own trace, the
/// global ones by the union. A global subroutine may call a local one
/// only when a single dictionary is kept, since the numbering it would
/// need is the caller's; otherwise its call cannot be rewritten and the
/// caller falls back to stubs. `None` when any call cannot be rewritten.
fn renumber_cid(
    program: &CffProgram,
    dicts: &[PrivateDict],
    groups: &[(u8, BTreeSet<u16>)],
    traces: &[Reached],
) -> Option<CidPruned> {
    let dense = |keep: &BTreeSet<usize>| -> BTreeMap<usize, usize> {
        keep.iter()
            .enumerate()
            .map(|(new, &old)| (old, new))
            .collect()
    };
    let mut global_reached = BTreeSet::new();
    let mut global_masks = Reached::default();
    for reached in traces {
        global_reached.extend(reached.global.iter().copied());
        global_masks.masks.extend(
            reached
                .masks
                .iter()
                .filter(|(site, _, _)| matches!(site, Site::Global(_)))
                .copied(),
        );
    }
    let global_map = dense(&global_reached);
    let global_bias = (
        bias(program.global_subrs().len()),
        bias(global_reached.len()),
    );
    let numbering_for = |fd: u8, reached: &Reached| Numbering {
        local: dense(&reached.local),
        global: global_map.clone(),
        local_bias: (
            bias(dicts[usize::from(fd)].subrs.len()),
            bias(reached.local.len()),
        ),
        global_bias,
    };
    let mut locals = Vec::with_capacity(groups.len());
    let mut charstrings = Vec::new();
    for ((fd, gids), reached) in groups.iter().zip(traces) {
        let numbering = numbering_for(*fd, reached);
        let private = &dicts[usize::from(*fd)];
        let rewrite = |code: &[u8], site: Site| -> Option<Vec<u8>> {
            renumber_charstring(code, &mask_lengths(reached, site)?, &numbering)
        };
        locals.push(
            reached
                .local
                .iter()
                .map(|&k| rewrite(private.subrs.get(k)?, Site::Local(k)))
                .collect::<Option<Vec<_>>>()?,
        );
        for &gid in gids {
            charstrings.push(rewrite(program.charstring(gid).ok()?, Site::Glyph(gid))?);
        }
    }
    let global_numbering = match (groups, traces) {
        ([(fd, _)], [reached]) => numbering_for(*fd, reached),
        _ => Numbering {
            local: BTreeMap::new(),
            global: global_map.clone(),
            local_bias: (0, 0),
            global_bias,
        },
    };
    let global = global_reached
        .iter()
        .map(|&k| {
            renumber_charstring(
                program.global_subrs().get(k)?,
                &mask_lengths(&global_masks, Site::Global(k))?,
                &global_numbering,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CidPruned {
        global,
        locals,
        charstrings,
    })
}

/// The subset's subroutines and charstrings, tier by tier as for a
/// name-keyed program (see [`prune`]), with the local subroutines
/// handled per kept font dictionary.
fn prune_cid(
    program: &CffProgram,
    dicts: &[PrivateDict],
    groups: &[(u8, BTreeSet<u16>)],
) -> CidPruned {
    let verbatim = || -> Vec<Vec<u8>> {
        groups
            .iter()
            .flat_map(|(_, gids)| gids.iter())
            .map(|&gid| {
                program
                    .charstring(gid)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default()
            })
            .collect()
    };
    let traces: Option<Vec<Reached>> = groups
        .iter()
        .map(|(_, gids)| trace(program, gids))
        .collect();
    let Some(traces) = traces else {
        return CidPruned {
            global: program.global_subrs().to_vec(),
            locals: groups
                .iter()
                .map(|(fd, _)| dicts[usize::from(*fd)].subrs.clone())
                .collect(),
            charstrings: verbatim(),
        };
    };
    renumber_cid(program, dicts, groups, &traces).unwrap_or_else(|| {
        let mut global_reached = BTreeSet::new();
        for reached in &traces {
            global_reached.extend(reached.global.iter().copied());
        }
        CidPruned {
            global: stubbed(program.global_subrs(), &global_reached),
            locals: groups
                .iter()
                .zip(&traces)
                .map(|((fd, _), reached)| stubbed(&dicts[usize::from(*fd)].subrs, &reached.local))
                .collect(),
            charstrings: verbatim(),
        }
    })
}

/// A CID-keyed program holding the glyphs of `cids` and CID 0 under
/// `font_name`: header, name index, top dictionary (`ROS` first, the
/// copied entries, `CIDCount`, and the offsets), string index, the
/// reached global subroutines, a format 0 charset of the kept CIDs, a
/// format 3 select table, the kept charstrings, the font dictionary
/// array restricted to the dictionaries the kept glyphs run under, and
/// each of those dictionaries' private data with its reached local
/// subroutines (see the module notes). A name-keyed program is
/// `Unsupported`.
pub fn subset_cid(
    program: &CffProgram,
    font_name: &[u8],
    cids: &BTreeSet<u16>,
) -> Result<Vec<u8>, FontError> {
    let Privates::Cid { dicts, select } = program.privates() else {
        return Err(FontError::Unsupported("name-keyed program as a CID subset"));
    };
    let ros = program
        .ros()
        .ok_or(FontError::Malformed("registry, ordering, supplement"))?;
    let mut gids: BTreeSet<u16> = [0].into_iter().collect();
    gids.extend(cids.iter().filter_map(|&cid| program.gid_of_cid(cid)));
    let groups = glyphs_by_fd(select, &gids);
    let pruned = prune_cid(program, dicts, &groups);

    // Glyphs are written in old index order; each carries its CID and
    // the new index of its dictionary.
    let mut glyphs: Vec<(u16, u8, Vec<u8>)> = Vec::with_capacity(gids.len());
    let mut charstrings = pruned.charstrings.into_iter();
    for (new_fd, (_, fd_gids)) in groups.iter().enumerate() {
        for &gid in fd_gids {
            let code = charstrings.next().expect("one charstring per kept glyph");
            glyphs.push((program.charset()[usize::from(gid)], new_fd as u8, code));
        }
    }
    glyphs.sort_by_key(|(cid, _, _)| *cid);
    let charset_ids: Vec<u16> = glyphs.iter().map(|(cid, _, _)| *cid).collect();
    let fd_select = fd_select_format3(&glyphs.iter().map(|(_, fd, _)| *fd).collect::<Vec<_>>());
    let charstrings: Vec<Vec<u8>> = glyphs.into_iter().map(|(_, _, code)| code).collect();

    let mut strings = Strings::new();
    let registry = strings.sid(&ros.registry);
    let ordering = strings.sid(&ros.ordering);
    let top_strings: Vec<(DictOp, u16)> = COPIED_TOP_STRINGS
        .iter()
        .filter_map(|&entry| Some((entry, strings.sid(program.top_string(entry)?))))
        .collect();
    let top = |charset: i32, fd_select: i32, charstrings: i32, fd_array: i32| -> Vec<u8> {
        let mut w = DictWriter::new();
        w.entry(
            op::ROS,
            &[
                f64::from(registry),
                f64::from(ordering),
                f64::from(ros.supplement),
            ],
        );
        for &(entry, sid) in &top_strings {
            w.entry(entry, &[f64::from(sid)]);
        }
        for entry in COPIED_TOP_ENTRIES {
            if let Some(operands) = program.top().get(entry) {
                w.entry(entry, operands);
            }
        }
        w.entry(op::CID_COUNT, &[f64::from(program.cid_count())]);
        w.fixed(op::CHARSET, &[charset]);
        w.fixed(op::FD_SELECT, &[fd_select]);
        w.fixed(op::CHARSTRINGS, &[charstrings]);
        w.fixed(op::FD_ARRAY, &[fd_array]);
        w.bytes
    };

    // Each kept dictionary's private data: the entries, a `Subrs` offset
    // when local subroutines survive, and the subroutine index after it.
    let mut privates: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(groups.len());
    for ((fd, _), local) in groups.iter().zip(&pruned.locals) {
        let mut dict = DictWriter::new();
        for (entry, operands) in &dicts[usize::from(*fd)].dict.entries {
            dict.entry(*entry, operands);
        }
        let index = if local.is_empty() {
            Vec::new()
        } else {
            let at = dict.len() + 6;
            dict.fixed(op::SUBRS, &[at as i32]);
            index(local)
        };
        privates.push((dict.bytes, index));
    }
    let fd_dict = |size: i32, offset: i32| -> Vec<u8> {
        let mut w = DictWriter::new();
        w.fixed(op::PRIVATE, &[size, offset]);
        w.bytes
    };

    let name_index = index(&[font_name.to_vec()]);
    let top_len = index(&[top(0, 0, 0, 0)]).len();
    let string_index = index(&strings.own);
    let global_index = index(&pruned.global);
    let charset = charset_format0(&charset_ids);
    let charstrings_index = index(&charstrings);
    let charset_at = 4 + name_index.len() + top_len + string_index.len() + global_index.len();
    let fd_select_at = charset_at + charset.len();
    let charstrings_at = fd_select_at + fd_select.len();
    let fd_array_at = charstrings_at + charstrings_index.len();
    let fd_array_len = index(&privates.iter().map(|_| fd_dict(0, 0)).collect::<Vec<_>>()).len();
    let mut at = fd_array_at + fd_array_len;
    let mut fd_dicts = Vec::with_capacity(privates.len());
    for (dict, subrs) in &privates {
        fd_dicts.push(fd_dict(dict.len() as i32, at as i32));
        at += dict.len() + subrs.len();
    }
    let top = top(
        charset_at as i32,
        fd_select_at as i32,
        charstrings_at as i32,
        fd_array_at as i32,
    );
    let mut out = header(offset_size(at)).to_vec();
    out.extend(name_index);
    out.extend(index(&[top]));
    out.extend(string_index);
    out.extend(global_index);
    out.extend(charset);
    out.extend(fd_select);
    out.extend(charstrings_index);
    out.extend(index(&fd_dicts));
    for (dict, subrs) in privates {
        out.extend(dict);
        out.extend(subrs);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cff::charstring::tokens;
    use crate::cff::{Dict, op};
    use crate::outline::Glyph;
    use crate::testing::{CffFd, CffFont, Type2Builder, corpus_cff, rectangle};

    #[test]
    fn operands_take_their_shortest_forms_and_read_back() {
        let mut out = Vec::new();
        for v in [
            0.0, 107.0, -107.0, 108.0, 1131.0, -108.0, -1131.0, 1132.0, -32768.0, 40000.0,
        ] {
            dict_number(v, &mut out);
        }
        dict_real(-2.5e-3, &mut out);
        dict_real(0.001, &mut out);
        dict_int5(7, &mut out);
        dict_op(op::FONT_BBOX, &mut out);
        let dict = Dict::parse(&out).unwrap();
        assert_eq!(
            dict.get(op::FONT_BBOX).unwrap(),
            &[
                0.0, 107.0, -107.0, 108.0, 1131.0, -108.0, -1131.0, 1132.0, -32768.0, 40000.0,
                -2.5e-3, 0.001, 7.0
            ]
        );
        let mut w = DictWriter::new();
        w.entry(op::ROS, &[1.0, 2.0, 0.0])
            .fixed(op::CHARSET, &[300]);
        assert!(!w.is_empty());
        assert_eq!(w.len(), 3 + 2 + 5 + 1);
        let dict = Dict::parse(&w.bytes).unwrap();
        assert_eq!(dict.get(op::ROS).unwrap(), &[1.0, 2.0, 0.0]);
        assert_eq!(dict.number(op::CHARSET), Some(300.0));
    }

    #[test]
    fn offsets_and_tables_take_their_documented_shapes() {
        assert_eq!(offset_size(255), 1);
        assert_eq!(offset_size(256), 2);
        assert_eq!(offset_size(70000), 3);
        assert_eq!(offset_size(1 << 24), 4);
        assert_eq!(charset_format0(&[0, 5, 6]), vec![0, 0, 5, 0, 6]);
        assert_eq!(
            encoding_supplements(&[(97, 5), (98, 6)]),
            vec![0x80, 0, 2, 97, 0, 5, 98, 0, 6]
        );
        assert_eq!(
            fd_select_format3(&[0, 0, 1, 1, 0]),
            vec![3, 0, 3, 0, 0, 0, 0, 2, 1, 0, 4, 0, 0, 5]
        );
        assert_eq!(header(1), [1, 0, 4, 1]);
        let big = index(&[vec![0u8; 70000]]);
        assert_eq!(big[2], 3);
    }

    fn names(list: &[&str]) -> BTreeSet<Vec<u8>> {
        list.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    fn set(list: &[usize]) -> BTreeSet<usize> {
        list.iter().copied().collect()
    }

    fn glyph(program: &CffProgram, name: &str) -> Glyph {
        (*program.glyph(name.as_bytes()).unwrap().unwrap()).clone()
    }

    /// The subset of `font` keeping `used`, parsed back; every kept glyph
    /// measures and outlines as the original.
    fn round_trip(font: &CffFont, used: &[&str]) -> (CffProgram, CffProgram, Vec<u8>) {
        let program = CffProgram::parse(&font.build()).unwrap();
        let keep = subset_names(&program, used.iter().map(|n| n.as_bytes()));
        let bytes = subset(&program, b"ABCDEF+Sub", &keep).unwrap();
        let again = CffProgram::parse(&bytes).unwrap();
        assert_eq!(again.name(), b"ABCDEF+Sub");
        assert_eq!(again.glyph_count() as usize, keep.len());
        assert!(again.has_standard_encoding(), "no encoding is written");
        for name in &keep {
            assert!(
                again.gid(name).is_some(),
                "{}",
                String::from_utf8_lossy(name)
            );
            let name = std::str::from_utf8(name).unwrap();
            assert_eq!(glyph(&again, name), glyph(&program, name), "{name}");
        }
        (program, again, bytes)
    }

    #[test]
    fn subsets_keep_notdef_and_accent_components() {
        let font = corpus_cff()
            .charstring(
                "e",
                Type2Builder::new()
                    .num(0)
                    .rmoveto(0, 0)
                    .rlineto(400, 0)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "acute",
                Type2Builder::new()
                    .num(-200)
                    .rmoveto(0, 500)
                    .rlineto(100, 100)
                    .endchar()
                    .bytes(),
            )
            .charstring(
                "eacute",
                Type2Builder::new()
                    .num(0)
                    .num(150)
                    .num(20)
                    .num(101)
                    .num(194)
                    .endchar()
                    .bytes(),
            );
        let program = CffProgram::parse(&font.build()).unwrap();
        assert_eq!(
            subset_names(&program, [&b"a"[..], b"zz"]),
            names(&[".notdef", "a"])
        );
        assert_eq!(
            subset_names(&program, [&b"eacute"[..]]),
            names(&[".notdef", "acute", "e", "eacute"])
        );
        assert_eq!(subset_names(&program, []), names(&[".notdef"]));
        let (_, again, _) = round_trip(&font, &["eacute"]);
        assert_eq!(
            again.charset_names(),
            vec![&b".notdef"[..], b"e", b"acute", b"eacute"]
        );
    }

    #[test]
    fn the_corpus_font_round_trips_with_only_the_reached_subroutines() {
        let font = corpus_cff();
        // `a` reaches nothing: no subroutines of either kind survive.
        let (program, again, bytes) = round_trip(&font, &["a"]);
        assert!(again.private().unwrap().subrs.is_empty());
        assert!(again.global_subrs().is_empty());
        assert_eq!(again.charset(), &[0, 66]);
        assert_eq!(again.font_bbox(), program.font_bbox());
        assert!(!again.has_font_matrix());
        assert_eq!(again.private().unwrap().number(op::STD_VW), Some(80.0));
        assert_eq!(again.private().unwrap().nominal_width_x, 500.0);
        assert!(again.strings().is_empty());
        assert!(bytes.len() < font.build().len());
        // `b` reaches local 0 and global 0; local 1 is dropped.
        let (program, again, _) = round_trip(&font, &["b"]);
        let private = again.private().unwrap();
        assert_eq!(private.subrs.len(), 1);
        assert_eq!(private.subrs[0], program.private().unwrap().subrs[0]);
        assert_eq!(again.global_subrs(), program.global_subrs());
        let reached = again.reached_subrs(again.gid(b"b").unwrap()).unwrap();
        assert_eq!(reached.local, set(&[0]));
        assert_eq!(reached.global, set(&[0]));
        // `f` carries stems, a mask, and a flex.
        let (_, again, _) = round_trip(&font, &["f", "c"]);
        assert!(again.private().unwrap().subrs.is_empty());
        assert_eq!(again.global_subrs().len(), 1);
        assert_eq!(again.charset_names(), vec![&b".notdef"[..], b"c", b"f"]);
    }

    #[test]
    fn renumbering_crosses_the_bias_boundary() {
        // 1300 local and 1240 global subroutines, so both biases are
        // 1131; `a` reaches local 1299 (which reaches local 5) and
        // global 1239. Pruned, two locals and one global remain, so the
        // biases fall to 107 and every operand changes.
        let mut font = CffFont::new("Big").widths(0, 500);
        for k in 0..1300 {
            let code = match k {
                5 => Type2Builder::new().rlineto(0, 10).r#return().bytes(),
                1299 => Type2Builder::new()
                    .rlineto(10, 0)
                    .callsubr(5 - 1131)
                    .r#return()
                    .bytes(),
                _ => Type2Builder::new().rlineto(k % 50, 1).r#return().bytes(),
            };
            font = font.subr(code);
        }
        for k in 0..1240 {
            let code = if k == 1239 {
                Type2Builder::new().rlineto(7, 7).r#return().bytes()
            } else {
                Type2Builder::new().rlineto(1, k % 30).r#return().bytes()
            };
            font = font.gsubr(code);
        }
        font = font.charstring(
            "a",
            Type2Builder::new()
                .num(100)
                .rmoveto(0, 0)
                .callsubr(1299 - 1131)
                .callgsubr(1239 - 1131)
                .endchar()
                .bytes(),
        );
        let (program, again, bytes) = round_trip(&font, &["a"]);
        assert_eq!(bias(program.private().unwrap().subrs.len()), 1131);
        let private = again.private().unwrap();
        assert_eq!(private.subrs.len(), 2);
        assert_eq!(again.global_subrs().len(), 1);
        assert_eq!(bias(private.subrs.len()), 107);
        assert_eq!(private.subrs[0], program.private().unwrap().subrs[5]);
        assert_eq!(
            private.subrs[1],
            Type2Builder::new()
                .rlineto(10, 0)
                .callsubr(-107)
                .r#return()
                .bytes()
        );
        assert_eq!(
            again.charstring(1).unwrap(),
            Type2Builder::new()
                .num(100)
                .rmoveto(0, 0)
                .callsubr(-106)
                .callgsubr(-107)
                .endchar()
                .bytes()
        );
        let reached = again.reached_subrs(1).unwrap();
        assert_eq!(reached.local, set(&[0, 1]));
        assert_eq!(reached.global, set(&[0]));
        assert!(bytes.len() < font.build().len() / 10, "{}", bytes.len());
    }

    /// Eight stems declared in the glyph, then a subroutine whose
    /// `hintmask` byte is the `callsubr` operator's value and whose call
    /// follows it, then a mask in the glyph whose byte is `callgsubr`'s
    /// value: the mask lengths come from the trace, not from the stems
    /// the subroutine itself declares.
    fn mask_font() -> CffFont {
        let hint = Type2Builder::new()
            .hintmask(&[10])
            .rmoveto(0, 0)
            .callsubr(-105)
            .r#return()
            .bytes();
        let inner = Type2Builder::new().rlineto(10, 10).r#return().bytes();
        let unused = Type2Builder::new().rlineto(1, 1).r#return().bytes();
        let mut h = Type2Builder::new().num(100);
        for k in 0..4 {
            h = h.num(k * 20).num(10);
        }
        h = h.op(18);
        for k in 0..4 {
            h = h.num(k * 20).num(10);
        }
        let h = h
            .op(23)
            .callsubr(-106)
            .hintmask(&[29])
            .rlineto(5, 5)
            .endchar()
            .bytes();
        CffFont::new("Mask")
            .widths(0, 500)
            .subr(unused)
            .subr(hint)
            .subr(inner)
            .charstring("h", h)
    }

    #[test]
    fn masks_inside_subroutines_are_stepped_over_by_the_trace() {
        let font = mask_font();
        let program = CffProgram::parse(&font.build()).unwrap();
        let reached = program.reached_subrs(1).unwrap();
        assert_eq!(reached.local, set(&[1, 2]));
        let hint_mask_at = 0;
        let glyph_mask_at = program.charstring(1).unwrap().len() - 6;
        assert_eq!(
            reached.masks,
            [
                (Site::Local(1), hint_mask_at, 1),
                (Site::Glyph(1), glyph_mask_at, 1),
            ]
            .into_iter()
            .collect()
        );
        // Read on its own, the subroutine's mask byte looks like a call.
        let naive = tokens(&program.private().unwrap().subrs[1]).unwrap();
        assert_eq!(naive[1], Token::Mask(&[]));
        assert_eq!(naive[2], Token::Op(10));

        let (_, again, _) = round_trip(&font, &["h"]);
        let private = again.private().unwrap();
        assert_eq!(private.subrs.len(), 2, "renumbered, not stubbed");
        assert_eq!(
            private.subrs[0],
            Type2Builder::new()
                .hintmask(&[10])
                .rmoveto(0, 0)
                .callsubr(-106)
                .r#return()
                .bytes()
        );
        assert_eq!(again.reached_subrs(1).unwrap().local, set(&[0, 1]));
    }

    #[test]
    fn calls_without_a_literal_operand_fall_back_to_stubs_and_faults_keep_all() {
        // `-214 2 div callsubr` reaches 0 through arithmetic.
        let computed = Type2Builder::new()
            .num(100)
            .rmoveto(0, 0)
            .num(-214)
            .num(2)
            .esc(12)
            .op(10)
            .endchar()
            .bytes();
        let font = corpus_cff().charstring("k", computed);
        let (program, again, _) = round_trip(&font, &["k"]);
        let private = again.private().unwrap();
        assert_eq!(private.subrs.len(), 2);
        assert_eq!(private.subrs[0], program.private().unwrap().subrs[0]);
        assert_eq!(private.subrs[1], RETURN.to_vec());
        assert_eq!(again.global_subrs(), &[RETURN.to_vec()]);
        assert_eq!(again.charstring(1).unwrap(), program.charstring(5).unwrap());

        // A kept charstring that cannot be interpreted keeps everything.
        let font = corpus_cff().charstring("m", Type2Builder::new().num(1).bytes());
        let program = CffProgram::parse(&font.build()).unwrap();
        let keep = subset_names(&program, [&b"m"[..], b"a"]);
        let again = CffProgram::parse(&subset(&program, b"Sub", &keep).unwrap()).unwrap();
        assert_eq!(
            again.private().unwrap().subrs,
            program.private().unwrap().subrs
        );
        assert_eq!(again.global_subrs(), program.global_subrs());
        assert_eq!(again.charstring(2).unwrap(), program.charstring(5).unwrap());
        assert_eq!(glyph(&again, "a"), glyph(&program, "a"));

        // An empty `.notdef` reaches nothing and does not stop pruning.
        let mut font = corpus_cff();
        font.glyphs[0].1.clear();
        let program = CffProgram::parse(&font.build()).unwrap();
        let keep = subset_names(&program, [&b"a"[..]]);
        let again = CffProgram::parse(&subset(&program, b"Sub", &keep).unwrap()).unwrap();
        assert!(again.private().unwrap().subrs.is_empty());
        assert_eq!(again.charstring(0).unwrap(), &[]);
        assert_eq!(glyph(&again, "a"), glyph(&program, "a"));
    }

    /// The CID subset of `font` keeping `cids`, parsed back; every kept
    /// CID measures and outlines as the original.
    fn cid_round_trip(font: &CffFont, cids: &[u16]) -> (CffProgram, CffProgram, Vec<u8>) {
        let program = CffProgram::parse(&font.build()).unwrap();
        let keep: BTreeSet<u16> = cids.iter().copied().collect();
        let bytes = subset_cid(&program, b"ABCDEF+Sub", &keep).unwrap();
        let again = CffProgram::parse(&bytes).unwrap();
        assert_eq!(again.name(), b"ABCDEF+Sub");
        assert!(again.is_cid_keyed());
        assert_eq!(again.ros(), program.ros());
        assert_eq!(again.cid_count(), program.cid_count());
        assert_eq!(again.font_bbox(), program.font_bbox());
        for &cid in keep.iter().chain(std::iter::once(&0)) {
            let want = program.glyph_by_cid(cid).unwrap();
            let got = again.glyph_by_cid(cid).unwrap();
            assert_eq!(got.is_some(), want.is_some(), "CID {cid}");
            if let (Some(got), Some(want)) = (got, want) {
                assert_eq!(*got, *want, "CID {cid}");
            }
        }
        (program, again, bytes)
    }

    #[test]
    fn cid_subsets_keep_only_the_dictionaries_the_glyphs_run_under() {
        use crate::testing::corpus_cid_cff;
        let font = corpus_cid_cff();
        // CIDs 1 and 2 run under dictionaries 0 and 1: both survive.
        let (program, again, bytes) = cid_round_trip(&font, &[1, 2]);
        assert_eq!(again.charset(), &[0, 1, 2]);
        assert_eq!(again.fd_index(0), Some(0));
        assert_eq!(again.fd_index(1), Some(0));
        assert_eq!(again.fd_index(2), Some(1));
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts.len(), 2);
        assert_eq!(dicts[0].default_width_x, 500.0);
        assert_eq!(dicts[1].default_width_x, 700.0);
        assert!(bytes.len() < font.build().len());
        assert_eq!(program.glyph_count(), 6);
        // CID 2 alone: only dictionary 1 survives and becomes dictionary
        // 0 of the subset; CID 0 comes along and keeps its own.
        let (_, again, _) = cid_round_trip(&font, &[2]);
        assert_eq!(again.charset(), &[0, 2]);
        let Privates::Cid { dicts, select } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts.len(), 2, "CID 0's dictionary and CID 2's");
        assert_eq!(select, &[0, 1]);
        assert_eq!(dicts[1].default_width_x, 700.0);
        // CIDs of one dictionary only, out of order, with an unknown one.
        let (_, again, _) = cid_round_trip(&font, &[200, 34, 99]);
        assert_eq!(again.charset(), &[0, 34, 200]);
        let Privates::Cid { dicts, select } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts.len(), 1);
        assert_eq!(select, &[0, 0, 0]);
        assert_eq!(again.glyph_by_cid(99).unwrap(), None);
        // Nothing but CID 0.
        let (_, again, _) = cid_round_trip(&font, &[]);
        assert_eq!(again.glyph_count(), 1);
        // A name-keyed program has no CID form.
        let named = CffProgram::parse(&corpus_cff().build()).unwrap();
        assert!(matches!(
            subset_cid(&named, b"Sub", &BTreeSet::new()),
            Err(FontError::Unsupported(_))
        ));
    }

    /// Two dictionaries with local subroutines of their own and two
    /// global ones: CID 1 (dictionary 0) reaches local 1 and global 0,
    /// CID 2 (dictionary 1) reaches local 0 and global 1, CID 3
    /// (dictionary 1) reaches nothing; local 0 of dictionary 0 and
    /// local 1 of dictionary 1 are reached by no glyph.
    fn subr_font() -> CffFont {
        let bar = Type2Builder::new().rlineto(100, 0).r#return().bytes();
        let up = Type2Builder::new().rlineto(0, 100).r#return().bytes();
        let diag = Type2Builder::new().rlineto(50, 50).r#return().bytes();
        let back = Type2Builder::new().rlineto(-50, 50).r#return().bytes();
        let unused = Type2Builder::new().rlineto(1, 1).r#return().bytes();
        let one = Type2Builder::new()
            .rmoveto(0, 0)
            .callsubr(1 - 107)
            .callgsubr(0 - 107)
            .endchar()
            .bytes();
        let two = Type2Builder::new()
            .rmoveto(10, 10)
            .callsubr(0 - 107)
            .callgsubr(1 - 107)
            .endchar()
            .bytes();
        let three = Type2Builder::new()
            .rmoveto(0, 0)
            .rlineto(20, 20)
            .endchar()
            .bytes();
        CffFont::cid_keyed("Subrs", "Adobe", "Identity", 0)
            .fd(CffFd {
                subrs: vec![unused.clone(), bar],
                default_width: 500,
                nominal_width: 0,
            })
            .fd(CffFd {
                subrs: vec![up, unused],
                default_width: 600,
                nominal_width: 0,
            })
            .gsubr(diag)
            .gsubr(back)
            .cid_glyph(1, 0, one)
            .cid_glyph(2, 1, two)
            .cid_glyph(3, 1, three)
    }

    #[test]
    fn cid_subsets_prune_each_dictionarys_subroutines_and_the_global_ones() {
        let font = subr_font();
        let (program, again, _) = cid_round_trip(&font, &[1, 2, 3]);
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        let Privates::Cid {
            dicts: original, ..
        } = program.privates()
        else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts[0].subrs, vec![original[0].subrs[1].clone()]);
        assert_eq!(dicts[1].subrs, vec![original[1].subrs[0].clone()]);
        assert_eq!(again.global_subrs(), program.global_subrs());
        assert_eq!(
            again.charstring(1).unwrap(),
            Type2Builder::new()
                .rmoveto(0, 0)
                .callsubr(-107)
                .callgsubr(-107)
                .endchar()
                .bytes()
        );
        // CID 2 alone: one dictionary, one local, one global, renumbered.
        let (_, again, _) = cid_round_trip(&font, &[2]);
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts.len(), 2);
        assert!(dicts[0].subrs.is_empty(), "CID 0 reaches nothing");
        assert_eq!(dicts[1].subrs.len(), 1);
        assert_eq!(again.global_subrs().len(), 1);
        assert_eq!(
            again.charstring(1).unwrap(),
            Type2Builder::new()
                .rmoveto(10, 10)
                .callsubr(-107)
                .callgsubr(-107)
                .endchar()
                .bytes()
        );
        // CID 3 alone: no subroutine of any kind survives.
        let (_, again, _) = cid_round_trip(&font, &[3]);
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        assert!(dicts.iter().all(|d| d.subrs.is_empty()));
        assert!(again.global_subrs().is_empty());
    }

    #[test]
    fn a_global_subroutine_calling_a_local_one_is_renumbered_only_under_one_dictionary() {
        // Global 0 calls local 0 of whichever dictionary the glyph runs
        // under; both dictionaries have one.
        let bar = Type2Builder::new().rlineto(100, 0).r#return().bytes();
        let up = Type2Builder::new().rlineto(0, 100).r#return().bytes();
        let unused = Type2Builder::new().rlineto(1, 1).r#return().bytes();
        let glyph = |dx: i32| {
            Type2Builder::new()
                .rmoveto(dx, 0)
                .callgsubr(1 - 107)
                .endchar()
                .bytes()
        };
        let font = CffFont::cid_keyed("Cross", "Adobe", "Identity", 0)
            .fd(CffFd {
                subrs: vec![unused.clone(), bar],
                default_width: 500,
                nominal_width: 0,
            })
            .fd(CffFd {
                subrs: vec![unused.clone(), up],
                default_width: 500,
                nominal_width: 0,
            })
            .gsubr(unused)
            .gsubr(Type2Builder::new().callsubr(1 - 107).r#return().bytes())
            .cid_glyph(1, 0, glyph(0))
            .cid_glyph(2, 1, glyph(10));
        // Both dictionaries kept: the global call cannot be renumbered
        // for two numberings at once, so the stub tier keeps counts.
        let (program, again, _) = cid_round_trip(&font, &[1, 2]);
        assert_eq!(again.global_subrs().len(), 2);
        assert_eq!(again.global_subrs()[0], RETURN.to_vec());
        assert_eq!(again.global_subrs()[1], program.global_subrs()[1]);
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts[0].subrs[0], RETURN.to_vec());
        assert_eq!(dicts[1].subrs[1], up_of(&program, 1));
        // One dictionary kept (CID 0 shares dictionary 0 with CID 1):
        // everything renumbers densely, the global call included.
        let (_, again, _) = cid_round_trip(&font, &[1]);
        let Privates::Cid { dicts, .. } = again.privates() else {
            panic!("CID-keyed");
        };
        assert_eq!(dicts.len(), 1);
        assert_eq!(dicts[0].subrs, vec![bar_of(&program)]);
        assert_eq!(again.global_subrs().len(), 1);
        assert_eq!(
            again.global_subrs()[0],
            Type2Builder::new().callsubr(-107).r#return().bytes()
        );
    }

    fn bar_of(program: &CffProgram) -> Vec<u8> {
        let Privates::Cid { dicts, .. } = program.privates() else {
            panic!("CID-keyed");
        };
        dicts[0].subrs[1].clone()
    }

    fn up_of(program: &CffProgram, fd: usize) -> Vec<u8> {
        let Privates::Cid { dicts, .. } = program.privates() else {
            panic!("CID-keyed");
        };
        dicts[fd].subrs[1].clone()
    }

    #[test]
    fn strings_matrices_and_descriptor_entries_carry_over() {
        let font = CffFont::new("Enc")
            .widths(250, 500)
            .std_vw(90)
            .font_matrix([0.002, 0.0, 0.0, 0.002, 0.0, 0.0])
            .bbox([-10, -20, 700, 800])
            .notice("a notice")
            .glyph("square", 600, &rectangle(0.0, 0.0, 500.0, 500.0))
            .glyph("A", 700, &rectangle(0.0, 0.0, 600.0, 600.0))
            .glyph("odd.alt", 300, &rectangle(0.0, 0.0, 100.0, 100.0))
            .encode(65, "square")
            .encode(66, "A");
        let (_, again, _) = round_trip(&font, &["A", "odd.alt"]);
        assert_eq!(again.top_string(op::NOTICE), Some(&b"a notice"[..]));
        assert_eq!(
            again.strings(),
            &[b"a notice".to_vec(), b"odd.alt".to_vec()]
        );
        assert!(again.has_font_matrix());
        assert_eq!(again.font_matrix(), [0.002, 0.0, 0.0, 0.002, 0.0, 0.0]);
        assert_eq!(again.font_bbox(), [-10.0, -20.0, 700.0, 800.0]);
        assert_eq!(again.charset(), &[0, 34, 392]);
        assert_eq!(again.private().unwrap().number(op::STD_VW), Some(90.0));
        assert_eq!(again.private().unwrap().default_width_x, 250.0);
        assert_eq!(glyph(&again, ".notdef").advance.0, 250.0);
        // The custom encoding (square at 65, A at 66) is dropped; the
        // standard one puts A at 65 and nothing at 66.
        assert_eq!(again.encoding()[65], again.gid(b"A"));
        assert!(again.encoding()[66].is_none());

        let cid = CffFont::cid_keyed("SynCID", "Adobe", "Identity", 0)
            .fd(CffFd::default())
            .cid_glyph(1, 0, Type2Builder::new().endchar().bytes());
        let program = CffProgram::parse(&cid.build()).unwrap();
        assert_eq!(
            subset(&program, b"Sub", &names(&[".notdef"])),
            Err(FontError::Unsupported("CID-keyed subset"))
        );
    }
}
