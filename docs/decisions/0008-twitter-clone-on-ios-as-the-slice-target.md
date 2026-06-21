# ADR 0008: Initial Chirp social baseline on iOS as the Phase 1a demo target

**Date:** 2026-05-17
**Status:** accepted (positioning modified by ADR-0009)
**Supersedes (in part):** ADR-0006 (in choice of demo target only — discipline preserved)
**Relates to:** ADR-0005 (domain-keyed shadow), ADR-0007 (diagnostics bridge)
**Modified by:** ADR-0009 (the social baseline is built as extension modules over `nmp-core` + protocol modules, not as kernel built-ins) and ADR-0046 (composition is a library call, not a generated per-app crate)

## Decision

The Phase 1a demo target is the first Chirp social baseline on iOS: a real, recognizable Nostr client — open to a lively timeline, tap into threads and profiles, log in to compose, like, and reply — pulling from a single hardcoded relay and persisting across restart. It replaces ADR-0006's deliberately narrow desktop avatar slice while preserving ADR-0006's walking-skeleton discipline (running code at every checkpoint, one architectural ingredient at a time, runtime evidence over modeled budgets). Desktop iced is retained alongside as a non-shipping, UniFFI-free diagnostic reference target.

The unauthenticated timeline is seeded from the union of a small set of hardcoded seed dev accounts' follow lists, giving the demo real breadth from first launch. Logged-in users can switch the timeline source to their own follows.

## 2026-05-20 positioning update

ADR-0008 defines the first social slice, not Chirp's product ceiling. Chirp has
since become NMP's full showcase client: every reusable feature NMP ships should
eventually have a Chirp surface, diagnostics path, smoke path, or documented
platform exception. The timeline/profile/thread/compose workflow remains the
minimum social baseline that proves the kernel can drive a recognizable client.
It is not the final ambition for Chirp.

See [`../plan/chirp-showcase.md`](../plan/chirp-showcase.md).

## Consequences

- The first runtime evidence is a real iOS app demoable to the Nostr community, not a desktop avatar.
- iOS toolchain risk (UniFFI, xcframework, Xcode versioning) is front-loaded; the desktop reference target is the fallback validation path that separates architecture bugs from toolchain bugs.
- ADR-0007 diagnostics are first-class from the first sub-phase, not bolted on later.

## Alternatives considered

- **Keep ADR-0006 as-is (desktop avatar only).** Rejected — the demo doesn't show the framework's actual value proposition.
- **Skip the desktop reference target.** Rejected — loses the UniFFI-vs-architecture debugging shortcut.
- **Start with iOS directly (skip the desktop slice).** Rejected — UniFFI noise in the very first runtime would conflate architecture bugs with toolchain bugs.
- **Build a Mastodon clone or a DM-first messenger instead.** Rejected — Nostr-shaped public-event data and relay behavior are what the slice must prove; the messenger pattern (NIP-17 + gift-wrap + NSE) adds significant Phase 5 work to the critical path.
