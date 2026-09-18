// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The C ABI declared by `include/platen.h`: a [`Job`] behind an opaque
//! pointer, fed and drained through plain functions. Every function is
//! `extern "C"`, catches a panic at the boundary (unwinding into C is
//! undefined) and answers it with a failure code, after which the job is
//! poisoned and every later call on it fails the same way; the message
//! is kept for [`platen_last_error`]. All memory a function returns is
//! the job's and lives until [`platen_job_free`]. No function calls back
//! into the host or creates a thread, and the whole interface is
//! single-threaded: the last-error buffer is one static, so a program
//! calling it from several threads must serialise them itself.

// The crate denies unsafe code; this module, the C boundary, is the one
// site where it is permitted: raw pointers from the host, the opaque job
// handle, and the static last-error buffer need it.
#![allow(unsafe_code)]
// The C names are the header's.
#![allow(non_camel_case_types)]

use std::ffi::{CStr, c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::{Finished, Job, JobConfig, JobError, Options, Outcome};

/// The version of `platen.h` this library implements; a configuration
/// naming another is refused.
pub const PLATEN_ABI_VERSION: u32 = 1;

/// `platen_job_feed`: the bytes ran and the job waits for more.
pub const PLATEN_OK: c_int = 0;
/// `platen_job_feed`: the job has ended before its data did (on this
/// call or an earlier one); it accepts no more bytes, and `finish`
/// reports the outcome.
pub const PLATEN_DONE: c_int = 1;
/// A null pointer or an unusable configuration.
pub const PLATEN_ERR_ARGUMENT: c_int = -1;
/// A call the job's state does not allow: `feed` after `finish`, or
/// `finish` twice.
pub const PLATEN_ERR_STATE: c_int = -2;
/// The job is poisoned: an earlier call panicked.
pub const PLATEN_ERR_PANIC: c_int = -3;
/// The document could not be closed.
pub const PLATEN_ERR_DOCUMENT: c_int = -4;

/// `platen_job_finish` outcomes.
pub const PLATEN_OUTCOME_OK: c_int = 0;
pub const PLATEN_OUTCOME_ERROR: c_int = 1;
pub const PLATEN_OUTCOME_BUDGET: c_int = 2;
/// Reserved: a prelude failure fails `platen_job_new` instead.
pub const PLATEN_OUTCOME_PRELUDE: c_int = 3;

/// One identity entry: a key and its value as PostScript literal text.
#[repr(C)]
pub struct platen_entry {
    pub key: *const c_char,
    pub value: *const c_char,
}

/// The configuration `platen_job_new` reads; see the header.
#[repr(C)]
pub struct platen_config {
    pub abi_version: u32,
    pub identity: *const platen_entry,
    pub identity_len: usize,
    pub prelude: *const u8,
    pub prelude_len: usize,
    pub server_password: i32,
    pub compress: c_int,
    pub embed_all_fonts: c_int,
    /// 0 is unlimited.
    pub step_budget: u64,
}

enum Stage {
    Running(Box<Job>),
    Finished(Box<Finished>),
    /// A panic left the job unusable.
    Poisoned,
}

/// The opaque job the host holds.
pub struct platen_job {
    stage: Stage,
    /// Reply bytes not yet read by the host.
    replies: Vec<u8>,
    /// Error-report bytes not yet read by the host.
    errors: Vec<u8>,
    /// The error name and offending command as C strings, once finished.
    error_name: Vec<u8>,
    offending: Vec<u8>,
}

const EMPTY: &[u8] = b"\0";

// --- the last-error buffer ----------------------------------------------------------

const LAST_ERROR_CAP: usize = 512;

/// The last failure's message, NUL-terminated. One static, unguarded:
/// the interface is single-threaded by contract.
static mut LAST_ERROR: [u8; LAST_ERROR_CAP] = [0; LAST_ERROR_CAP];

fn set_last_error(message: &str) {
    let bytes = message.as_bytes();
    let n = bytes.len().min(LAST_ERROR_CAP - 1);
    // SAFETY: single-threaded by the interface's contract; the buffer is
    // only ever accessed from these two functions.
    unsafe {
        let buffer = &raw mut LAST_ERROR;
        ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>(), n);
        *(buffer.cast::<u8>()).add(n) = 0;
    }
}

/// The message of the last failure (a null configuration, a rejected
/// identity value, a prelude error, a panic), or an empty string.
#[unsafe(no_mangle)]
pub extern "C" fn platen_last_error() -> *const c_char {
    (&raw const LAST_ERROR).cast::<c_char>()
}

// --- the boundary -------------------------------------------------------------------

