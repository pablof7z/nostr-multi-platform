---
title: Embed Kind Projection
slug: embed-kind-projection
topic: content-rendering
summary: Content rendering and embedded event content rendering must use the same rendering engine, with embedded events rendering as an inline widget that dispatches th
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-18
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
  - session:019edc00-f3a6-77f3-b21a-d6b45f5f6cab
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Embed Kind Projection

## Embed Kind Projection

Content rendering and embedded event content rendering must use the same rendering engine, with embedded events rendering as an inline widget that dispatches through the same per-kind rendering pipeline. A native `[kind → widget]` map is the wrong primitive; the correct primitive is `[EmbedKindProjection variant → widget]`, where Rust decides which variant (what shape of data) and native decides which widget renders it. Each platform's kind registry dispatches on `EmbedProjectionVariantTag` (not raw kind number) to select which widget renders an embedded event. <!-- [^15725-1] -->

The kind-dispatch system must be easily extensible to hundreds or thousands of kind renderers (classified events, zap receipts, mute lists, NIP-29 group metadata, etc.). <!-- [^15725-2] -->

Per-kind sub-widgets use composable compound components (e.g. `Article.Root { Article.Title(); Article.Summary(); Article.Image() }`) rather than monolithic card layouts. <!-- [^15725-3] -->

Adding a new kind handler involves a coordinated Rust+native PR that adds a new `EmbedKindProjection` variant alongside the native widget. <!-- [^15725-4] -->

The default handler set for each platform registry at launch covers only kind:1 (ShortNote) and Unknown, with kind:30023/9802/0 added in follow-up work items. <!-- [^15725-5] -->

## ADR Requirement

An ADR must be written before any code lands, locking in three commitments: (1) `EmbedKindProjection` is a Rust-side typed enum, not a native kind-to-widget map; (2) `ContentTreeWire` is the canonical embed-host wire format across all platforms; (3) `nmp-content::RenderContext` is the single recursion guard. ADRs live in `docs/decisions/0032-*.md` format and plan files in `docs/plan/m*.md` format, with F-08 already scoping this area in BACKLOG. The ADR-0055 projection merge cache must live in the wasm worker/Rust side; the web client.ts must only decode and render already-merged frames, not reimplement keep-last-good projection merge semantics in TypeScript.

<!-- citations: [^15725-6] [^019ed-14] -->
## EmbedKindProjection Variants

The `EmbedKindProjection` enum has five variants: `ShortNote`, `Article`, `Highlight`, `Profile`, and `Unknown`, each carrying a typed payload with the data the widget needs to render that variant. The variants carry raw protocol data (hex pubkeys, u64 timestamps, verbatim strings) — native widgets compute display strings like initials, colors, abbreviated npubs, and relative times themselves. Presentation formatting (SF Symbol names, English prose labels, pluralization, bech32 encoding, emoji) must not appear in projection builders, snapshot types, or FFI serialization paths — aim.md §2 explicitly forbids it. In-code comments citing 'doctrine §6' or 'V-24' to justify presentation formatting in Rust projections are bogus; aim.md §2 is the actual rule that forbids formatting in projections, and those stale violations are being removed.

<!-- citations: [^15725-7] [^15725-8] [^11850-54] [^11850-95] -->
## NIP Crates

New NIP crates `nmp-nip23` (kind:30023 articles) and `nmp-nip84` (kind:9802 highlights) must be created to hold their respective projection types and decoders. Kind:9802 highlights are green-field with no existing `nmp-nip84` crate. <!-- [^15725-9] -->

## Wire Format and Recursion Guard

Android's `ContentTreeDto`/`SegmentDto` wire format must be migrated onto `ContentTreeWire` so that a single wire format is used across all three platforms. The `nmp-content::RenderContext` depth/cycle guard is the single recursion guard consumed by all platforms via a `RenderContextWire { depth, max_depth, visited: Vec<String> }` field on the embed envelope. <!-- [^15725-10] -->

## Platform Embed Rendering

The `EmbeddedEvent` view/composable/widget on each platform fetches the `EmbeddedEventEnvelope`, looks up the handler for the projection variant via the registry, wraps the result in `EmbedChromeContainer`, and threads `context.descend(uri)` into the child for recursion/cycle detection. The TUI `NostrKindRegistry` uses `Arc<dyn KindRenderer>` so the registry can be cloned cheaply into rendering closures. <!-- [^15725-11] -->

The `EmbedChromeContainer` is a per-platform chrome component (border, indented padding, collapse affordance, cycle-broken placeholder) that knows nothing about the wrapped content. The collapse-reason copy strings (e.g. 'Already shown (cycle broken)', 'Nested content collapsed') live in Rust (`nmp-core::display::embed_collapse_copy`), and each platform binds them from the envelope directly. <!-- [^15725-12] -->

iOS's existing closure-based `quoteCardProvider` API must be preserved as `@available(*, deprecated)` for one release so Chirp call sites continue compiling during migration. <!-- [^15725-13] -->

The legacy `NostrQuoteCard` (iOS), `EmbedCard` (Android), and `NostrQuoteCard` (TUI) are deleted in F-CR-04 after all three platform registries ship with default handlers bound. <!-- [^15725-14] -->

## NDK Svelte Reference Architecture

The NDK Svelte `ContentRenderer` provides a kind-to-component dispatch registry, and its `EmbeddedEvent` component resolves an event reference through that same registry rather than using a bespoke embed renderer. The registry separates content parsing from content rendering into two independent layers. The `ContentRenderer` registers kind handlers with `addKind`, which accepts either an NDK wrapper class (auto-extracting kind numbers) or an explicit array of kind numbers. The `ContentRenderer` supports priority-based handler resolution where higher-priority handlers override lower ones, and propagates through Svelte contexts, allowing nested components to inherit and override the renderer. <!-- [^15725-15] -->

## Current Platform Behavior

The `NostrQuoteCard` widget on iOS renders all event kinds identically with no kind-specific differentiation. Android's `EmbedCard` is kind-aware, dispatching to `ArticlePreview` for kind:30023, `ListCard` for NIP-51 lists, and a generic event card otherwise. The TUI `NostrQuoteCard` surfaces the kind number in its header text but does not visually differentiate the body rendering by kind. <!-- [^15725-16] -->

## Regression Fixtures

Regression fixtures must include nested-embed scenarios exercising depth chains (kind:1 quoting kind:30023 quoting kind:9802 quoting kind:1), cycle detection (kind:1 quoting itself transitively), and depth-limit enforcement at max_depth=4. <!-- [^15725-17] -->
