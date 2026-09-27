// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

// The try-it page: an editor for the program, a worker that runs the
// engine, and a sheet that shows the PDF with the browser's own viewer.

const $ = (id) => document.getElementById(id);

const ui = {
  source: $("source"),
  highlight: $("highlight"),
  editor: $("editor"),
  fileInput: $("file"),
  fileCard: $("file-card"),
  fileName: $("file-name"),
  fileMeta: $("file-meta"),
  fileClear: $("file-clear"),
  convert: $("convert"),
  shortcut: $("shortcut"),
  budget: $("budget"),
  compress: $("compress"),
  embed: $("embed"),
  status: $("status"),
  viewer: $("viewer"),
  note: $("sheet-note"),
  noteTitle: $("sheet-note-title"),
  noteBody: $("sheet-note-body"),
  meter: $("meter"),
  meterFill: $("meter-fill"),
  outcome: $("outcome"),
  pages: $("pages"),
  size: $("size"),
  time: $("time"),
  fonts: $("fonts"),
  download: $("download"),
  open: $("open"),
  stop: $("stop"),
  log: $("log"),
  dropzone: $("dropzone"),
  version: $("version"),
};

// Above this size a file is converted as it is, without the editor.
const EDITABLE_LIMIT = 256 * 1024;
// Above this size the editor stops colouring the program.
const HIGHLIGHT_LIMIT = 64 * 1024;

const state = {
  sample: "specimen",
  fileBytes: null, // a file too large to edit
  baseName: "specimen",
  pdfUrl: null,
  jobId: 0,
  running: null, // { id, started, timer }
  engineReady: false,
};

if (/Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)) {
  ui.shortcut.textContent = "⌘ ↵";
}

// --- syntax colouring -----------------------------------------------------------

const DELIMS = new Set(["(", ")", "<", ">", "[", "]", "{", "}", "/", "%"]);
const isSpace = (c) => c === " " || c === "\n" || c === "\t" || c === "\r" || c === "\f" || c === "\0";
const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$|^\d+#[0-9A-Za-z]+$/;

function escapeHtml(s) {
  return s.replace(/[&<>]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"));
}

function span(kind, text) {
  return `<span class="tok-${kind}">${escapeHtml(text)}</span>`;
}

// A scanner just good enough to colour the program: comments, strings
// (balanced parentheses and escapes), hex strings, literal names,
// numbers, and the procedure and array brackets.
function colour(text) {
  let out = "";
  let i = 0;
  const n = text.length;
  while (i < n) {
    const c = text[i];
    if (c === "%") {
      let j = text.indexOf("\n", i);
      if (j < 0) j = n;
      const line = text.slice(i, j);
      out += span(/^%%|^%!/.test(line) ? "dsc" : "comment", line);
      i = j;
    } else if (c === "(") {
      let depth = 0;
      let j = i;
      for (; j < n; j++) {
        const d = text[j];
        if (d === "\\") j++;
        else if (d === "(") depth++;
        else if (d === ")" && --depth === 0) break;
      }
      out += span("string", text.slice(i, j + 1));
      i = j + 1;
    } else if (c === "<" && text[i + 1] !== "<") {
      let j = text.indexOf(">", i);
      if (j < 0) j = n - 1;
      out += span("string", text.slice(i, j + 1));
      i = j + 1;
    } else if (c === "{" || c === "}" || c === "[" || c === "]") {
      out += span("brace", c);
      i++;
    } else if ((c === "<" && text[i + 1] === "<") || (c === ">" && text[i + 1] === ">")) {
      out += span("brace", c + c);
      i += 2;
    } else if (isSpace(c)) {
      let j = i;
      while (j < n && isSpace(text[j])) j++;
      out += text.slice(i, j);
      i = j;
    } else {
      let j = i + 1;
      while (j < n && !isSpace(text[j]) && !DELIMS.has(text[j])) j++;
      const word = text.slice(i, j);
      if (c === "/") out += span("name", word);
      else if (NUMBER.test(word)) out += span("number", word);
      else out += escapeHtml(word);
      i = j;
    }
  }
  // A trailing newline needs a line to stand on, or the last line of
  // the textarea has no counterpart.
  return out + "\n";
}

function paint() {
  const text = ui.source.value;
  if (text.length > HIGHLIGHT_LIMIT) {
    ui.editor.classList.add("plain");
    return;
  }
  ui.editor.classList.remove("plain");
  ui.highlight.innerHTML = colour(text);
  syncScroll();
}

function syncScroll() {
  const pre = ui.highlight.parentElement;
  pre.scrollTop = ui.source.scrollTop;
  pre.scrollLeft = ui.source.scrollLeft;
}

ui.source.addEventListener("input", () => {
  state.sample = null;
  markSample(null);
  paint();
});
ui.source.addEventListener("scroll", syncScroll);
ui.source.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
    e.preventDefault();
    convert();
  } else if (e.key === "Tab" && !e.shiftKey && !e.altKey) {
    // Tab indents; Escape then Tab leaves the editor.
    e.preventDefault();
    document.execCommand("insertText", false, "    ");
  }
});

