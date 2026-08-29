// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The PostScript object: a 16-byte `Copy` value.
//!
//! Composite objects hold a handle into a VM arena plus an offset and length
//! into that slot's storage (see [`crate::memory`]); they never hold pointers.

use std::fmt;

use crate::names::Atom;

/// Index of a slot in an arena. Each arena has its own handle space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(pub u32);

/// Which VM a composite object's storage lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    Local = 0,
    Global = 1,
}

/// Access attribute. The discriminants are ordered from most to least
/// permissive so the values can be compared to answer "at least this
/// permissive".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Access {
    Unlimited = 0,
    ReadOnly = 1,
    ExecuteOnly = 2,
    None = 3,
}

/// PostScript object types, as reported by the `type` operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Type {
    Integer = 0,
    Real = 1,
    Boolean = 2,
    Null = 3,
    Mark = 4,
    Name = 5,
    Operator = 6,
    Save = 7,
    FontId = 8,
    Array = 9,
    PackedArray = 10,
    String = 11,
    Dict = 12,
    File = 13,
    GState = 14,
}

impl Type {
    // Indexed by the 6-bit header field; `PackedArray` is never stored there
    // (it is `Array` plus the packed bit), so it is absent from this table.
    const FROM_RAW: [Type; 14] = [
        Type::Integer,
        Type::Real,
        Type::Boolean,
        Type::Null,
        Type::Mark,
        Type::Name,
        Type::Operator,
        Type::Save,
        Type::FontId,
        Type::Array,
        Type::String,
        Type::Dict,
        Type::File,
        Type::GState,
    ];

    const fn raw(self) -> u32 {
        match self {
            Type::Integer => 0,
            Type::Real => 1,
            Type::Boolean => 2,
            Type::Null => 3,
            Type::Mark => 4,
            Type::Name => 5,
            Type::Operator => 6,
            Type::Save => 7,
            Type::FontId => 8,
            Type::Array | Type::PackedArray => 9,
            Type::String => 10,
            Type::Dict => 11,
            Type::File => 12,
            Type::GState => 13,
        }
    }

    pub const fn is_composite(self) -> bool {
        matches!(
            self,
            Type::Array | Type::PackedArray | Type::String | Type::Dict | Type::File | Type::GState
        )
    }

    pub const fn is_number(self) -> bool {
        matches!(self, Type::Integer | Type::Real)
    }

    /// Whether objects of this type carry their access attribute in the
    /// object header rather than in shared storage.
    pub const fn has_object_access(self) -> bool {
        matches!(
            self,
            Type::Array | Type::PackedArray | Type::String | Type::File
        )
    }

    /// Whether objects of this type address a sub-interval of their storage.
    pub const fn has_interval(self) -> bool {
        matches!(self, Type::Array | Type::PackedArray | Type::String)
    }

    /// The name returned by the `type` operator.
    pub const fn name(self) -> &'static str {
        match self {
            Type::Integer => "integertype",
            Type::Real => "realtype",
            Type::Boolean => "booleantype",
            Type::Null => "nulltype",
            Type::Mark => "marktype",
            Type::Name => "nametype",
            Type::Operator => "operatortype",
            Type::Save => "savetype",
            Type::FontId => "fonttype",
            Type::Array => "arraytype",
            Type::PackedArray => "packedarraytype",
            Type::String => "stringtype",
            Type::Dict => "dicttype",
            Type::File => "filetype",
            Type::GState => "gstatetype",
        }
    }
}

/// Space, handle, offset, and length of a composite object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompositeRef {
    pub space: Space,
    pub handle: Handle,
    pub offset: u32,
    pub length: u32,
}

// Header layout, low bits first: type:6 exec:1 access:2 space:1 packed:1.
const TYPE_MASK: u32 = 0x3F;
const EXEC_BIT: u32 = 1 << 6;
const ACCESS_SHIFT: u32 = 7;
const ACCESS_MASK: u32 = 0b11 << ACCESS_SHIFT;
const SPACE_BIT: u32 = 1 << 9;
const PACKED_BIT: u32 = 1 << 10;

