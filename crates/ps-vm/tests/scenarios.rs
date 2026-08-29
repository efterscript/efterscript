// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `corpus/unit/vm/*.ps` scenarios, driven through the public `Memory`
//! API until an interpreter can run the corpus itself. Each test names the
//! corpus file it mirrors.

use ps_vm::{Access, Memory, Object, Space, Stream, Type, VmError};

fn ints(values: &[i32]) -> Vec<Object> {
    values.iter().map(|&v| Object::integer(v)).collect()
}

fn get(m: &Memory, a: Object, i: usize) -> i32 {
    m.array_get(a, i).unwrap().as_i32().unwrap()
}

// composite-copies-share.ps
#[test]
fn composite_copies_share_storage() {
    let mut m = Memory::new();
    let a = m.alloc_array(ints(&[1, 2, 3])).unwrap();
    let b = a;
    m.array_put(b, 1, Object::integer(7)).unwrap();
    assert!(a.eq(b));
    assert_eq!(get(&m, a, 1), 7);
}

// sub-interval-aliasing.ps
#[test]
fn sub_intervals_alias_their_parent() {
    let mut m = Memory::new();
    let s = m.alloc_string(b"abcdef".to_vec());
    let t = s.with_interval(2, 3).unwrap();
    m.string_put(t, 0, 65).unwrap();
    assert_eq!(m.string_get(s, 2), Ok(65));
    assert!(!s.eq(t));
}

// readonly-array-per-object.ps
#[test]
fn readonly_array_leaves_earlier_copies_writable() {
    let mut m = Memory::new();
    let a = m.alloc_array(ints(&[0, 0])).unwrap();
    let b = a.with_access(Access::ReadOnly).unwrap();
    m.array_put(a, 0, Object::integer(1)).unwrap();
    assert_eq!(get(&m, a, 0), 1);
    assert_eq!(
        m.array_put(b, 0, Object::integer(1)),
        Err(VmError::InvalidAccess)
    );
}

// readonly-dict-shared.ps
#[test]
fn readonly_dictionary_affects_all_references() {
    let mut m = Memory::new();
    let d = m.new_dict(2);
    let e = d;
    let k = m.intern(b"k").unwrap();
    m.dict_set_access(e, Access::ReadOnly).unwrap();
    assert_eq!(
        m.dict_put(d, k, Object::integer(1)),
        Err(VmError::InvalidAccess)
    );
}

// restore-reverts-local.ps
#[test]
fn local_values_revert() {
    let mut m = Memory::new();
    let a = m.alloc_array(ints(&[1, 2, 3])).unwrap();
    let save = m.save(0).unwrap();
    m.array_put(a, 0, Object::integer(9)).unwrap();
    m.restore(save, &[]).unwrap();
    assert_eq!(get(&m, a, 0), 1);
}

// restore-rejects-stack-objects.ps
#[test]
fn objects_created_after_save_are_rejected_on_the_stack() {
    let mut m = Memory::new();
    let save = m.save(0).unwrap();
    let fresh = m.alloc_array(ints(&[1, 2])).unwrap();
    let operand_stack = [save, fresh];
    assert_eq!(
        m.restore(save, &[&operand_stack]),
        Err(VmError::InvalidRestore)
    );
    assert!(m.save_is_live(save));
}

// restore-keeps-global.ps
#[test]
fn global_values_persist() {
    let mut m = Memory::new();
    m.set_global(true);
    let g = m.alloc_array(ints(&[1])).unwrap();
    m.set_global(false);
    let save = m.save(0).unwrap();
    m.array_put(g, 0, Object::integer(2)).unwrap();
    m.restore(save, &[]).unwrap();
    assert_eq!(get(&m, g, 0), 2);
}

// global-dict-rejects-local.ps
#[test]
fn def_into_a_global_dictionary() {
    let mut m = Memory::new();
    m.set_global(true);
    let gd = m.new_dict(1);
    m.set_global(false);
    let la = m.alloc_array(ints(&[1])).unwrap();
    let k = m.intern(b"k").unwrap();
    assert_eq!(m.dict_put(gd, k, la), Err(VmError::InvalidAccess));
    assert_eq!(m.dict_len(gd), Ok(0));
}

// forall-insertion-order.ps
#[test]
fn forall_order_is_insertion_order() {
    let mut m = Memory::new();
    let d = m.new_dict(2);
    let b = m.intern(b"b").unwrap();
    let a = m.intern(b"a").unwrap();
    m.dict_put(d, b, Object::integer(1)).unwrap();
    m.dict_put(d, a, Object::integer(2)).unwrap();
    let keys: Vec<&[u8]> = m
        .dict_entries(d)
        .unwrap()
        .into_iter()
        .map(|(k, _)| m.name_text(k.as_name().unwrap()))
        .collect();
    assert_eq!(keys, [&b"b"[..], &b"a"[..]]);
}

// file-without-capability.ps
#[test]
fn file_with_no_capability() {
    let mut m = Memory::new();
    assert_eq!(
        m.open_file(b"/etc/passwd", b"r").err(),
        Some(VmError::UndefinedFileName)
    );
    assert_eq!(m.files().watermark(), 0);
}

// Embedder-issued streams are the only way a file object comes to exist.
#[test]
fn embedder_streams_become_file_objects() {
    struct Sink(Vec<u8>);
    impl Stream for Sink {
        fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
            Ok(0)
        }
        fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
    }
    let mut m = Memory::new();
    let f = m.open_stream(Box::new(Sink(Vec::new())));
    assert_eq!(f.ty(), Type::File);
    assert_eq!(f.space(), Some(Space::Local));
    assert_eq!(m.file_write(f, b"hi"), Ok(2));
    let save = m.save(0).unwrap();
    let g = m.open_stream(Box::new(Sink(Vec::new())));
    m.restore(save, &[]).unwrap();
    assert!(m.file_is_open(f));
    assert!(!m.file_is_open(g));
}