/// Runs `f` on the job behind `job`, answering a null pointer, a
/// poisoned job, or a panic with `failure`. A panic poisons the job.
fn guarded<T>(job: *mut platen_job, failure: T, f: impl FnOnce(&mut platen_job) -> T) -> T {
    if job.is_null() {
        set_last_error("null job");
        return failure;
    }
    // SAFETY: a non-null `job` came from `platen_job_new` and has not
    // been freed, by the header's contract.
    let job = unsafe { &mut *job };
    if matches!(job.stage, Stage::Poisoned) {
        return failure;
    }
    match catch_unwind(AssertUnwindSafe(|| f(job))) {
        Ok(value) => value,
        Err(payload) => {
            job.stage = Stage::Poisoned;
            set_last_error(&format!("panic: {}", panic_message(&payload)));
            failure
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown".to_string()
    }
}

// --- the functions ------------------------------------------------------------------

/// Reads `cfg` into a [`JobConfig`]; `None` with the last error set.
///
/// # Safety
///
/// `cfg` and every pointer in it must be valid as the header describes.
unsafe fn read_config(cfg: *const platen_config) -> Option<JobConfig> {
    if cfg.is_null() {
        set_last_error("null configuration");
        return None;
    }
    // SAFETY: the caller's contract.
    let cfg = unsafe { &*cfg };
    if cfg.abi_version != PLATEN_ABI_VERSION {
        set_last_error(&format!(
            "abi version {} requested, {} implemented",
            cfg.abi_version, PLATEN_ABI_VERSION
        ));
        return None;
    }
    let mut identity = Vec::with_capacity(cfg.identity_len);
    if cfg.identity_len > 0 {
        if cfg.identity.is_null() {
            set_last_error("null identity with a length");
            return None;
        }
        // SAFETY: the caller's contract.
        let entries = unsafe { std::slice::from_raw_parts(cfg.identity, cfg.identity_len) };
        for (index, entry) in entries.iter().enumerate() {
            if entry.key.is_null() || entry.value.is_null() {
                set_last_error(&format!("identity entry {index}: null key or value"));
                return None;
            }
            // SAFETY: the caller's contract.
            let (key, value) = unsafe { (CStr::from_ptr(entry.key), CStr::from_ptr(entry.value)) };
            let text = |s: &CStr| String::from_utf8_lossy(s.to_bytes()).into_owned();
            identity.push((text(key), text(value)));
        }
    }
    let prelude = if cfg.prelude_len > 0 {
        if cfg.prelude.is_null() {
            set_last_error("null prelude with a length");
            return None;
        }
        // SAFETY: the caller's contract.
        Some(unsafe { std::slice::from_raw_parts(cfg.prelude, cfg.prelude_len) }.to_vec())
    } else {
        None
    };
    let mut options = Options::compress(cfg.compress != 0);
    options.params.embed_all_fonts = cfg.embed_all_fonts != 0;
    Some(JobConfig {
        identity,
        prelude,
        server_password: cfg.server_password,
        options,
        step_budget: (cfg.step_budget > 0).then_some(cfg.step_budget),
    })
}

/// Creates a job; NULL on failure, with `platen_last_error` set.
///
/// # Safety
///
/// `cfg` and every pointer in it must be valid as the header describes;
/// none of them is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_new(cfg: *const platen_config) -> *mut platen_job {
    let created = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller's contract.
        let config = unsafe { read_config(cfg) }?;
        match Job::new(config) {
            Ok(job) => Some(job),
            Err(e) => {
                set_last_error(&e.to_string());
                None
            }
        }
    }));
    match created {
        Ok(Some(job)) => Box::into_raw(Box::new(platen_job {
            stage: Stage::Running(Box::new(job)),
            replies: Vec::new(),
            errors: Vec::new(),
            error_name: Vec::new(),
            offending: Vec::new(),
        })),
        Ok(None) => ptr::null_mut(),
        Err(payload) => {
            set_last_error(&format!("panic: {}", panic_message(&payload)));
            ptr::null_mut()
        }
    }
}

/// Feeds `len` bytes; see the header for the codes.
///
/// # Safety
///
/// `job` is a live job; `bytes` points to `len` readable bytes (or `len`
/// is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_feed(
    job: *mut platen_job,
    bytes: *const u8,
    len: usize,
) -> c_int {
    if bytes.is_null() && len > 0 {
        set_last_error("null bytes with a length");
        return PLATEN_ERR_ARGUMENT;
    }
    // SAFETY: the caller's contract.
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    guarded(job, PLATEN_ERR_PANIC, |job| match &mut job.stage {
        Stage::Running(running) => match running.feed(bytes) {
            Ok(progress) => {
                job.replies.extend_from_slice(&progress.replies);
                job.errors.extend_from_slice(&progress.errors);
                if progress.done {
                    PLATEN_DONE
                } else {
                    PLATEN_OK
                }
            }
            Err(JobError::Finished) => PLATEN_DONE,
            Err(e) => {
                set_last_error(&e.to_string());
                PLATEN_ERR_STATE
            }
        },
        Stage::Finished(_) => {
            set_last_error("feed after finish");
            PLATEN_ERR_STATE
        }
        Stage::Poisoned => PLATEN_ERR_PANIC,
    })
}