/// A PostScript object.
///
/// Attributes (executable, access) are properties of the value, so changing
/// them yields a new `Object`; earlier copies are unaffected.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Object {
    header: u32,
    // Simple types use word 0 (and zero the rest, so bitwise comparison of
    // two objects built the same way is meaningful). Composites use
    // handle, offset, length.
    payload: [u32; 3],
}

impl Object {
    const fn simple(ty: Type, word: u32) -> Self {
        Object {
            header: ty.raw(),
            payload: [word, 0, 0],
        }
    }

    const fn composite(ty: Type, space: Space, handle: Handle, length: u32) -> Self {
        let space_bit = match space {
            Space::Local => 0,
            Space::Global => SPACE_BIT,
        };
        Object {
            header: ty.raw() | space_bit,
            payload: [handle.0, 0, length],
        }
    }

    pub const fn integer(value: i32) -> Self {
        Self::simple(Type::Integer, value as u32)
    }

    pub const fn real(value: f32) -> Self {
        Self::simple(Type::Real, value.to_bits())
    }

    pub const fn boolean(value: bool) -> Self {
        Self::simple(Type::Boolean, value as u32)
    }

    pub const fn null() -> Self {
        Self::simple(Type::Null, 0)
    }

    pub const fn mark() -> Self {
        Self::simple(Type::Mark, 0)
    }

    /// A literal name.
    pub const fn name(atom: Atom) -> Self {
        Self::simple(Type::Name, atom.0)
    }

    /// An operator, identified by its index into the operator table.
    /// Operators are executable by default.
    pub const fn operator(index: u32) -> Self {
        Self::simple(Type::Operator, index).with_exec(true)
    }

    /// A save object, identified by its index on the save stack.
    pub const fn save(index: u32) -> Self {
        Self::simple(Type::Save, index)
    }

    pub const fn font_id(id: u32) -> Self {
        Self::simple(Type::FontId, id)
    }

    pub const fn array(space: Space, handle: Handle, length: u32) -> Self {
        Self::composite(Type::Array, space, handle, length)
    }

    /// Packed arrays are arrays with the packed bit set and read-only access.
    pub const fn packed_array(space: Space, handle: Handle, length: u32) -> Self {
        let object = Self::composite(Type::Array, space, handle, length);
        Object {
            header: object.header | PACKED_BIT | (Access::ReadOnly as u32) << ACCESS_SHIFT,
            payload: object.payload,
        }
    }

    pub const fn string(space: Space, handle: Handle, length: u32) -> Self {
        Self::composite(Type::String, space, handle, length)
    }

    pub const fn dict(space: Space, handle: Handle) -> Self {
        Self::composite(Type::Dict, space, handle, 0)
    }

    pub const fn file(space: Space, handle: Handle) -> Self {
        Self::composite(Type::File, space, handle, 0)
    }

    pub const fn gstate(space: Space, handle: Handle) -> Self {
        Self::composite(Type::GState, space, handle, 0)
    }

    pub fn ty(self) -> Type {
        let ty = Type::FROM_RAW[(self.header & TYPE_MASK) as usize];
        if ty == Type::Array && self.header & PACKED_BIT != 0 {
            Type::PackedArray
        } else {
            ty
        }
    }

    pub fn is_composite(self) -> bool {
        self.ty().is_composite()
    }

    pub fn is_number(self) -> bool {
        self.ty().is_number()
    }

    pub const fn is_executable(self) -> bool {
        self.header & EXEC_BIT != 0
    }

    pub const fn is_literal(self) -> bool {
        !self.is_executable()
    }

    pub const fn with_exec(self, executable: bool) -> Self {
        let header = if executable {
            self.header | EXEC_BIT
        } else {
            self.header & !EXEC_BIT
        };
        Object { header, ..self }
    }

    /// `cvx`
    pub const fn as_executable(self) -> Self {
        self.with_exec(true)
    }

    /// `cvlit`
    pub const fn as_literal(self) -> Self {
        self.with_exec(false)
    }

    pub const fn is_packed(self) -> bool {
        self.header & PACKED_BIT != 0
    }

