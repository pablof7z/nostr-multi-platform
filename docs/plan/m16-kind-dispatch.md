# M16 Kind-Dispatch Content Rendering

> Sub-plan of [F-08 / M16 component registry](m16-component-registry.md).
> Architectural decisions are locked in
> [ADR-0034](../decisions/0034-kind-dispatch-content-rendering.md).
> Backlog source of truth: F-CR-01 through F-CR-12 in
> [BACKLOG.md](../BACKLOG.md#f-cr-01--rust-embedkindprojection--embeddedeventenvelope-prerequisite--rust-only).

## Goal

Replace the bespoke embed-card paths with one recursive rendering model:
`ContentTreeWire` identifies event references, Rust resolves each reference into an
`EmbeddedEventEnvelope`, and platform registries dispatch the envelope to a renderer
for that event kind. Embed chrome stays separate from rendered content.

The architectural line is strict:

- Rust owns kind classification and wire data.
- Native and TUI surfaces render typed projections and execute only local UI behavior.
- Unknown event kinds remain first-class through `UnknownProjection` so new handlers do
  not require changes to core dispatch machinery.

## Current Slice

PR #588 covers the first production slice:

| Item | Status | Notes |
|---|---|---|
| F-CR-01 | Done in PR #588 | Adds `nmp_content::embed_projection`, `EmbedKindProjection`, `EmbeddedEventEnvelope`, and the single Rust `match event.kind` dispatch point. |
| F-CR-06 | Done in PR #588 | Adds the TUI `content-kind-registry` component and wires `NostrContentView` to use it when an envelope map is supplied. |
| ADR-0034 | Done in PR #588 | Renumbered from ADR-0033 after feed viewport claimed ADR-0033 on master. |

F-CR-02, F-CR-05, F-CR-07, F-CR-09, F-CR-10, F-CR-11, F-CR-12, and F-CR-04 remain in
BACKLOG order.

## Reading Order

1. [ADR-0034](../decisions/0034-kind-dispatch-content-rendering.md) for the locked
   architectural contract.
2. [ADR-0032](../decisions/0032-raw-data-projection-doctrine.md) for raw-data projection rules.
3. [Foundation plan](m16-kind-dispatch-foundation.md) for Rust wire shape and Android
   `ContentTreeWire` migration.
4. [Platform registry plan](m16-kind-dispatch-platform-registries.md) for iOS, TUI,
   and Android registry work.
5. [Handler and validation plan](m16-kind-dispatch-handlers.md) for per-kind renderer
   components, nested fixtures, and legacy cleanup.

## Implementation Order

1. **F-CR-01: Rust projection envelope.**
   Define `EmbedKindProjection`, `EmbeddedEventEnvelope`, `RenderContextWire`, and
   `resolve_embed_projection`. This is the only Rust dispatch table for content
   rendering kinds.

2. **F-CR-02: Android gallery wire migration.**
   Move Android gallery rendering off legacy DTOs and onto `ContentTreeWire` so all
   platforms consume the same arena-indexed content tree.

3. **F-CR-05/F-CR-06/F-CR-07: platform registries.**
   Add `NostrKindRegistry`, `EmbeddedEvent`, and `EmbedChromeContainer` for SwiftUI,
   TUI, and Compose. These can proceed in parallel once F-CR-01 is available; Android
   also needs F-CR-02.

4. **F-CR-09/F-CR-10/F-CR-11: kind handlers.**
   Ship independently installable renderers for long-form articles, NIP-84
   highlights, and profile cards.

5. **F-CR-12: nested-embed regression fixtures.**
   Add one-deep, cycle, depth-limit, unknown-kind, and highlight scenarios with
   per-platform golden/snapshot coverage.

6. **F-CR-04: legacy embed deletion.**
   Delete old embed DTOs and quote-card components only after the registry and handler
   suite is green across platforms.

## Extensibility Contract

If a kind needs a custom Rust data shape, add a new `EmbedKindProjection` variant,
populate it in `resolve_embed_projection`, and add golden wire coverage before writing
native renderers.

If `UnknownProjection` already exposes enough data, add only a platform component under
`crates/nmp-cli/registry/{swiftui,compose,tui}/content-kind-<N>/` and register it with
the platform `NostrKindRegistry`. Do not add a new Rust match arm for a kind whose
renderer can read from `tags`, `content`, and `content_tree`.

## Test Commands

Use the narrow tests for the slice being changed, plus the always-on doctrine smoke:

```sh
cargo test -p nmp-content
cargo test -p nmp-cli --test e2e_tui
cargo test -p chirp-tui --lib
cargo test -p nmp-testing --test doctrine_lint_smoke
```

Platform-specific follow-up slices add their own gates:

```sh
# iOS registry and handlers
xcodebuild test -scheme Chirp -testPlan ContentRendering

# Android registry and handlers
./gradlew :gallery:test :gallery:connectedAndroidTest
```

## Compatibility Notes

- iOS keeps `quoteCardProvider` functional for one release when F-CR-05 lands; the new
  provider is `embedEnvelopeProvider`.
- TUI falls back to the existing quote-card path unless both a kind registry and a
  matching `EmbeddedEventEnvelope` map are supplied.
- Android deletes its internal `EmbedCard` path in the same slice that introduces
  `EmbeddedEvent`; no public deprecation period is needed for the gallery app.
