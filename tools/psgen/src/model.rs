// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The generator's static picture of the program it is writing: the
//! operand stack as abstract types, the composite objects those types
//! refer to (so an array's element types and a string's length are
//! known), the dictionary stack with its definitions, and the few bits
//! of graphics state the graphics productions depend on.
//!
//! The model is exact for the programs the grammar emits: every
//! production consumes and produces model items, and the productions that
//! could fail at run time (division by zero, an index out of range, an
//! integer overflowing into a real) are only offered when the model shows
//! they cannot.

/// An abstract type; `Any` is what nothing in the grammar produces but
/// what the ill-typed replacement may pretend to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Real,
    Bool,
    String,
    Name,
    /// An executable name, as `type` returns: it runs when a variable
    /// holding it is referenced, so the grammar stores it but never
    /// reads it back by name.
    ExecName,
    Array,
    Proc,
    Dict,
    Mark,
    Null,
    Any,
}

impl Ty {
    pub fn is_num(self) -> bool {
        matches!(self, Ty::Int | Ty::Real)
    }

    /// Whether values of the type live in VM and are subject to
    /// `restore`'s referential check.
    pub fn is_composite(self) -> bool {
        matches!(self, Ty::String | Ty::Array | Ty::Proc | Ty::Dict)
    }
}

/// One operand-stack entry. `mag` bounds a number's magnitude: the value
/// is below `10^mag` in absolute terms. `id` names the composite the item
/// refers to, `None` for a composite the model knows nothing about (a
/// procedure's output, a `cvs` result). `epoch` is the save level the
/// composite was created in. `inexact` marks a number read back from the
/// graphics state through the CTM (`currentpoint`, `pathbbox`, `arcto`),
/// whose last digit a translation of the program may move: the grammar
/// keeps such numbers, and anything computed from them, away from
/// operators whose result would jump at a boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub ty: Ty,
    pub mag: u8,
    pub id: Option<usize>,
    pub epoch: u32,
    pub inexact: bool,
}

impl Item {
    pub fn scalar(ty: Ty) -> Item {
        Item {
            ty,
            mag: 0,
            id: None,
            epoch: 0,
            inexact: false,
        }
    }

    pub fn num(ty: Ty, mag: u8) -> Item {
        Item {
            ty,
            mag,
            id: None,
            epoch: 0,
            inexact: false,
        }
    }

    /// A real read back through the CTM.
    pub fn inexact_real(mag: u8) -> Item {
        Item {
            inexact: true,
            ..Item::num(Ty::Real, mag)
        }
    }

    /// A composite the model does not track, created now.
    pub fn opaque(ty: Ty, epoch: u32) -> Item {
        Item {
            ty,
            mag: 0,
            id: None,
            epoch,
            inexact: false,
        }
    }

    /// Whether the item matches another in everything a loop body must
    /// preserve: type, identity, magnitude, save level, and exactness.
    pub fn same_as(&self, other: &Item) -> bool {
        self.ty == other.ty
            && self.id == other.id
            && self.mag == other.mag
            && self.epoch == other.epoch
            && self.inexact == other.inexact
    }
}

/// A composite the model tracks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comp {
    /// `aliased` marks storage shared with another object (through
    /// `getinterval`), which the grammar then never writes to.
    Array {
        elems: Vec<Item>,
        aliased: bool,
    },
    Str {
        len: usize,
        aliased: bool,
    },
    Dict {
        entries: Vec<(String, Item)>,
    },
    /// A procedure with a declared signature: the inputs it consumes
    /// (with the magnitudes it was generated for) and what it leaves.
    Proc {
        inputs: Vec<Item>,
        outputs: Vec<Item>,
    },
}

/// The graphics state bits the productions depend on. `lo` and `hi`
/// bound the CTM's scale in thousandths (the smallest and largest factor
/// by which it may stretch a length): readings back through the CTM
/// carry a rounding error near `10⁻⁷·|translation| / scale`, so the
/// grammar keeps the band within [1/8, 8] and the relations' tolerances
/// cover what is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gfx {
    pub current_point: bool,
    pub font: bool,
    pub lo: i64,
    pub hi: i64,
}

impl Default for Gfx {
    fn default() -> Self {
        Gfx {
            current_point: false,
            font: false,
            lo: 1000,
            hi: 1000,
        }
    }
}

impl Gfx {
    pub const BAND: (i64, i64) = (125, 8000);

