// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

// The session library's C interface (`platen.h`), compiled for
// `wasm32-unknown-unknown` and driven from JavaScript. The module has no
// imports and no allocator exports, so the wrapper reserves its own
// scratch region by growing the instance's memory: the allocator inside
// only ever uses pages it grew itself, so pages grown here stay ours.
// Every job gets a fresh instance, which is what the interface promises
// anyway (nothing survives a job) and makes a trap cost only that job.

const ABI_VERSION = 1;
const PAGE = 65536;

// `platen_config` on a 32-bit target: pointers and `size_t` are four
// bytes, the step budget is a `u64` aligned to eight.
const CONFIG = {
  abiVersion: 0,
  identity: 4,
  identityLen: 8,
  prelude: 12,
  preludeLen: 16,
  serverPassword: 20,
  compress: 24,
  embedAllFonts: 28,
  stepBudget: 32,
  size: 40,
};
const ENTRY_SIZE = 8;

// Feeds and drains move through one window of the scratch region.
const WINDOW = 64 * 1024;

const OUTCOMES = ["ok", "error", "budget"];

/** Objects a job may execute unless its options say otherwise. */
export const DEFAULT_BUDGET = 100_000_000;

const utf8 = new TextEncoder();

function toBytes(input, what) {
  if (typeof input === "string") return utf8.encode(input);
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (ArrayBuffer.isView(input)) {
    return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  }
  throw new TypeError(`efterscript: ${what} must be a string, an ArrayBuffer, or a typed array`);
}

function decode(bytes) {
  return new TextDecoder("utf-8").decode(bytes);
}

