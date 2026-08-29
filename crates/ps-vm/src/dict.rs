// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Dictionary storage: an insertion-ordered map keyed by objects under `eq`.
//!
//! String keys are converted to names before they reach this type; that
//! conversion needs the name table and lives in [`crate::memory::Memory`].

use std::collections::HashMap;

use crate::object::{Access, Handle, Object, Space, Type};

// The `eq` relation, reified so it can be hashed. Integer-valued reals fold
// onto the integer; attributes are ignored; composites are identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Integer(i32),
    Real(u32),
    Boolean(bool),
    Null,
    Mark,
    Name(u32),
    Operator(u32),
    Save(u32),
    FontId(u32),
    Composite {
        ty: Type,
        space: Space,
        handle: Handle,
        offset: u32,
        length: u32,
    },
}

impl Key {
    fn of(object: Object) -> Self {
        match object.ty() {
            Type::Integer => Key::Integer(object.as_i32().unwrap()),
            Type::Real => {
                let r = object.as_f32().unwrap();
                // `i32::MAX as f32` rounds up to 2^31, so bound with the
                // exclusive limit.
                if r.fract() == 0.0 && r >= i32::MIN as f32 && r < 2_147_483_648.0 {
                    Key::Integer(r as i32)
                } else {
                    Key::Real(r.to_bits())
                }
            }
            Type::Boolean => Key::Boolean(object.as_bool().unwrap()),
            Type::Null => Key::Null,
            Type::Mark => Key::Mark,
            Type::Name => Key::Name(object.as_name().unwrap().0),
            Type::Operator => Key::Operator(object.as_operator().unwrap()),
            Type::Save => Key::Save(object.as_save().unwrap()),
            Type::FontId => Key::FontId(object.as_font_id().unwrap()),
            ty => {
                let r = object.composite_ref().unwrap();
                Key::Composite {
                    ty: if ty == Type::PackedArray {
                        Type::Array
                    } else {
                        ty
                    },
                    space: r.space,
                    handle: r.handle,
                    offset: r.offset,
                    length: r.length,
                }
            }
        }
    }
}

/// Dictionary contents plus the access attribute, which for dictionaries is
/// shared by every reference. Grows without limit; `maxlength` is only
/// reported.
#[derive(Clone, Debug, Default)]
pub struct Dict {
    entries: Vec<(Object, Object)>,
    index: HashMap<Key, usize>,
    access: Access,
    maxlength: u32,
}

