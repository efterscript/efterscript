<!-- SPDX-FileCopyrightText: 2026 EfterScript contributors -->
<!-- SPDX-License-Identifier: MIT -->

# efterscript

A memory-safe interpreter compatible with the PostScript language,
compiled to WebAssembly: a program in, a PDF out, in the browser or in
Node.js. Vectors stay vectors, text stays text, colour spaces are
preserved; nothing is rasterised.

Try it in your browser: <https://efterscript.github.io/efterscript/>

The engine is the same Rust code that is published as the
[`efterscript`](https://crates.io/crates/efterscript) crates, built for
`wasm32-unknown-unknown` with no imports: it cannot reach the network,
the file system, or anything else in its host. The package has no
dependencies.

## Use

```js
import { load } from "efterscript";

const engine = await load();
const result = await engine.convert(`%!PS
/Helvetica findfont 24 scalefont setfont
72 700 moveto (Hello, PDF) show
showpage`);

result.outcome; // "ok", "error", or "budget"
result.pdf;     // Uint8Array holding the document
result.stdout;  // what the program printed
```

`load()` fetches `efterscript.wasm` from beside the module (from the file
system under Node.js). Pass a URL, a `Response`, the bytes, or a
compiled `WebAssembly.Module` to load it from somewhere else; a bundler
can resolve `efterscript/efterscript.wasm` to a URL for you.

Programs arriving in pieces can be fed as they come, which is how a host
acting as a printer answers queries while a job is still arriving:

```js
const job = await engine.job({ identity: { product: "(Fictional Press)" } });
job.feed(firstChunk);     // { stdout, stderr, done }
job.feed(secondChunk);
const result = job.finish();
job.free();
```

Each job runs in a fresh instance of the module, so nothing survives
from one job to the next.

### Options

| Option | Default | Meaning |
|---|---|---|
| `budget` | `100000000` | Objects the program may execute before it stops with the `budget` outcome; `null` for no limit. |
| `identity` | `{}` | `statusdict` entries, each value as literal text in the PostScript language. |
| `prelude` | none | A program run once at the server level before the job. |
| `compress` | `true` | Compress page content. |
| `embedAllFonts` | `false` | Embed every font, the resident ones included. |
| `serverPassword` | `0` | The password `exitserver` expects. |

The engine runs synchronously inside `feed` and `finish`. In a browser,
run it in a Web Worker so the page stays responsive, and terminate the
worker to cancel a job; `engine.module` can be posted to the worker and
passed to `load` there.

## Licence

MIT for the project's own code. The module contains the resident font
set and its data, which carry their own terms; see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). PDFs produced with
these fonts carry no obligation.

PostScript is a registered trademark of Adobe. EfterScript is an
independent interpreter compatible with the PostScript language and is
not affiliated with or endorsed by Adobe.
