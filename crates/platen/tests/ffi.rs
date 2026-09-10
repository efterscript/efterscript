// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The query scenario through the C functions, with raw pointers as a C
//! host would pass them, then a page job's document and an error's
//! codes.

use std::ffi::{CStr, CString};
use std::ptr;

use platen::ffi::{
    PLATEN_ABI_VERSION, PLATEN_DONE, PLATEN_ERR_STATE, PLATEN_OK, PLATEN_OUTCOME_BUDGET,
    PLATEN_OUTCOME_ERROR, PLATEN_OUTCOME_OK, platen_config, platen_entry, platen_job,
    platen_job_error_name, platen_job_feed, platen_job_finish, platen_job_free, platen_job_new,
    platen_job_offending, platen_job_pages, platen_job_pdf, platen_job_read_errors,
    platen_job_read_replies, platen_last_error,
};

fn config(entries: &[platen_entry], prelude: &[u8], budget: u64) -> platen_config {
    platen_config {
        abi_version: PLATEN_ABI_VERSION,
        identity: if entries.is_empty() {
            ptr::null()
        } else {
            entries.as_ptr()
        },
        identity_len: entries.len(),
        prelude: if prelude.is_empty() {
            ptr::null()
        } else {
            prelude.as_ptr()
        },
        prelude_len: prelude.len(),
        server_password: 0,
        compress: 0,
        embed_all_fonts: 0,
        step_budget: budget,
    }
}

fn feed(job: *mut platen_job, text: &str) -> i32 {
    unsafe { platen_job_feed(job, text.as_ptr(), text.len()) }
}

/// Drains a channel through a small buffer, as a host loop would.
fn drain(job: *mut platen_job, errors: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 5];
    loop {
        let n = unsafe {
            if errors {
                platen_job_read_errors(job, buf.as_mut_ptr(), buf.len())
            } else {
                platen_job_read_replies(job, buf.as_mut_ptr(), buf.len())
            }
        };
        if n == 0 {
            break out;
        }
        out.extend_from_slice(&buf[..n]);
    }
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(platen_last_error()) }
        .to_string_lossy()
        .into_owned()
}

#[test]
fn the_query_scenario_through_the_abi() {
    let key = CString::new("product").unwrap();
    let value = CString::new("(Fictional Press)").unwrap();
    let entries = [platen_entry {
        key: key.as_ptr(),
        value: value.as_ptr(),
    }];
    let cfg = config(&entries, b"", 0);
    let job = unsafe { platen_job_new(&cfg) };
    assert!(!job.is_null(), "{}", last_error());
    assert_eq!(feed(job, "statusdict /pro"), PLATEN_OK);
    assert!(drain(job, false).is_empty());
    assert_eq!(feed(job, "duct get"), PLATEN_OK);
    assert!(drain(job, false).is_empty());
    assert_eq!(feed(job, " = flush"), PLATEN_OK);
    assert_eq!(drain(job, false), b"Fictional Press\n");
    assert!(drain(job, true).is_empty());
    let mut len = 1;
    assert!(unsafe { platen_job_pdf(job, &mut len) }.is_null());
    assert_eq!(len, 0);
    assert_eq!(unsafe { platen_job_finish(job) }, PLATEN_OUTCOME_OK);
    let pdf = unsafe { platen_job_pdf(job, &mut len) };
    assert!(!pdf.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(pdf, len) };
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(unsafe { platen_job_pages(job) }, 0);
    assert_eq!(
        unsafe { CStr::from_ptr(platen_job_error_name(job)) }.to_bytes(),
        b""
    );
    assert_eq!(feed(job, "1 ="), PLATEN_ERR_STATE);
    assert_eq!(last_error(), "feed after finish");
    assert_eq!(unsafe { platen_job_finish(job) }, PLATEN_ERR_STATE);
    unsafe { platen_job_free(job) };
}

#[test]
fn a_page_job_and_a_prelude_through_the_abi() {
    let prelude = b"/box { 0 0 72 72 rectfill } def";
    let cfg = config(&[], prelude, 0);
    let job = unsafe { platen_job_new(&cfg) };
    assert!(!job.is_null(), "{}", last_error());
    assert_eq!(feed(job, "box showpage box show"), PLATEN_OK);
    assert_eq!(unsafe { platen_job_pages(job) }, 1);
    // A token ends at a delimiter, so the page shows once one arrives.
    assert_eq!(feed(job, "page"), PLATEN_OK);
    assert_eq!(unsafe { platen_job_pages(job) }, 1);
    assert_eq!(feed(job, "\n"), PLATEN_OK);
    assert_eq!(unsafe { platen_job_pages(job) }, 2);
    assert_eq!(unsafe { platen_job_finish(job) }, PLATEN_OUTCOME_OK);
    assert_eq!(unsafe { platen_job_pages(job) }, 2);
    let mut len = 0;
    let pdf = unsafe { platen_job_pdf(job, &mut len) };
    let bytes = unsafe { std::slice::from_raw_parts(pdf, len) };
    assert!(bytes.windows(8).any(|w| w == b"/Count 2"));
    unsafe { platen_job_free(job) };
}

#[test]
fn an_error_and_the_budget_through_the_abi() {
    let cfg = config(&[], b"", 0);
    let job = unsafe { platen_job_new(&cfg) };
    assert_eq!(feed(job, "(a) = 1 0 div (b) ="), PLATEN_DONE);
    assert_eq!(drain(job, false), b"a\n");
    assert_eq!(
        drain(job, true),
        b"%%[ Error: undefinedresult; OffendingCommand: div ]%%\n"
    );
    assert_eq!(feed(job, "(c) ="), PLATEN_DONE);
    assert_eq!(unsafe { platen_job_finish(job) }, PLATEN_OUTCOME_ERROR);
    assert_eq!(
        unsafe { CStr::from_ptr(platen_job_error_name(job)) }.to_bytes(),
        b"undefinedresult"
    );
    assert_eq!(
        unsafe { CStr::from_ptr(platen_job_offending(job)) }.to_bytes(),
        b"div"
    );
    unsafe { platen_job_free(job) };

    let cfg = config(&[], b"", 500);
    let job = unsafe { platen_job_new(&cfg) };
    assert_eq!(feed(job, "{ } loop\n"), PLATEN_DONE);
    assert_eq!(unsafe { platen_job_finish(job) }, PLATEN_OUTCOME_BUDGET);
    unsafe { platen_job_free(job) };
}

#[test]
fn a_rejected_identity_and_a_failing_prelude_return_null() {
    let key = CString::new("product").unwrap();
    let value = CString::new("add").unwrap();
    let entries = [platen_entry {
        key: key.as_ptr(),
        value: value.as_ptr(),
    }];
    let cfg = config(&entries, b"", 0);
    assert!(unsafe { platen_job_new(&cfg) }.is_null());
    assert_eq!(
        last_error(),
        "identity entry product: `add` is not a literal"
    );
    let cfg = config(&[], b"1 0 div", 0);
    assert!(unsafe { platen_job_new(&cfg) }.is_null());
    assert_eq!(last_error(), "prelude failed: undefinedresult in div");
}