    /// The band after a transform stretching lengths by factors between
    /// `lo` and `hi` hundredths; `None` when it would leave [1/8, 8].
    pub fn stretched(self, lo: i64, hi: i64) -> Option<Gfx> {
        let lo = self.lo * lo / 100;
        let hi = (self.hi * hi + 99) / 100;
        (lo >= Gfx::BAND.0 && hi <= Gfx::BAND.1).then_some(Gfx { lo, hi, ..self })
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub stack: Vec<Item>,
    pub comps: Vec<Comp>,
    /// Dictionary ids, bottom first; the first is `userdict`.
    pub dstack: Vec<usize>,
    pub epoch: u32,
    pub gfx: Gfx,
    pub gsaves: Vec<Gfx>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Model {
        Model {
            stack: Vec::new(),
            comps: vec![Comp::Dict {
                entries: Vec::new(),
            }],
            dstack: vec![0],
            epoch: 0,
            gfx: Gfx::default(),
            gsaves: Vec::new(),
        }
    }

    // --- the operand stack -----------------------------------------------

    pub fn push(&mut self, item: Item) {
        self.stack.push(item);
    }

    pub fn pop(&mut self) -> Option<Item> {
        self.stack.pop()
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The item `n` below the top.
    pub fn peek(&self, n: usize) -> Option<&Item> {
        let len = self.stack.len();
        if n < len {
            self.stack.get(len - 1 - n)
        } else {
            None
        }
    }

    // --- composites -------------------------------------------------------

    pub fn alloc(&mut self, comp: Comp) -> Item {
        let ty = match &comp {
            Comp::Array { .. } => Ty::Array,
            Comp::Str { .. } => Ty::String,
            Comp::Dict { .. } => Ty::Dict,
            Comp::Proc { .. } => Ty::Proc,
        };
        self.comps.push(comp);
        Item {
            ty,
            mag: 0,
            id: Some(self.comps.len() - 1),
            epoch: self.epoch,
            inexact: false,
        }
    }

    pub fn comp(&self, id: usize) -> &Comp {
        &self.comps[id]
    }

    pub fn comp_mut(&mut self, id: usize) -> &mut Comp {
        &mut self.comps[id]
    }

    /// The element types of a tracked array.
    pub fn array_elems(&self, item: &Item) -> Option<&[Item]> {
        match item.id.map(|id| self.comp(id)) {
            Some(Comp::Array { elems, .. }) if item.ty == Ty::Array => Some(elems),
            _ => None,
        }
    }

    /// Whether a tracked array may be written to.
    pub fn array_writable(&self, item: &Item) -> bool {
        matches!(
            item.id.map(|id| self.comp(id)),
            Some(Comp::Array { aliased: false, .. })
        ) && item.ty == Ty::Array
    }

    /// The length of a tracked string.
    pub fn string_len(&self, item: &Item) -> Option<usize> {
        match item.id.map(|id| self.comp(id)) {
            Some(Comp::Str { len, .. }) if item.ty == Ty::String => Some(*len),
            _ => None,
        }
    }

    pub fn string_writable(&self, item: &Item) -> bool {
        matches!(
            item.id.map(|id| self.comp(id)),
            Some(Comp::Str { aliased: false, .. })
        ) && item.ty == Ty::String
    }

    pub fn mark_aliased(&mut self, id: usize) {
        match self.comp_mut(id) {
            Comp::Array { aliased, .. } | Comp::Str { aliased, .. } => *aliased = true,
            _ => {}
        }
    }

    pub fn dict_entries(&self, item: &Item) -> Option<&[(String, Item)]> {
        match item.id.map(|id| self.comp(id)) {
            Some(Comp::Dict { entries }) if item.ty == Ty::Dict => Some(entries),
            _ => None,
        }
    }

    pub fn proc_sig(&self, item: &Item) -> Option<(&[Item], &[Item])> {
        match item.id.map(|id| self.comp(id)) {
            Some(Comp::Proc { inputs, outputs }) if item.ty == Ty::Proc => Some((inputs, outputs)),
            _ => None,
        }
    }

    /// The tracked length of a composite, for `length`.
    pub fn length_of(&self, item: &Item) -> Option<usize> {
        match item.id.map(|id| self.comp(id))? {
            Comp::Array { elems, .. } => Some(elems.len()),
            Comp::Str { len, .. } => Some(*len),
            Comp::Dict { entries } => Some(entries.len()),
            Comp::Proc { .. } => None,
        }
    }

    // --- dictionaries -----------------------------------------------------

    pub fn dict_put(&mut self, dict: usize, name: &str, item: Item) {
        if let Comp::Dict { entries } = self.comp_mut(dict) {
            match entries.iter_mut().find(|(k, _)| k == name) {
                Some(slot) => slot.1 = item,
                None => entries.push((name.to_string(), item)),
            }
        }
    }

    pub fn dict_remove(&mut self, dict: usize, name: &str) {
        if let Comp::Dict { entries } = self.comp_mut(dict) {
            entries.retain(|(k, _)| k != name);
        }
    }

    pub fn dict_get(&self, dict: usize, name: &str) -> Option<&Item> {
        match self.comp(dict) {
            Comp::Dict { entries } => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn current_dict(&self) -> usize {
        *self.dstack.last().expect("userdict is never popped")
    }

    /// `def` into the current dictionary.
    pub fn define(&mut self, name: &str, item: Item) {
        let dict = self.current_dict();
        self.dict_put(dict, name, item);
    }

    /// The value a name resolves to through the dictionary stack.
    pub fn lookup(&self, name: &str) -> Option<&Item> {
        self.dstack
            .iter()
            .rev()
            .find_map(|&dict| self.dict_get(dict, name))
    }

    /// Every name that resolves, innermost definitions first, without
    /// duplicates.
    pub fn visible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for &dict in self.dstack.iter().rev() {
            if let Comp::Dict { entries } = self.comp(dict) {
                for (name, _) in entries {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
            }
        }
        names
    }

    pub fn begin(&mut self, dict: usize) {
        self.dstack.push(dict);
    }

    pub fn end(&mut self) {
        if self.dstack.len() > 1 {
            self.dstack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionaries_scope_top_down() {
        let mut model = Model::new();
        model.define("x", Item::num(Ty::Int, 4));
        let inner = model.alloc(Comp::Dict {
            entries: Vec::new(),
        });
        model.begin(inner.id.unwrap());
        model.define("x", Item::scalar(Ty::Bool));
        model.define("y", Item::scalar(Ty::Name));
        assert_eq!(model.lookup("x").unwrap().ty, Ty::Bool);
        assert_eq!(model.visible_names(), ["x", "y"]);
        model.end();
        assert_eq!(model.lookup("x").unwrap().ty, Ty::Int);
        assert_eq!(model.lookup("y"), None);
        model.end();
        assert_eq!(model.dstack, [0]);
        model.dict_remove(0, "x");
        assert_eq!(model.lookup("x"), None);
    }

    #[test]
    fn composites_report_their_shape() {
        let mut model = Model::new();
        model.epoch = 2;
        let array = model.alloc(Comp::Array {
            elems: vec![Item::num(Ty::Int, 4); 3],
            aliased: false,
        });
        assert_eq!(array.epoch, 2);
        assert_eq!(model.length_of(&array), Some(3));
        assert!(model.array_writable(&array));
        model.mark_aliased(array.id.unwrap());
        assert!(!model.array_writable(&array));
        let string = model.alloc(Comp::Str {
            len: 5,
            aliased: false,
        });
        assert_eq!(model.string_len(&string), Some(5));
        assert_eq!(model.string_len(&array), None);
        model.push(string.clone());
        model.push(array.clone());
        assert_eq!(model.peek(1), Some(&string));
        assert_eq!(model.peek(2), None);
        assert!(Item::opaque(Ty::String, 1).same_as(&Item::opaque(Ty::String, 1)));
        assert!(!Item::opaque(Ty::String, 1).same_as(&Item::opaque(Ty::String, 3)));
        assert!(!Item::num(Ty::Int, 3).same_as(&Item::num(Ty::Int, 4)));
        assert!(!Item::inexact_real(4).same_as(&Item::num(Ty::Real, 4)));
        assert!(Item::inexact_real(4).inexact);
    }

    #[test]
    fn the_scale_band_is_bounded() {
        let g = Gfx::default();
        assert_eq!((g.lo, g.hi), (1000, 1000));
        let halved = g.stretched(50, 50).unwrap();
        assert_eq!((halved.lo, halved.hi), (500, 500));
        let mut band = g;
        let mut halvings = 0;
        while let Some(next) = band.stretched(50, 50) {
            band = next;
            halvings += 1;
        }
        assert_eq!(halvings, 3);
        let mut band = g;
        let mut doublings = 0;
        while let Some(next) = band.stretched(200, 200) {
            band = next;
            doublings += 1;
        }
        assert_eq!(doublings, 3);
        let mixed = g.stretched(50, 200).unwrap();
        assert_eq!((mixed.lo, mixed.hi), (500, 2000));
        assert_eq!(mixed.stretched(24, 100), None);
        assert!(mixed.stretched(25, 100).is_some());
    }
}
