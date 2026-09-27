// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

// The engine runs here, off the page's thread, so the page stays
// responsive and a job can be stopped by terminating the worker. The
// module is fetched with progress reports and compiled once per worker.

import { load } from "./efterscript/efterscript.js";

let engine = null;

async function fetchWithProgress(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`the engine could not be downloaded (${response.status})`);
  const total = Number(response.headers.get("content-length")) || 0;
  if (!response.body || !total) return new Uint8Array(await response.arrayBuffer());
  const reader = response.body.getReader();
  const chunks = [];
  let received = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    postMessage({ type: "progress", received, total });
  }
  const bytes = new Uint8Array(received);
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.length;
  }
  return bytes;
}

async function ready() {
  if (!engine) {
    const bytes = await fetchWithProgress(new URL("./efterscript/efterscript.wasm", import.meta.url));
    engine = await load(bytes);
    postMessage({ type: "ready" });
  }
  return engine;
}

onmessage = async ({ data }) => {
  try {
    if (data.type === "warm") {
      await ready();
      return;
    }
    if (data.type === "convert") {
      const e = await ready();
      const started = performance.now();
      const result = await e.convert(data.program, data.options);
      const ms = performance.now() - started;
      postMessage({ type: "result", id: data.id, result, ms }, [result.pdf.buffer]);
    }
  } catch (err) {
    postMessage({ type: "failed", id: data.id, message: String(err?.message ?? err) });
  }
};