// --- samples and files ----------------------------------------------------------

function markSample(name) {
  for (const b of document.querySelectorAll("[data-sample]")) {
    b.setAttribute("aria-pressed", String(b.dataset.sample === name));
  }
}

async function loadSample(name, run = true) {
  const response = await fetch(`samples/${name}.ps`);
  if (!response.ok) throw new Error(`the sample ${name} could not be loaded`);
  showEditor(await response.text());
  state.sample = name;
  state.baseName = name;
  markSample(name);
  if (run) convert();
}

for (const b of document.querySelectorAll("[data-sample]")) {
  b.addEventListener("click", () => loadSample(b.dataset.sample).catch(fail));
}

function showEditor(text) {
  state.fileBytes = null;
  ui.fileCard.hidden = true;
  ui.editor.hidden = false;
  ui.source.value = text;
  ui.source.scrollTop = 0;
  paint();
}

// A DOS EPS file wraps the program in a binary header with a preview
// image; the program is the section the header points at.
function unwrapEps(bytes) {
  if (bytes.length > 30 && bytes[0] === 0xc5 && bytes[1] === 0xd0 && bytes[2] === 0xd3 && bytes[3] === 0xc6) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const start = view.getUint32(4, true);
    const length = view.getUint32(8, true);
    if (start + length <= bytes.length) return bytes.subarray(start, start + length);
  }
  return bytes;
}

function looksTextual(bytes) {
  const sample = bytes.subarray(0, 4096);
  let control = 0;
  for (const b of sample) if (b < 9 || (b > 13 && b < 32)) control++;
  return control < sample.length * 0.02;
}

async function openFile(file) {
  if (!file) return;
  const bytes = unwrapEps(new Uint8Array(await file.arrayBuffer()));
  state.baseName = file.name.replace(/\.[^.]+$/, "") || "document";
  state.sample = null;
  markSample(null);
  if (bytes.length <= EDITABLE_LIMIT && looksTextual(bytes)) {
    showEditor(new TextDecoder("latin1").decode(bytes));
  } else {
    state.fileBytes = bytes;
    ui.editor.hidden = true;
    ui.fileCard.hidden = false;
    ui.fileName.textContent = file.name;
    ui.fileMeta.textContent = `${formatBytes(bytes.length)} · converted as it is, too large or too binary to edit here`;
  }
  convert();
}

ui.fileInput.addEventListener("change", () => {
  openFile(ui.fileInput.files[0]).catch(fail);
  ui.fileInput.value = "";
});
ui.fileClear.addEventListener("click", () => loadSample("specimen", false).catch(fail));

let dragDepth = 0;
addEventListener("dragenter", (e) => {
  if (![...(e.dataTransfer?.types ?? [])].includes("Files")) return;
  e.preventDefault();
  dragDepth++;
  ui.dropzone.hidden = false;
});
addEventListener("dragover", (e) => {
  if ([...(e.dataTransfer?.types ?? [])].includes("Files")) e.preventDefault();
});
addEventListener("dragleave", () => {
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) ui.dropzone.hidden = true;
});
addEventListener("drop", (e) => {
  e.preventDefault();
  dragDepth = 0;
  ui.dropzone.hidden = true;
  const file = e.dataTransfer?.files?.[0];
  if (file) openFile(file).catch(fail);
});