impl Dict {
    pub fn new(maxlength: u32) -> Self {
        Dict {
            entries: Vec::with_capacity(maxlength as usize),
            index: HashMap::with_capacity(maxlength as usize),
            access: Access::Unlimited,
            maxlength,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value the program passed to `dict`, or the entry count if it was
    /// exceeded.
    pub fn maxlength(&self) -> u32 {
        self.maxlength
            .max(u32::try_from(self.entries.len()).unwrap_or(u32::MAX))
    }

    pub fn access(&self) -> Access {
        self.access
    }

    pub fn set_access(&mut self, access: Access) {
        self.access = access;
    }

    pub fn get(&self, key: Object) -> Option<Object> {
        self.index.get(&Key::of(key)).map(|&i| self.entries[i].1)
    }

    pub fn contains(&self, key: Object) -> bool {
        self.index.contains_key(&Key::of(key))
    }

    /// Inserts or replaces the value; a replaced entry keeps its position
    /// and its original key object. Returns the previous value.
    pub fn insert(&mut self, key: Object, value: Object) -> Option<Object> {
        match self.index.get(&Key::of(key)) {
            Some(&i) => Some(std::mem::replace(&mut self.entries[i].1, value)),
            None => {
                self.index.insert(Key::of(key), self.entries.len());
                self.entries.push((key, value));
                None
            }
        }
    }

    pub fn remove(&mut self, key: Object) -> Option<Object> {
        let i = self.index.remove(&Key::of(key))?;
        let (_, value) = self.entries.remove(i);
        for slot in self.index.values_mut() {
            if *slot > i {
                *slot -= 1;
            }
        }
        Some(value)
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (Object, Object)> + '_ {
        self.entries.iter().copied()
    }

    pub fn keys(&self) -> impl Iterator<Item = Object> + '_ {
        self.entries.iter().map(|&(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = Object> + '_ {
        self.entries.iter().map(|&(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Atom;

    fn n(i: u32) -> Object {
        Object::name(Atom(i))
    }

    #[test]
    fn insertion_order_is_preserved() {
        let mut d = Dict::new(2);
        d.insert(n(1), Object::integer(1));
        d.insert(n(0), Object::integer(2));
        d.insert(n(1), Object::integer(3));
        let keys: Vec<_> = d.keys().map(|k| k.as_name().unwrap().0).collect();
        assert_eq!(keys, [1, 0]);
        let values: Vec<_> = d.values().map(|v| v.as_i32().unwrap()).collect();
        assert_eq!(values, [3, 2]);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn keys_follow_eq() {
        let mut d = Dict::default();
        d.insert(Object::integer(1), n(0));
        assert_eq!(
            d.get(Object::real(1.0)).map(|o| o.as_name()),
            Some(Some(Atom(0)))
        );
        assert!(d.contains(Object::real(1.0)));
        assert!(d.insert(Object::real(1.0), n(1)).unwrap().eq(n(0)));
        assert_eq!(d.len(), 1);
        assert!(d.keys().next().unwrap().ty() == Type::Integer);
        assert!(d.get(Object::real(1.5)).is_none());
        d.insert(Object::real(-0.0), n(2));
        assert!(d.get(Object::integer(0)).is_some());
        assert!(!d.contains(Object::boolean(true)));
        assert!(!d.contains(Object::integer(2)));
        assert!(d.get(Object::real(2.0)).is_none());
        d.insert(Object::real(2.5), n(3));
        assert!(d.get(Object::real(2.5)).is_some());
        d.insert(Object::real(1e10), n(4));
        assert!(d.get(Object::real(1e10)).is_some());
        assert!(d.get(Object::integer(i32::MAX)).is_none());
    }

    #[test]
    fn attributes_do_not_affect_keys() {
        let mut d = Dict::default();
        let a = Object::array(Space::Local, Handle(3), 2);
        d.insert(a, Object::integer(1));
        assert!(d.contains(a.as_executable()));
        assert!(d.contains(a.with_access(Access::ReadOnly).unwrap()));
        assert!(d.contains(Object::packed_array(Space::Local, Handle(3), 2)));
        assert!(!d.contains(a.with_interval(0, 1).unwrap()));
        assert!(!d.contains(Object::array(Space::Global, Handle(3), 2)));
        assert!(!d.contains(Object::string(Space::Local, Handle(3), 2)));
        d.insert(n(5).as_executable(), Object::integer(2));
        assert_eq!(d.get(n(5)).and_then(Object::as_i32), Some(2));
    }

    #[test]
    fn every_simple_type_can_be_a_key() {
        let mut d = Dict::default();
        let keys = [
            Object::null(),
            Object::mark(),
            Object::boolean(false),
            Object::operator(1),
            Object::save(1),
            Object::font_id(1),
            Object::dict(Space::Global, Handle(0)),
            Object::file(Space::Local, Handle(0)),
        ];
        for (i, k) in keys.iter().enumerate() {
            d.insert(*k, Object::integer(i as i32));
        }
        assert_eq!(d.len(), keys.len());
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(d.get(*k).and_then(Object::as_i32), Some(i as i32));
        }
        assert!(!d.contains(Object::boolean(true)));
        assert!(!d.contains(Object::operator(2)));
    }

    #[test]
    fn remove_keeps_order_of_the_rest() {
        let mut d = Dict::default();
        for i in 0..5 {
            d.insert(n(i), Object::integer(i as i32));
        }
        assert_eq!(d.remove(n(1)).and_then(Object::as_i32), Some(1));
        assert!(d.remove(n(1)).is_none());
        let keys: Vec<_> = d.keys().map(|k| k.as_name().unwrap().0).collect();
        assert_eq!(keys, [0, 2, 3, 4]);
        for i in [0, 2, 3, 4] {
            assert_eq!(d.get(n(i)).and_then(Object::as_i32), Some(i as i32));
        }
        d.insert(n(1), Object::integer(9));
        let keys: Vec<_> = d.keys().map(|k| k.as_name().unwrap().0).collect();
        assert_eq!(keys, [0, 2, 3, 4, 1]);
    }

    #[test]
    fn grows_past_maxlength_and_reports_it() {
        let mut d = Dict::new(1);
        assert!(d.is_empty());
        assert_eq!(d.maxlength(), 1);
        d.insert(n(0), Object::null());
        d.insert(n(1), Object::null());
        assert_eq!(d.len(), 2);
        assert_eq!(d.maxlength(), 2);
        let entries: Vec<_> = d.iter().collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn access_lives_in_storage() {
        let mut d = Dict::default();
        assert_eq!(d.access(), Access::Unlimited);
        d.set_access(Access::ReadOnly);
        assert_eq!(d.clone().access(), Access::ReadOnly);
    }
}
