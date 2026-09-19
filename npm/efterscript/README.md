<!-- SPDX-FileCopyrightText: 2026 EfterScript contributors -->
<!-- SPDX-License-Identifier: MIT -->

# efterscript

A memory-safe, embeddable interpreter compatible with the PostScript
language, written in Rust, whose primary output is PDF: vectors stay
vectors, text stays text, colour spaces are preserved, nothing is
rasterised.

**This npm package is a placeholder.** It reserves the name for the
forthcoming WebAssembly build and exports nothing usable yet; calling
its one function throws an explanatory error. The engine itself is
published today as Rust crates, starting from
[`efterscript`](https://crates.io/crates/efterscript), and its source,
documentation, and roadmap live at
<https://github.com/efterscript/efterscript>.

When the WebAssembly build ships, this package will export the
distillation entry point (a program in, PDF bytes out) and the
streaming session interface, for browsers and Node.js alike.

PostScript is a registered trademark of Adobe. EfterScript is an
independent interpreter compatible with the PostScript language and is
not affiliated with or endorsed by Adobe.

MIT licence.