// --- the worker -----------------------------------------------------------------

let worker = null;

function startWorker() {
  worker = new Worker(new URL("worker.js", import.meta.url), { type: "module" });
  worker.onmessage = ({ data }) => {
    if (data.type === "progress") onProgress(data.received, data.total);
    else if (data.type === "ready") onReady();
    else if (data.type === "result") onResult(data);
    else if (data.type === "failed") onFailed(data);
  };
  worker.onerror = (e) => {
    e.preventDefault();
    onFailed({ id: state.running?.id, message: e.message || "the engine could not start" });
  };
  worker.postMessage({ type: "warm" });
}

function onProgress(received, total) {
  ui.meter.classList.remove("indeterminate");
  ui.meterFill.style.width = `${Math.round((received / total) * 100)}%`;
  ui.status.textContent = `Loading the engine · ${formatBytes(received)} of ${formatBytes(total)}`;
}

function onReady() {
  state.engineReady = true;
  if (!state.running) ui.status.textContent = "Engine ready · runs on this computer";
}

// --- converting -----------------------------------------------------------------

function options() {
  const budget = ui.budget.value === "unlimited" ? null : Number(ui.budget.value);
  return { budget, compress: ui.compress.checked, embedAllFonts: ui.embed.checked };
}

function convert() {
  if (state.running) return;
  const program = state.fileBytes ?? ui.source.value;
  const id = ++state.jobId;
  const started = performance.now();
  state.running = {
    id,
    started,
    timer: setInterval(() => {
      const s = (performance.now() - started) / 1000;
      ui.status.textContent = state.engineReady ? `Running · ${s.toFixed(1)} s` : ui.status.textContent;
      if (s > 0.8) ui.stop.hidden = false;
    }, 100),
  };
  ui.convert.disabled = true;
  setOutcome("running", "Running");
  if (state.engineReady) ui.status.textContent = "Running";
  worker.postMessage({ type: "convert", id, program, options: options() });
}

function finishRun() {
  clearInterval(state.running?.timer);
  state.running = null;
  ui.convert.disabled = false;
  ui.stop.hidden = true;
}

function onResult({ id, result, ms }) {
  if (id !== state.running?.id) return;
  finishRun();
  showPdf(result.pdf);
  ui.status.textContent = "Engine ready · runs on this computer";
  const outcome = { ok: "Finished", error: "Stopped by an error", budget: "Budget spent" }[result.outcome];
  setOutcome(result.outcome, outcome);
  ui.pages.textContent = String(result.pages);
  ui.size.textContent = formatBytes(result.pdf.length);
  ui.time.textContent = ms < 1000 ? `${Math.max(1, Math.round(ms))} ms` : `${(ms / 1000).toFixed(1)} s`;
  showFonts(result.pdf);
  showLog(result);
}

function onFailed({ id, message }) {
  if (state.running && id !== undefined && id !== state.running.id) return;
  finishRun();
  setOutcome("error", "Could not run");
  ui.status.textContent = "Something went wrong";
  writeLog([{ cls: "report", text: message }]);
  if (!state.engineReady) {
    showNote("The engine did not load", `${message}. Reload the page to try again.`);
  }
}

ui.convert.addEventListener("click", convert);

ui.stop.addEventListener("click", () => {
  if (!state.running) return;
  worker.terminate();
  finishRun();
  state.engineReady = false;
  setOutcome("stopped", "Stopped");
  writeLog([{ cls: "meta", text: "The job was stopped before it finished; nothing was produced." }]);
  ui.status.textContent = "Restarting the engine…";
  startWorker();
});

// --- the proof ------------------------------------------------------------------

function showPdf(bytes) {
  if (state.pdfUrl) URL.revokeObjectURL(state.pdfUrl);
  state.pdfUrl = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  const name = `${state.baseName}.pdf`;
  for (const a of [ui.download, ui.open]) {
    a.href = state.pdfUrl;
    a.removeAttribute("aria-disabled");
  }
  ui.download.download = name;
  if (navigator.pdfViewerEnabled === false) {
    ui.viewer.hidden = true;
    showNote(
      "Your PDF is ready",
      "This browser does not show PDFs inside a page. Open it in a new tab or download it with the buttons below.",
    );
    return;
  }
  ui.note.hidden = true;
  ui.viewer.hidden = false;
  ui.viewer.src = `${state.pdfUrl}#view=Fit&toolbar=0&navpanes=0`;
}