    /// The access attribute for types that carry it in the object. `None`
    /// for dictionaries (whose access lives in storage) and simple objects.
    pub fn access(self) -> Option<Access> {
        if !self.ty().has_object_access() {
            return None;
        }
        Some(match (self.header & ACCESS_MASK) >> ACCESS_SHIFT {
            0 => Access::Unlimited,
            1 => Access::ReadOnly,
            2 => Access::ExecuteOnly,
            _ => Access::None,
        })
    }

    /// A copy with the given access, for types that carry access in the
    /// object; `None` otherwise.
    pub fn with_access(self, access: Access) -> Option<Self> {
        if !self.ty().has_object_access() {
            return None;
        }
        let header = (self.header & !ACCESS_MASK) | (access as u32) << ACCESS_SHIFT;
        Some(Object { header, ..self })
    }

    pub fn as_i32(self) -> Option<i32> {
        (self.ty() == Type::Integer).then_some(self.payload[0] as i32)
    }

    pub fn as_f32(self) -> Option<f32> {
        (self.ty() == Type::Real).then_some(f32::from_bits(self.payload[0]))
    }

    /// Either numeric type, widened to a real.
    pub fn as_number(self) -> Option<f32> {
        match self.ty() {
            Type::Integer => Some(self.payload[0] as i32 as f32),
            Type::Real => Some(f32::from_bits(self.payload[0])),
            _ => None,
        }
    }

    pub fn as_bool(self) -> Option<bool> {
        (self.ty() == Type::Boolean).then_some(self.payload[0] != 0)
    }

    pub fn as_name(self) -> Option<Atom> {
        (self.ty() == Type::Name).then_some(Atom(self.payload[0]))
    }

    pub fn as_operator(self) -> Option<u32> {
        (self.ty() == Type::Operator).then_some(self.payload[0])
    }

    pub fn as_save(self) -> Option<u32> {
        (self.ty() == Type::Save).then_some(self.payload[0])
    }

    pub fn as_font_id(self) -> Option<u32> {
        (self.ty() == Type::FontId).then_some(self.payload[0])
    }

    pub fn space(self) -> Option<Space> {
        let space = if self.header & SPACE_BIT != 0 {
            Space::Global
        } else {
            Space::Local
        };
        self.is_composite().then_some(space)
    }

    pub fn handle(self) -> Option<Handle> {
        self.is_composite().then_some(Handle(self.payload[0]))
    }

    pub fn offset(self) -> Option<u32> {
        self.is_composite().then_some(self.payload[1])
    }

    /// Element count for arrays and strings; zero for other composites.
    pub fn length(self) -> Option<u32> {
        self.is_composite().then_some(self.payload[2])
    }

    pub fn composite_ref(self) -> Option<CompositeRef> {
        Some(CompositeRef {
            space: self.space()?,
            handle: Handle(self.payload[0]),
            offset: self.payload[1],
            length: self.payload[2],
        })
    }

    /// `getinterval`: a copy narrowed to `length` elements starting at
    /// `offset` within this object's own interval. `None` if the type has no
    /// interval or the range does not fit.
    pub fn with_interval(self, offset: u32, length: u32) -> Option<Self> {
        if !self.ty().has_interval() {
            return None;
        }
        let end = offset.checked_add(length)?;
        if end > self.payload[2] {
            return None;
        }
        Some(Object {
            header: self.header,
            payload: [
                self.payload[0],
                self.payload[1].checked_add(offset)?,
                length,
            ],
        })
    }

