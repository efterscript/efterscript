/* SPDX-FileCopyrightText: 2026 EfterScript contributors */
/* SPDX-License-Identifier: MIT */

/*
 * platen: the job a host feeds when it acts as a printer.
 *
 * One job is one interpreter seeded with the host's identity entries
 * and prelude, with a PDF document behind it. The host feeds the
 * program in pieces of any size as they arrive from its transport; each
 * feed executes as far as the bytes allow and leaves the reply bytes
 * (the program's standard output) and the error-report bytes (its
 * standard error, the conventional "%%[ Error: ...; OffendingCommand:
 * ... ]%%" lines) for the host to read, so a query is answered while the
 * program is still arriving. finish signals end of data, runs to
 * completion, and closes the document. Nothing survives a job: the host
 * maps one connection to one job and re-sends what a printer would have
 * kept. Status text is the host's business; the library reports facts.
 *
 * Memory: everything a function returns belongs to the job and is valid
 * until platen_job_free; the host copies what it keeps. Nothing the host
 * passes in is retained after the call returns.
 *
 * Threads: none are created, and no function calls back into the host.
 * The interface is single-threaded: platen_last_error reads one static
 * buffer, so a program using several threads serialises its calls.
 *
 * Failures: a function that cannot do its work returns a negative code
 * (PLATEN_ERR_*) and platen_last_error describes why. An internal
 * failure (a Rust panic) is caught at the boundary; the job is then
 * poisoned and every later call on it returns PLATEN_ERR_PANIC, except
 * platen_job_free, which always frees.
 */

#ifndef PLATEN_H
#define PLATEN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The interface this header declares; platen_config.abi_version must
 * hold it. */
#define PLATEN_ABI_VERSION 1u

/* platen_job_feed results. */
#define PLATEN_OK 0            /* the bytes ran; the job waits for more */
#define PLATEN_DONE 1          /* the job ended before its data did (an
                                  uncaught error, quit, or the budget), on
                                  this call or an earlier one; it accepts
                                  no more bytes, and finish reports why */

/* Failure codes (negative), from any function returning int. */
#define PLATEN_ERR_ARGUMENT (-1) /* a null pointer or an unusable config */
#define PLATEN_ERR_STATE (-2)    /* feed after finish, or finish twice */
#define PLATEN_ERR_PANIC (-3)    /* the job is poisoned by an earlier panic */
#define PLATEN_ERR_DOCUMENT (-4) /* the document could not be closed */

/* platen_job_finish outcomes. */
#define PLATEN_OUTCOME_OK 0      /* the program ran to the end of its data */
#define PLATEN_OUTCOME_ERROR 1   /* an uncaught error: see error_name and
                                    offending */
#define PLATEN_OUTCOME_BUDGET 2  /* the execution budget was spent */
#define PLATEN_OUTCOME_PRELUDE 3 /* reserved: a prelude failure fails
                                    platen_job_new instead */

/* A job. Opaque; create with platen_job_new, release with
 * platen_job_free. */
typedef struct platen_job platen_job;

/* One statusdict entry: the key, and the value as PostScript literal
 * text — "(Fictional Press)", "47.0", "true", "/name", "[612 792]",
 * "<< /a 1 >>". Both NUL-terminated. */
typedef struct {
    const char *key;
    const char *value;
} platen_entry;

/* How a job is set up. Zero the struct, then set abi_version and what
 * you need: every pointer may be NULL when its length is 0. */
typedef struct {
    uint32_t abi_version;          /* PLATEN_ABI_VERSION */
    const platen_entry *identity;  /* statusdict entries seeded before the
                                      prelude */
    size_t identity_len;
    const uint8_t *prelude;        /* a program run once at the server
                                      level before the job; its output is
                                      discarded; an error in it fails
                                      platen_job_new */
    size_t prelude_len;
    int32_t server_password;       /* what exitserver expects; 0 default */
    int compress;                  /* non-zero: compress page streams */
    int embed_all_fonts;           /* non-zero: embed the resident faces */
    uint64_t step_budget;          /* objects the job may execute before
                                      it is stopped; 0 = unlimited */
} platen_config;

/* Creates a job: the identity is parsed and seeded, the prelude run,
 * the document started. NULL on failure — a bad configuration, an
 * identity value that is not one literal, a prelude error — with
 * platen_last_error explaining. */
platen_job *platen_job_new(const platen_config *cfg);

/* Appends len bytes of the program and executes as far as they allow.
 * Returns PLATEN_OK while the job waits for more, PLATEN_DONE once it
 * has ended before its data did, or a negative failure code. Reply and
 * error bytes produced by the call wait for platen_job_read_replies and
 * platen_job_read_errors; a query's reply is available as soon as the
 * feed that completed it returns. A token ends at a delimiter: a piece
 * meant to complete a command ends with whitespace, or the command runs
 * when the next piece (or finish) delimits it. */
int platen_job_feed(platen_job *job, const uint8_t *bytes, size_t len);

/* Copies pending reply bytes (the program's standard output) into buf,
 * at most cap, and drops them; returns the count, 0 when nothing is
 * pending. Call until it returns 0. */
size_t platen_job_read_replies(platen_job *job, uint8_t *buf, size_t cap);

/* As platen_job_read_replies, for the error-report bytes (the program's
 * standard error). */
size_t platen_job_read_errors(platen_job *job, uint8_t *buf, size_t cap);

/* Signals end of data, runs the program to completion, closes the
 * document, and returns the outcome code (PLATEN_OUTCOME_*) or a
 * negative failure code. Output produced by the completion waits for
 * the read functions. After finish the job accepts no feed. */
int platen_job_finish(platen_job *job);

/* The finished document: a pointer to its bytes with *len set (len may
 * be NULL); NULL with *len 0 before finish. Valid until the job is
 * freed. The document is complete whatever the outcome: pages shown
 * before an error are in it. */
const uint8_t *platen_job_pdf(const platen_job *job, size_t *len);

/* The error name of a finished job's error outcome ("undefined",
 * "limitcheck", ...), else "". Valid until the job is freed. */
const char *platen_job_error_name(const platen_job *job);

/* The offending command of a finished job's error outcome, else "". */
const char *platen_job_offending(const platen_job *job);

/* Pages shown so far, or in the finished document. */
uint32_t platen_job_pages(const platen_job *job);

/* Frees the job and everything it returned. NULL is ignored. */
void platen_job_free(platen_job *job);

/* The message of the last failure (a rejected configuration, a prelude
 * error, a panic), or "". One static buffer for the whole library. */
const char *platen_last_error(void);

#ifdef __cplusplus
}
#endif

#endif /* PLATEN_H */
