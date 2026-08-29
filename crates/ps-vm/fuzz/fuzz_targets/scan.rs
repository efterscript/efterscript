// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Scans arbitrary bytes with a slice source and a resolver that defines
//! nothing. Any input must scan to completion or a `ScanError`, never a
//! panic, and must not allocate more VM objects than it has bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ps_vm::{Memory, Space, scan_all};

fuzz_target!(|data: &[u8]| {
    for global in [false, true] {
        let mut memory = Memory::new();
        memory.set_global(global);
        let tokens = scan_all(data, &mut memory, &mut ());
        if let Ok(tokens) = tokens {
            assert!(tokens.len() <= data.len() + 1);
        }
        let slots = memory.arena(Space::Local).slot_count() + memory.arena(Space::Global).slot_count();
        assert!(slots <= data.len() + 1);
    }
});