    /// The `eq` relation as far as it can be decided from the values alone:
    /// numbers compare across integer/real, names by atom, and composites by
    /// identity (space, handle, offset, length) ignoring attributes. The
    /// `eq` operator additionally compares string contents, which needs
    /// storage access and lives with the operators.
    // Deliberately not `PartialEq`: the relation is not reflexive (NaN) and
    // ignores attributes, so `==` would mislead.
    #[allow(clippy::should_implement_trait)]
    pub fn eq(self, other: Object) -> bool {
        use Type::*;
        match (self.ty(), other.ty()) {
            (Integer, Integer) => self.payload[0] == other.payload[0],
            (Integer | Real, Integer | Real) => {
                self.as_number().unwrap() == other.as_number().unwrap()
            }
            (Null, Null) | (Mark, Mark) => true,
            (Boolean, Boolean)
            | (Name, Name)
            | (Operator, Operator)
            | (Save, Save)
            | (FontId, FontId) => self.payload[0] == other.payload[0],
            (a, b) if a.is_composite() && a.raw() == b.raw() => {
                self.header & SPACE_BIT == other.header & SPACE_BIT && self.payload == other.payload
            }
            _ => false,
        }
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ty = self.ty();
        let mut d = f.debug_struct(ty.name());
        match ty {
            Type::Integer => d.field("value", &self.as_i32().unwrap()),
            Type::Real => d.field("value", &self.as_f32().unwrap()),
            Type::Boolean => d.field("value", &self.as_bool().unwrap()),
            Type::Null | Type::Mark => &mut d,
            Type::Name => d.field("atom", &self.payload[0]),
            Type::Operator | Type::Save | Type::FontId => d.field("index", &self.payload[0]),
            _ => {
                let r = self.composite_ref().unwrap();
                d.field("space", &r.space)
                    .field("handle", &r.handle.0)
                    .field("offset", &r.offset)
                    .field("length", &r.length);
                if let Some(access) = self.access() {
                    d.field("access", &access);
                }
                &mut d
            }
        };
        d.field("exec", &self.is_executable()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: Handle = Handle(7);

    #[test]
    fn object_is_sixteen_bytes() {
        assert_eq!(std::mem::size_of::<Object>(), 16);
        assert_eq!(std::mem::align_of::<Object>(), 4);
    }

    #[test]
    fn simple_constructors_round_trip() {
        assert_eq!(Object::integer(-5).as_i32(), Some(-5));
        assert_eq!(Object::integer(i32::MIN).as_i32(), Some(i32::MIN));
        assert_eq!(Object::real(1.5).as_f32(), Some(1.5));
        assert_eq!(Object::boolean(true).as_bool(), Some(true));
        assert_eq!(Object::boolean(false).as_bool(), Some(false));
        assert_eq!(Object::name(Atom(3)).as_name(), Some(Atom(3)));
        assert_eq!(Object::operator(9).as_operator(), Some(9));
        assert_eq!(Object::save(2).as_save(), Some(2));
        assert_eq!(Object::font_id(4).as_font_id(), Some(4));
        assert_eq!(Object::null().ty(), Type::Null);
        assert_eq!(Object::mark().ty(), Type::Mark);
    }

    #[test]
    fn accessors_reject_other_types() {
        let i = Object::integer(1);
        assert_eq!(i.as_f32(), None);
        assert_eq!(i.as_bool(), None);
        assert_eq!(i.as_name(), None);
        assert_eq!(i.as_operator(), None);
        assert_eq!(i.handle(), None);
        assert_eq!(i.space(), None);
        assert_eq!(i.offset(), None);
        assert_eq!(i.length(), None);
        assert_eq!(i.composite_ref(), None);
        assert_eq!(Object::real(2.0).as_i32(), None);
        assert_eq!(Object::integer(2).as_number(), Some(2.0));
        assert_eq!(Object::real(2.5).as_number(), Some(2.5));
        assert_eq!(Object::null().as_number(), None);
    }

    #[test]
    fn composite_constructors_round_trip() {
        let a = Object::array(Space::Global, H, 12);
        assert_eq!(a.ty(), Type::Array);
        assert_eq!(a.space(), Some(Space::Global));
        assert_eq!(a.handle(), Some(H));
        assert_eq!(a.offset(), Some(0));
        assert_eq!(a.length(), Some(12));
        assert!(a.is_composite());
        assert!(!a.is_packed());

        let s = Object::string(Space::Local, H, 3);
        assert_eq!(s.ty(), Type::String);
        assert_eq!(s.space(), Some(Space::Local));

        assert_eq!(Object::dict(Space::Local, H).ty(), Type::Dict);
        assert_eq!(Object::file(Space::Local, H).ty(), Type::File);
        assert_eq!(Object::gstate(Space::Local, H).ty(), Type::GState);
        assert_eq!(
            Object::dict(Space::Global, H).composite_ref(),
            Some(CompositeRef {
                space: Space::Global,
                handle: H,
                offset: 0,
                length: 0
            })
        );
    }

    #[test]
    fn packed_array_is_read_only_array_with_packed_bit() {
        let p = Object::packed_array(Space::Local, H, 2);
        assert_eq!(p.ty(), Type::PackedArray);
        assert!(p.is_packed());
        assert_eq!(p.access(), Some(Access::ReadOnly));
        assert_eq!(p.ty().name(), "packedarraytype");
        assert!(p.eq(Object::array(Space::Local, H, 2)));
    }

    #[test]
    fn executable_attribute_is_per_object() {
        let lit = Object::array(Space::Local, H, 1);
        assert!(lit.is_literal());
        let proc_ = lit.as_executable();
        assert!(proc_.is_executable());
        assert!(lit.is_literal());
        assert!(proc_.as_literal().is_literal());
        assert!(Object::operator(1).is_executable());
        assert!(Object::integer(1).with_exec(true).is_executable());
        assert!(lit.eq(proc_));
    }

    #[test]
    fn access_attribute_is_per_object_for_arrays_strings_files() {
        for o in [
            Object::array(Space::Local, H, 1),
            Object::packed_array(Space::Local, H, 1),
            Object::string(Space::Local, H, 1),
            Object::file(Space::Local, H),
        ] {
            let ro = o.with_access(Access::ReadOnly).unwrap();
            assert_eq!(ro.access(), Some(Access::ReadOnly));
            let xo = ro.with_access(Access::ExecuteOnly).unwrap();
            assert_eq!(xo.access(), Some(Access::ExecuteOnly));
            assert_eq!(ro.access(), Some(Access::ReadOnly));
            let none = xo.with_access(Access::None).unwrap();
            assert_eq!(none.access(), Some(Access::None));
            assert_eq!(
                none.with_access(Access::Unlimited).unwrap().access(),
                Some(Access::Unlimited)
            );
            assert_eq!(none.ty(), o.ty());
            assert!(none.eq(o));
        }
        let fresh = Object::array(Space::Local, H, 1);
        assert_eq!(fresh.access(), Some(Access::Unlimited));
    }

    #[test]
    fn access_is_not_an_object_property_for_dicts_and_simple_objects() {
        for o in [
            Object::dict(Space::Local, H),
            Object::gstate(Space::Local, H),
            Object::integer(1),
            Object::name(Atom(0)),
            Object::null(),
        ] {
            assert_eq!(o.access(), None);
            assert!(o.with_access(Access::ReadOnly).is_none());
        }
    }

    #[test]
    fn access_levels_order_from_most_to_least_permissive() {
        assert!(Access::Unlimited < Access::ReadOnly);
        assert!(Access::ReadOnly < Access::ExecuteOnly);
        assert!(Access::ExecuteOnly < Access::None);
    }

    #[test]
    fn interval_narrows_without_allocating() {
        let s = Object::string(Space::Local, H, 10);
        let t = s.with_interval(2, 3).unwrap();
        assert_eq!(t.offset(), Some(2));
        assert_eq!(t.length(), Some(3));
        assert_eq!(t.handle(), Some(H));
        let u = t.with_interval(1, 2).unwrap();
        assert_eq!(u.offset(), Some(3));
        assert_eq!(u.length(), Some(2));
        assert!(t.with_interval(1, 3).is_none());
        assert!(t.with_interval(3, 0).is_some());
        assert!(t.with_interval(4, 0).is_none());
        assert!(s.with_interval(u32::MAX, 1).is_none());
        assert!(Object::dict(Space::Local, H).with_interval(0, 0).is_none());
        assert!(Object::integer(1).with_interval(0, 0).is_none());
        let ro = s.with_access(Access::ReadOnly).unwrap();
        assert_eq!(
            ro.with_interval(0, 1).unwrap().access(),
            Some(Access::ReadOnly)
        );
    }

    #[test]
    fn eq_numbers_compare_across_types() {
        assert!(Object::integer(1).eq(Object::integer(1)));
        assert!(!Object::integer(1).eq(Object::integer(2)));
        assert!(Object::integer(1).eq(Object::real(1.0)));
        assert!(Object::real(1.0).eq(Object::integer(1)));
        assert!(Object::real(0.5).eq(Object::real(0.5)));
        assert!(!Object::real(0.5).eq(Object::integer(0)));
        assert!(!Object::real(f32::NAN).eq(Object::real(f32::NAN)));
        assert!(Object::real(0.0).eq(Object::real(-0.0)));
        assert!(!Object::integer(1).eq(Object::boolean(true)));
    }

    #[test]
    fn eq_simple_values() {
        assert!(Object::boolean(true).eq(Object::boolean(true)));
        assert!(!Object::boolean(true).eq(Object::boolean(false)));
        assert!(Object::null().eq(Object::null()));
        assert!(Object::mark().eq(Object::mark()));
        assert!(!Object::null().eq(Object::mark()));
        assert!(Object::name(Atom(1)).eq(Object::name(Atom(1))));
        assert!(!Object::name(Atom(1)).eq(Object::name(Atom(2))));
        assert!(Object::name(Atom(1)).eq(Object::name(Atom(1)).as_executable()));
        assert!(Object::operator(3).eq(Object::operator(3)));
        assert!(!Object::operator(3).eq(Object::operator(4)));
        assert!(!Object::operator(3).eq(Object::integer(3)));
        assert!(Object::save(0).eq(Object::save(0)));
        assert!(!Object::save(0).eq(Object::save(1)));
        assert!(Object::font_id(5).eq(Object::font_id(5)));
        assert!(!Object::font_id(5).eq(Object::font_id(6)));
        assert!(!Object::name(Atom(1)).eq(Object::integer(1)));
    }

    #[test]
    fn eq_composites_by_identity() {
        let a = Object::array(Space::Local, H, 4);
        assert!(a.eq(a));
        assert!(a.eq(a.as_executable()));
        assert!(a.eq(a.with_access(Access::ReadOnly).unwrap()));
        assert!(!a.eq(Object::array(Space::Global, H, 4)));
        assert!(!a.eq(Object::array(Space::Local, Handle(8), 4)));
        assert!(!a.eq(a.with_interval(0, 3).unwrap()));
        assert!(!a.eq(a.with_interval(1, 3).unwrap()));
        assert!(
            a.with_interval(1, 2)
                .unwrap()
                .eq(a.with_interval(1, 2).unwrap())
        );
        assert!(!a.eq(Object::string(Space::Local, H, 4)));
        assert!(Object::dict(Space::Local, H).eq(Object::dict(Space::Local, H)));
        assert!(!Object::dict(Space::Local, H).eq(Object::dict(Space::Global, H)));
        assert!(!Object::dict(Space::Local, H).eq(Object::gstate(Space::Local, H)));
        assert!(Object::file(Space::Local, H).eq(Object::file(Space::Local, H)));
        assert!(!a.eq(Object::integer(7)));
    }

    #[test]
    fn type_names() {
        assert_eq!(Object::integer(0).ty().name(), "integertype");
        assert_eq!(Object::real(0.0).ty().name(), "realtype");
        assert_eq!(Object::boolean(false).ty().name(), "booleantype");
        assert_eq!(Object::null().ty().name(), "nulltype");
        assert_eq!(Object::mark().ty().name(), "marktype");
        assert_eq!(Object::name(Atom(0)).ty().name(), "nametype");
        assert_eq!(Object::operator(0).ty().name(), "operatortype");
        assert_eq!(Object::save(0).ty().name(), "savetype");
        assert_eq!(Object::font_id(0).ty().name(), "fonttype");
        assert_eq!(Object::array(Space::Local, H, 0).ty().name(), "arraytype");
        assert_eq!(Object::string(Space::Local, H, 0).ty().name(), "stringtype");
        assert_eq!(Object::dict(Space::Local, H).ty().name(), "dicttype");
        assert_eq!(Object::file(Space::Local, H).ty().name(), "filetype");
        assert_eq!(Object::gstate(Space::Local, H).ty().name(), "gstatetype");
    }

    #[test]
    fn debug_output_is_readable() {
        let s = format!("{:?}", Object::integer(3));
        assert!(s.contains("integertype") && s.contains("3"));
        let s = format!("{:?}", Object::string(Space::Global, H, 2).as_executable());
        assert!(s.contains("stringtype") && s.contains("Global") && s.contains("exec: true"));
    }
}