/// Copies up to `cap` bytes of the pending output in `pending` into
/// `buf` and drops them; 0 when nothing is pending.
fn read_pending(pending: &mut Vec<u8>, buf: *mut u8, cap: usize) -> usize {
    if buf.is_null() || cap == 0 || pending.is_empty() {
        return 0;
    }
    let n = cap.min(pending.len());
    // SAFETY: the caller's contract: `buf` holds `cap` writable bytes.
    unsafe { ptr::copy_nonoverlapping(pending.as_ptr(), buf, n) };
    pending.drain(..n);
    n
}

/// Drains reply bytes into `buf`; call until it returns 0.
///
/// # Safety
///
/// `job` is a live job; `buf` points to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_read_replies(
    job: *mut platen_job,
    buf: *mut u8,
    cap: usize,
) -> usize {
    guarded(job, 0, |job| read_pending(&mut job.replies, buf, cap))
}

/// Drains error-report bytes into `buf`; call until it returns 0.
///
/// # Safety
///
/// `job` is a live job; `buf` points to `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_read_errors(
    job: *mut platen_job,
    buf: *mut u8,
    cap: usize,
) -> usize {
    guarded(job, 0, |job| read_pending(&mut job.errors, buf, cap))
}

/// Ends the data, runs to completion, closes the document, and returns
/// the outcome code (or a negative failure code).
///
/// # Safety
///
/// `job` is a live job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_finish(job: *mut platen_job) -> c_int {
    guarded(job, PLATEN_ERR_PANIC, |job| {
        let stage = std::mem::replace(&mut job.stage, Stage::Poisoned);
        let running = match stage {
            Stage::Running(running) => *running,
            Stage::Finished(finished) => {
                job.stage = Stage::Finished(finished);
                set_last_error("finish twice");
                return PLATEN_ERR_STATE;
            }
            Stage::Poisoned => return PLATEN_ERR_PANIC,
        };
        match running.finish() {
            Ok(finished) => {
                job.replies.extend_from_slice(&finished.replies);
                job.errors.extend_from_slice(&finished.errors);
                let (name, offending) = match &finished.outcome {
                    Outcome::Error { name, offending } => (name.as_str(), offending.as_str()),
                    Outcome::Ok | Outcome::Budget => ("", ""),
                };
                job.error_name = c_string(name);
                job.offending = c_string(offending);
                let code = outcome_code(&finished.outcome);
                job.stage = Stage::Finished(Box::new(finished));
                code
            }
            Err(e) => {
                set_last_error(&e.to_string());
                PLATEN_ERR_DOCUMENT
            }
        }
    })
}

fn outcome_code(outcome: &Outcome) -> c_int {
    match outcome {
        Outcome::Ok => PLATEN_OUTCOME_OK,
        Outcome::Error { .. } => PLATEN_OUTCOME_ERROR,
        Outcome::Budget => PLATEN_OUTCOME_BUDGET,
    }
}

fn c_string(text: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = text.bytes().filter(|&b| b != 0).collect();
    bytes.push(0);
    bytes
}

/// The finished document; NULL with `*len` 0 before `finish`. Valid
/// until the job is freed.
///
/// # Safety
///
/// `job` is a live job; `len` is null or points to a writable `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_pdf(job: *const platen_job, len: *mut usize) -> *const u8 {
    let (pointer, n) = guarded(job.cast_mut(), (ptr::null(), 0), |job| match &job.stage {
        Stage::Finished(finished) => (finished.pdf.as_ptr(), finished.pdf.len()),
        _ => (ptr::null(), 0),
    });
    if !len.is_null() {
        // SAFETY: the caller's contract.
        unsafe { *len = n };
    }
    pointer
}

/// The error name of a finished job's error outcome, else "".
///
/// # Safety
///
/// `job` is a live job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_error_name(job: *const platen_job) -> *const c_char {
    guarded(job.cast_mut(), EMPTY.as_ptr().cast(), |job| {
        if job.error_name.is_empty() {
            EMPTY.as_ptr().cast()
        } else {
            job.error_name.as_ptr().cast()
        }
    })
}