function concat(chunks) {
  const total = chunks.reduce((n, c) => n + c.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

async function compile(source) {
  if (source === undefined) source = new URL("./efterscript.wasm", import.meta.url);
  if (source instanceof WebAssembly.Module) return source;
  if (typeof source === "string") source = new URL(source, import.meta.url);
  if (source instanceof URL) {
    if (source.protocol === "file:") {
      const { readFile } = await import("node:fs/promises");
      return WebAssembly.compile(await readFile(source));
    }
    source = fetch(source);
  }
  if (source instanceof Promise) source = await source;
  if (typeof Response !== "undefined" && source instanceof Response) {
    if (!source.ok) {
      throw new Error(`efterscript: fetching the engine failed (${source.status} ${source.statusText})`);
    }
    // Streaming compilation needs the wasm media type; fall back to a
    // buffered compile for servers that do not send it.
    if (WebAssembly.compileStreaming && source.headers.get("content-type") === "application/wasm") {
      return WebAssembly.compileStreaming(source);
    }
    return WebAssembly.compile(await source.arrayBuffer());
  }
  return WebAssembly.compile(toBytes(source, "the engine"));
}

/**
 * Compiles the engine once; jobs are then cheap to start.
 *
 * `source` is where the WebAssembly module comes from: omitted, it is
 * `efterscript.wasm` beside this file; otherwise a URL, a `Response` (or
 * a promise of one), the module's bytes, or a compiled
 * `WebAssembly.Module`.
 */
export async function load(source) {
  return new Engine(await compile(source));
}

export class Engine {
  #module;

  constructor(module) {
    this.#module = module;
  }

  /** The compiled module, for handing to a worker. */
  get module() {
    return this.#module;
  }

  /** Starts a job; see `JobOptions` in the type declarations. */
  async job(options = {}) {
    const instance = await WebAssembly.instantiate(this.#module, {});
    return new Job(instance, options);
  }

  /** Runs a whole program and returns the finished result. */
  async convert(program, options = {}) {
    const job = await this.job(options);
    try {
      job.feed(program);
      return job.finish();
    } finally {
      job.free();
    }
  }
}

export class Job {
  #exports;
  #memory;
  #handle = 0;
  #scratch = 0;
  #stdout = [];
  #stderr = [];
  #state = "running";

  constructor(instance, options) {
    this.#exports = instance.exports;
    this.#memory = instance.exports.memory;
    this.#create(options);
  }

  /** Whether the program has ended before its data did. */
  get done() {
    return this.#state !== "running";
  }

  /**
   * Appends bytes to the program and runs as far as they allow. Returns
   * what the program wrote since the previous call and whether it has
   * ended (an uncaught error, `quit`, or the budget).
   */
  feed(input) {
    this.#require("running", "feed");
    const bytes = toBytes(input, "the program");
    let ended = false;
    this.#call(() => {
      for (let at = 0; at < bytes.length && !ended; at += WINDOW) {
        const piece = bytes.subarray(at, at + WINDOW);
        this.#view().set(piece, this.#scratch);
        const code = this.#exports.platen_job_feed(this.#handle, this.#scratch, piece.length);
        if (code < 0) this.#fail("feed");
        ended = code === 1;
      }
    });
    if (ended) this.#state = "ended";
    const stdout = this.#drain("platen_job_read_replies");
    const stderr = this.#drain("platen_job_read_errors");
    this.#stdout.push(stdout);
    this.#stderr.push(stderr);
    return { stdout: decode(stdout), stderr: decode(stderr), done: ended };
  }

  /**
   * Signals the end of the data, runs the program to completion, and
   * closes the document. The PDF is complete whatever the outcome: pages
   * shown before an error are in it.
   */
  finish() {
    if (this.#state !== "running" && this.#state !== "ended") this.#require("running", "finish");
    const code = this.#call(() => this.#exports.platen_job_finish(this.#handle));
    if (code < 0) this.#fail("finish");
    this.#state = "finished";
    this.#stdout.push(this.#drain("platen_job_read_replies"));
    this.#stderr.push(this.#drain("platen_job_read_errors"));

    const e = this.#exports;
    const lenAt = this.#scratch;
    const pdfAt = this.#call(() => e.platen_job_pdf(this.#handle, lenAt));
    const pdfLen = new DataView(this.#memory.buffer).getUint32(lenAt, true);
    const pdf = this.#view().slice(pdfAt, pdfAt + pdfLen);
    const outcome = OUTCOMES[code] ?? "error";
    const error =
      outcome === "error"
        ? {
            name: this.#cString(e.platen_job_error_name(this.#handle)),
            offending: this.#cString(e.platen_job_offending(this.#handle)),
          }
        : null;
    return {
      outcome,
      error,
      pdf,
      pages: e.platen_job_pages(this.#handle),
      stdout: decode(concat(this.#stdout)),
      stderr: decode(concat(this.#stderr)),
    };
  }

  /** Releases the job; its instance is dropped with it. */
  free() {
    if (this.#handle && this.#state !== "broken") {
      this.#exports.platen_job_free(this.#handle);
    }
    this.#handle = 0;
    this.#state = "freed";
  }

  // --- internals -------------------------------------------------------------------

  #create(options) {
    const {
      budget = DEFAULT_BUDGET,
      identity = {},
      prelude,
      compress = true,
      embedAllFonts = false,
      serverPassword = 0,
    } = options;
    if (budget !== null && !(Number.isSafeInteger(budget) && budget > 0)) {
      throw new RangeError("efterscript: budget must be a positive integer, or null for unlimited");
    }
    const entries = Object.entries(identity).map(([k, v]) => [
      utf8.encode(k + "\0"),
      utf8.encode(String(v) + "\0"),
    ]);
    const preludeBytes = prelude === undefined ? new Uint8Array(0) : toBytes(prelude, "the prelude");
    const textSize = entries.reduce((n, [k, v]) => n + k.length + v.length, 0);
    this.#reserve(
      Math.max(WINDOW, CONFIG.size + entries.length * ENTRY_SIZE + textSize + preludeBytes.length),
    );

    const base = this.#scratch;
    const entriesAt = base + CONFIG.size;
    let textAt = entriesAt + entries.length * ENTRY_SIZE;
    const view = this.#view();
    const data = new DataView(this.#memory.buffer);
    view.fill(0, base, base + CONFIG.size);
    entries.forEach(([k, v], i) => {
      view.set(k, textAt);
      data.setUint32(entriesAt + i * ENTRY_SIZE, textAt, true);
      textAt += k.length;
      view.set(v, textAt);
      data.setUint32(entriesAt + i * ENTRY_SIZE + 4, textAt, true);
      textAt += v.length;
    });
    view.set(preludeBytes, textAt);

    data.setUint32(base + CONFIG.abiVersion, ABI_VERSION, true);
    data.setUint32(base + CONFIG.identity, entries.length ? entriesAt : 0, true);
    data.setUint32(base + CONFIG.identityLen, entries.length, true);
    data.setUint32(base + CONFIG.prelude, preludeBytes.length ? textAt : 0, true);
    data.setUint32(base + CONFIG.preludeLen, preludeBytes.length, true);
    data.setInt32(base + CONFIG.serverPassword, serverPassword, true);
    data.setInt32(base + CONFIG.compress, compress ? 1 : 0, true);
    data.setInt32(base + CONFIG.embedAllFonts, embedAllFonts ? 1 : 0, true);
    data.setBigUint64(base + CONFIG.stepBudget, BigInt(budget ?? 0), true);

    const handle = this.#call(() => this.#exports.platen_job_new(base));
    if (handle === 0) {
      this.#state = "broken";
      throw new Error(`efterscript: the job could not start: ${this.#lastError()}`);
    }
    this.#handle = handle;
  }

  /** Grows memory by enough pages for `size` bytes the wrapper owns. */
  #reserve(size) {
    const pages = Math.ceil(size / PAGE);
    this.#scratch = this.#memory.grow(pages) * PAGE;
  }

  // Memory may grow during any call, which replaces its buffer, so a
  // view is taken fresh each time it is needed.
  #view() {
    return new Uint8Array(this.#memory.buffer);
  }

  #drain(name) {
    const chunks = [];
    for (;;) {
      const n = this.#call(() => this.#exports[name](this.#handle, this.#scratch, WINDOW));
      if (n === 0) break;
      chunks.push(this.#view().slice(this.#scratch, this.#scratch + n));
    }
    return concat(chunks);
  }

  #cString(at) {
    if (!at) return "";
    const view = this.#view();
    let end = at;
    while (view[end] !== 0) end++;
    return decode(view.subarray(at, end));
  }

  #lastError() {
    return this.#cString(this.#exports.platen_last_error());
  }

  #require(state, what) {
    if (this.#state === state) return;
    const why = {
      ended: "the program has ended",
      finished: "the job is finished",
      freed: "the job has been freed",
      broken: "the engine stopped during an earlier call",
    }[this.#state];
    throw new Error(`efterscript: cannot ${what}: ${why}`);
  }

  #fail(what) {
    throw new Error(`efterscript: ${what} failed: ${this.#lastError()}`);
  }

  // A trap inside the engine (a panic, or running out of memory) leaves
  // the instance unusable; the job is marked broken and later calls
  // are refused.
  #call(f) {
    try {
      return f();
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) {
        this.#state = "broken";
        throw new Error(`efterscript: the engine stopped: ${e.message}`, { cause: e });
      }
      throw e;
    }
  }
}
