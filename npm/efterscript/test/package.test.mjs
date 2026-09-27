// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

// The assembled package, as a consumer sees it. Run after
// `cargo xtask npm-package`:
//
//     node --test npm/efterscript/test/package.test.mjs
//
// EFTERSCRIPT_PACKAGE names another assembled package directory.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

const dir = process.env.EFTERSCRIPT_PACKAGE
  ? pathToFileURL(process.env.EFTERSCRIPT_PACKAGE.replace(/\/?$/, "/"))
  : new URL("../../../target/npm/efterscript/", import.meta.url);
const { load, DEFAULT_BUDGET } = await import(new URL("efterscript.js", dir));
const engine = await load();
const latin1 = (bytes) => new TextDecoder("latin1").decode(bytes);

test("a page becomes a PDF", async () => {
  const r = await engine.convert("%!PS\n0 0 moveto 100 100 lineto stroke showpage\n");
  assert.equal(r.outcome, "ok");
  assert.equal(r.error, null);
  assert.equal(r.pages, 1);
  assert.ok(latin1(r.pdf.subarray(0, 8)).startsWith("%PDF-"));
  assert.ok(latin1(r.pdf).trimEnd().endsWith("%%EOF"));
});

test("text stays text, and embedding follows the option", async () => {
  const program = "/Helvetica findfont 24 scalefont setfont 72 700 moveto (Hello) show showpage";
  const plain = latin1((await engine.convert(program, { compress: false })).pdf);
  assert.match(plain, /\/BaseFont \/Helvetica /);
  assert.match(plain, /\/ToUnicode/);
  assert.match(plain, /\(Hello\) Tj|\[\(Hello\)/);
  const embedded = latin1((await engine.convert(program, { compress: false, embedAllFonts: true })).pdf);
  assert.match(embedded, /\/BaseFont \/[A-Z]{6}\+/);
});

test("output and an uncaught error are reported", async () => {
  const r = await engine.convert("(before) = 1 0 div");
  assert.equal(r.outcome, "error");
  assert.deepEqual(r.error, { name: "undefinedresult", offending: "div" });
  assert.equal(r.stdout, "before\n");
  assert.match(r.stderr, /%%\[ Error: undefinedresult; OffendingCommand: div \]%%/);
  assert.ok(r.pdf.length > 0, "the document is complete whatever the outcome");
});

test("the budget stops a runaway program", async () => {
  const r = await engine.convert("{ } loop", { budget: 10_000 });
  assert.equal(r.outcome, "budget");
  assert.equal(DEFAULT_BUDGET, 100_000_000);
});

test("identity and prelude reach the job", async () => {
  const r = await engine.convert("product = greet", {
    identity: { product: "(Fictional Press)" },
    prelude: "/greet { (hello) = } def",
  });
  assert.equal(r.outcome, "ok");
  assert.equal(r.stdout, "Fictional Press\nhello\n");
});

test("a malformed identity value refuses the job", async () => {
  await assert.rejects(engine.job({ identity: { product: "(unclosed" } }), /identity entry product/);
});

test("a job fed in pieces answers as it goes", async () => {
  const job = await engine.job();
  assert.deepEqual(job.feed("(one) = (tw"), { stdout: "one\n", stderr: "", done: false });
  assert.deepEqual(job.feed("o) =\n"), { stdout: "two\n", stderr: "", done: false });
  const r = job.finish();
  job.free();
  assert.equal(r.stdout, "one\ntwo\n");
  assert.throws(() => job.feed("x"), /freed/);
});

test("a program larger than the feed window", async () => {
  const body = "1 1 add pop\n".repeat(20_000);
  const r = await engine.convert(body + "(end) =");
  assert.equal(r.outcome, "ok");
  assert.equal(r.stdout, "end\n");
});

test("the engine loads from its bytes and from a compiled module", async () => {
  const bytes = await readFile(new URL("efterscript.wasm", dir));
  const fromBytes = await load(bytes);
  const fromModule = await load(fromBytes.module);
  assert.equal((await fromModule.convert("(ok) =")).stdout, "ok\n");
});

test("the manifest carries a real version", async () => {
  const manifest = JSON.parse(await readFile(new URL("package.json", dir), "utf8"));
  assert.notEqual(manifest.version, "0.0.0");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
});
