// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Virtual-printer session front-end.
//!
//! An interactive PostScript session server: session mode with per-job
//! encapsulation over a persistent parent VM, `exitserver`/`startjob`
//! semantics, a configurable printer-identity layer (product strings,
//! `languagelevel`, `statusdict`, `pagecount`, tray stubs), resident-font
//! queries answered with correct PostScript names, and the
//! `%%[ Error: ...; OffendingCommand: ... ]%%` back channel. "Paper" is PDF.
//!
//! Transport-agnostic: job bytes in, back-channel bytes out, PDF out. AppleTalk
//! /PAP/LocalTalk framing lives emulator-side (Granny Smith) or in host glue —
//! never here. Native transports (LPD, raw TCP 9100, `papd`, pty) wrap this
//! same API.
//!
//! "LaserWriter" is an Apple trademark: an identity *setting*, never a product
//! name.