/// The offending command of a finished job's error outcome, else "".
///
/// # Safety
///
/// `job` is a live job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_offending(job: *const platen_job) -> *const c_char {
    guarded(job.cast_mut(), EMPTY.as_ptr().cast(), |job| {
        if job.offending.is_empty() {
            EMPTY.as_ptr().cast()
        } else {
            job.offending.as_ptr().cast()
        }
    })
}

/// Pages shown so far, or in the finished document.
///
/// # Safety
///
/// `job` is a live job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_pages(job: *const platen_job) -> u32 {
    guarded(job.cast_mut(), 0, |job| {
        let pages = match &job.stage {
            Stage::Running(running) => running.pages(),
            Stage::Finished(finished) => finished.report.pages,
            Stage::Poisoned => 0,
        };
        u32::try_from(pages).unwrap_or(u32::MAX)
    })
}

/// Frees the job and everything it returned. A null pointer is ignored.
///
/// # Safety
///
/// `job` is null or a job not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn platen_job_free(job: *mut platen_job) {
    if job.is_null() {
        return;
    }
    // SAFETY: the caller's contract; the box came from `platen_job_new`.
    let boxed = unsafe { Box::from_raw(job) };
    // A panic while dropping (an interpreter mid-suspension) must not
    // cross the boundary either.
    let _ = catch_unwind(AssertUnwindSafe(move || drop(boxed)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The tests below share the one last-error buffer, which the
    /// interface leaves unguarded by contract; they run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn new_job() -> *mut platen_job {
        let cfg = platen_config {
            abi_version: PLATEN_ABI_VERSION,
            identity: ptr::null(),
            identity_len: 0,
            prelude: ptr::null(),
            prelude_len: 0,
            server_password: 0,
            compress: 0,
            embed_all_fonts: 0,
            step_budget: 0,
        };
        let job = unsafe { platen_job_new(&cfg) };
        assert!(!job.is_null());
        job
    }

    #[test]
    fn a_panic_poisons_the_job_and_every_function_answers_the_failure_code() {
        let _serial = serial();
        let job = new_job();
        let answer = guarded(job, -9, |_| panic!("boom"));
        assert_eq!(answer, -9);
        let message = unsafe { CStr::from_ptr(platen_last_error()) };
        assert_eq!(message.to_str().unwrap(), "panic: boom");
        unsafe {
            assert_eq!(platen_job_feed(job, b"1 =".as_ptr(), 3), PLATEN_ERR_PANIC);
            let mut buf = [0u8; 8];
            assert_eq!(platen_job_read_replies(job, buf.as_mut_ptr(), 8), 0);
            assert_eq!(platen_job_read_errors(job, buf.as_mut_ptr(), 8), 0);
            assert_eq!(platen_job_finish(job), PLATEN_ERR_PANIC);
            let mut len = 7;
            assert!(platen_job_pdf(job, &mut len).is_null());
            assert_eq!(len, 0);
            assert_eq!(CStr::from_ptr(platen_job_error_name(job)).to_bytes(), b"");
            assert_eq!(CStr::from_ptr(platen_job_offending(job)).to_bytes(), b"");
            assert_eq!(platen_job_pages(job), 0);
            platen_job_free(job);
        }
    }

    #[test]
    fn null_and_wrong_version_are_refused() {
        let _serial = serial();
        unsafe {
            assert!(platen_job_new(ptr::null()).is_null());
            assert_eq!(
                CStr::from_ptr(platen_last_error()).to_bytes(),
                b"null configuration"
            );
            let cfg = platen_config {
                abi_version: 2,
                identity: ptr::null(),
                identity_len: 0,
                prelude: ptr::null(),
                prelude_len: 0,
                server_password: 0,
                compress: 0,
                embed_all_fonts: 0,
                step_budget: 0,
            };
            assert!(platen_job_new(&cfg).is_null());
            assert!(
                CStr::from_ptr(platen_last_error())
                    .to_str()
                    .unwrap()
                    .starts_with("abi version 2")
            );
            assert_eq!(
                platen_job_feed(ptr::null_mut(), ptr::null(), 0),
                PLATEN_ERR_PANIC
            );
            assert_eq!(platen_job_finish(ptr::null_mut()), PLATEN_ERR_PANIC);
            platen_job_free(ptr::null_mut());
        }
    }

    #[test]
    fn long_messages_are_truncated() {
        let _serial = serial();
        set_last_error(&"x".repeat(2000));
        let message = unsafe { CStr::from_ptr(platen_last_error()) };
        assert_eq!(message.to_bytes().len(), LAST_ERROR_CAP - 1);
    }
}
