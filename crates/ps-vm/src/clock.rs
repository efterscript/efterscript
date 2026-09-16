// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The real-time clock capability behind `realtime` (PLRM3 §8.2). The
//! library has no time source of its own: an embedder that wants
//! `realtime` to track wall-clock time installs a [`Clock`] through
//! `Capabilities::clock`; without one, `realtime` answers the execution
//! clock `usertime` reports, so output stays deterministic.

/// A clock counting real time in milliseconds from an arbitrary origin.
pub trait Clock {
    /// The current reading. It wraps like the operator's result does,
    /// so an embedder may hand over the low 32 bits of any counter.
    fn realtime_ms(&mut self) -> i32;
}
