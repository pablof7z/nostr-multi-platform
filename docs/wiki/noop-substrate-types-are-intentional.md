---
title: Noop*/Empty* Substrate Types Are Intentional Architectural Defaults
slug: noop-substrate-types-are-intentional
summary: Noop and Empty substrate types serve as intentional defaults for test isolation and bootstrap composition — never remove them without auditing all instantiation sites.
tags:
  - architecture
  - substrate
  - testing
  - discovery
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:322d163a-59eb-4c02-8604-009b4ae4d9b0
---

# Noop*/Empty* Substrate Types Are Intentional Architectural Defaults

> In this codebase, types prefixed with `Noop` or `Empty` (e.g., `NoopRelayAuthorScoreStore`, `EmptyOutboxRouter`, `EmptyMailboxCache`, `NoopRecipientRelayLookup`) are not dead stubs or forgotten scaffolding. They are deliberate architectural defaults used for test isolation and bootstrap composition — situations where a real implementation is not needed or not yet wired.

## Details

- **Test isolation**: Unit and integration tests frequently compose a substrate using `Noop`/`Empty` types to avoid pulling in real I/O, network, or storage dependencies.
- **Bootstrap composition**: Early startup or minimal configurations may use these types as safe no-op placeholders before a full implementation is injected.
- **Discovery pitfall**: These types may appear to have no callers when searching only production code paths. Always search test modules, example binaries, and integration harnesses before concluding a type is unused.
- **Removal gate**: Before deleting any `Noop*` or `Empty*` type, confirm:
  1. No production composition path instantiates it (including feature-flagged paths).
  2. No test or benchmark instantiates it directly or via a builder/factory.
  3. No downstream crate depends on it through a re-export.
- **Naming convention is load-bearing**: The `Noop`/`Empty` prefix is a signal to future readers that the type is intentionally inert. Renaming without preserving this signal obscures intent.

## See Also
- [[actor-thread-blocking-highest-severity|Synchronous Blocking on the Kernel Actor Thread Is a Correctness Showstopper]] — related guide
