---
title: ADR Directory Governance
slug: adr-governance
topic: adr-governance
summary: ADR-0073 keeps the ADR directory current-only; obsolete decisions move surviving rules to current owners and are deleted
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
---

# ADR Directory Governance

## ADR Directory Governance

ADR-0073 keeps `docs/decisions/` as a current-only decision surface. The
directory is not an archive: when a decision stops describing current
architecture, any surviving rule moves to its current owner and the obsolete ADR
file is deleted. Git history, closed issues, and pull request bodies preserve
earlier context.

The current decision spine is ADR-0069 through ADR-0073. ADR-0074 through
ADR-0076 are current extensions that remain only while they own live invariants
outside the spine or a durable architecture/API document.

ADR cleanup has failed if a new contributor reads the directory and comes away
thinking production `register_defaults()`, app-facing `open_interest`, public
`ReducedSource`, or hand-wired `ObservedProjectionSink` is the intended future.

A PR touching an architectural invariant updates the current owner in place. It
does not add a parallel correction document, preserve obsolete ADR text for
context, or point readers at a deleted decision as current authority.

<!-- citations: [^3c942-be52a] [^019f0-1cf49] [^3c942-7f62b] [^898a4-fffe4] -->
