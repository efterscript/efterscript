// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

/** A program, a prelude, or bytes to feed: text is encoded as UTF-8. */
export type Input = string | ArrayBuffer | ArrayBufferView;

/** Where the WebAssembly module comes from. */
export type EngineSource =
  | string
  | URL
  | Response
  | PromiseLike<Response>
  | ArrayBuffer
  | ArrayBufferView
  | WebAssembly.Module;

/** Objects a job may execute unless its options say otherwise. */
export const DEFAULT_BUDGET: number;

export interface JobOptions {
  /**
   * Objects the program may execute before it is stopped with the
   * `budget` outcome; `null` removes the limit. Default `DEFAULT_BUDGET`.
   */
  budget?: number | null;
  /**
   * `statusdict` entries the device claims, each value as literal text
   * in the PostScript language: `"(Fictional Press)"`, `"47.0"`, `"true"`, `"/name"`.
   */
  identity?: Record<string, string>;
  /** A program run once at the server level before the job; its output is discarded. */
  prelude?: Input;
  /** Compress page content (`CompressPages`). Default `true`. */
  compress?: boolean;
  /** Embed every font, the resident ones included. Default `false`. */
  embedAllFonts?: boolean;
  /** The password `exitserver` expects. Default `0`. */
  serverPassword?: number;
}

export interface Progress {
  /** The program's standard output since the previous call. */
  stdout: string;
  /** Its error reports since the previous call (`%%[ Error: … ]%%` lines). */
  stderr: string;
  /** Whether the program has ended before its data did. */
  done: boolean;
}

export interface Result {
  /** `ok`: ran to the end; `error`: an uncaught error; `budget`: the budget was spent. */
  outcome: "ok" | "error" | "budget";
  /** The error name and the offending command, when the outcome is `error`. */
  error: { name: string; offending: string } | null;
  /** The document, complete whatever the outcome. */
  pdf: Uint8Array;
  /** Pages in the document. */
  pages: number;
  /** Everything the program wrote to standard output. */
  stdout: string;
  /** Everything it wrote to standard error. */
  stderr: string;
}

/** Compiles the engine; by default from `efterscript.wasm` beside this module. */
export function load(source?: EngineSource): Promise<Engine>;

export class Engine {
  private constructor();
  /** The compiled module, for handing to a worker (`load(engine.module)`). */
  readonly module: WebAssembly.Module;
  /** Starts a job in a fresh instance. */
  job(options?: JobOptions): Promise<Job>;
  /** Runs a whole program: `job`, `feed`, `finish`, `free`. */
  convert(program: Input, options?: JobOptions): Promise<Result>;
}

export class Job {
  private constructor();
  /** Whether the program has ended before its data did. */
  readonly done: boolean;
  /** Appends bytes to the program and runs as far as they allow. */
  feed(input: Input): Progress;
  /** Ends the data, runs to completion, and closes the document. */
  finish(): Result;
  /** Releases the job and its instance. */
  free(): void;
}