function showNote(title, body) {
  ui.note.hidden = false;
  ui.meter.hidden = true;
  ui.noteTitle.textContent = title;
  ui.noteBody.textContent = body;
}

function setOutcome(stateName, label) {
  ui.outcome.dataset.state = stateName;
  ui.outcome.textContent = label;
}

// The fonts the document uses, read from its font dictionaries: a
// six-letter tag before the name marks an embedded subset.
function showFonts(pdf) {
  const text = new TextDecoder("latin1").decode(pdf);
  const seen = new Map();
  for (const m of text.matchAll(/\/BaseFont\s*\/([^\s/<>[\]()]+)/g)) {
    const raw = m[1];
    const subset = /^[A-Z]{6}\+/.test(raw);
    const name = raw.replace(/^[A-Z]{6}\+/, "").replace(/#20/g, " ");
    if (!seen.has(name) || subset) seen.set(name, subset);
  }
  ui.fonts.replaceChildren();
  if (seen.size === 0) {
    ui.fonts.textContent = "None; this document has no text.";
    return;
  }
  for (const [name, subset] of seen) {
    const el = document.createElement("span");
    el.className = "font";
    el.textContent = name;
    const small = document.createElement("small");
    small.textContent = subset ? "embedded subset" : "standard, not embedded";
    el.append(small);
    ui.fonts.append(el);
  }
}

// --- device output --------------------------------------------------------------

function showLog(result) {
  const lines = [];
  const out = (result.stdout + result.stderr).replace(/\n$/, "");
  if (out) {
    for (const line of out.split("\n")) {
      lines.push({ cls: /^%%\[.*\]%%$/.test(line.trim()) ? "report" : "", text: line });
    }
  } else {
    lines.push({ cls: "empty", text: "The program printed nothing." });
  }
  const end = {
    ok: `job ended normally · ${plural(result.pages, "page")}`,
    error: `job ended with ${result.error?.name ?? "an error"} in ${result.error?.offending || "the program"} · ${plural(result.pages, "page")} kept`,
    budget: `job stopped after spending its execution budget · ${plural(result.pages, "page")} kept`,
  }[result.outcome];
  lines.push({ cls: "meta", text: `\n— ${end}` });
  writeLog(lines);
}

function writeLog(lines) {
  ui.log.replaceChildren(
    ...lines.map(({ cls, text }) => {
      const el = document.createElement("span");
      if (cls) el.className = cls;
      el.textContent = text + "\n";
      return el;
    }),
  );
}

// --- helpers --------------------------------------------------------------------

function formatBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function plural(n, word) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

function fail(err) {
  onFailed({ id: undefined, message: String(err?.message ?? err) });
}

// --- notices --------------------------------------------------------------------

// The notices open over the page instead of replacing it; the link's own
// target stays as the fallback.
const notices = $("notices");
$("notices-link").addEventListener("click", async (e) => {
  if (typeof notices.showModal !== "function") return;
  e.preventDefault();
  const text = $("notices-text");
  if (!text.textContent) {
    try {
      const [engine, fonts] = await Promise.all(
        [e.currentTarget.href, "fonts/NOTICE.md"].map(async (url) => {
          const r = await fetch(url);
          if (!r.ok) throw new Error(r.statusText);
          return (await r.text()).replace(/^<!--.*-->\n/gm, "").trim();
        }),
      );
      text.textContent = `${engine}\n\n\n${fonts}`;
    } catch {
      text.textContent = "The notices could not be loaded.";
    }
  }
  notices.showModal();
});
$("notices-close").addEventListener("click", () => notices.close());
notices.addEventListener("click", (e) => {
  if (e.target === notices) notices.close();
});

// --- start ----------------------------------------------------------------------

fetch("efterscript/package.json")
  .then((r) => (r.ok ? r.json() : null))
  .then((p) => {
    if (p?.version) ui.version.textContent = `${p.version}`;
  })
  .catch(() => {});

ui.meter.classList.add("indeterminate");
startWorker();
loadSample("specimen").catch(fail);
