// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Injected output streams.
//!
//! The interpreter writes only through streams the embedder hands in; with
//! none, output is discarded. Nothing here touches host stdout or stderr.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::VmError;
use crate::files::Stream;

/// The standard output and error streams of an interpreter.
#[derive(Default)]
pub struct Io {
    pub stdout: Option<Box<dyn Stream>>,
    pub stderr: Option<Box<dyn Stream>>,
}

impl Io {
    pub fn new(stdout: impl Stream + 'static, stderr: impl Stream + 'static) -> Self {
        Io {
            stdout: Some(Box::new(stdout)),
            stderr: Some(Box::new(stderr)),
        }
    }

    /// Streams that discard everything written to them.
    pub fn discard() -> Self {
        Self::default()
    }

    /// Both streams backed by capture buffers, returned with the handles
    /// that read them back.
    pub fn capture() -> (Self, Capture, Capture) {
        let out = Capture::new();
        let err = Capture::new();
        (Io::new(out.clone(), err.clone()), out, err)
    }
}

/// An in-memory sink shared between the interpreter and whoever inspects
/// it.
#[derive(Clone, Debug, Default)]
pub struct Capture(Rc<RefCell<Vec<u8>>>);

impl Capture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }

    /// The captured bytes as text, lossily.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }

    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
}

impl Stream for Capture {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_collects_writes_and_reads_nothing() {
        let (mut io, out, err) = Io::capture();
        io.stdout.as_mut().unwrap().write(b"ab").unwrap();
        io.stdout.as_mut().unwrap().write(b"c").unwrap();
        io.stderr.as_mut().unwrap().write(b"!").unwrap();
        assert_eq!(out.bytes(), b"abc");
        assert_eq!(out.text(), "abc");
        assert_eq!(err.text(), "!");
        assert_eq!(io.stdout.as_mut().unwrap().read(&mut [0; 4]), Ok(0));
        out.clear();
        assert!(out.bytes().is_empty());
        assert!(Io::discard().stdout.is_none());
    }
}
