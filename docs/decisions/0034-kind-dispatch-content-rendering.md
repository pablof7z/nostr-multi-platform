# ADR-0034 — Kind-dispatched content rendering with open per-kind widget registry

- **Status:** Accepted (2026-05-26)
- **Relates to:** ADR-0032 (raw-data projection doctrine), ADR-0018 (ContentTreeWire),
  F-08 (component registry + content rendering kits)
- **Scope:** `nmp-content`, `nmp-content-fixtures`, the `nmp-cli` registry components
  (`crates/nmp-cli/registry/{swiftui,compose,tui}/`), and every platform's content
  rendering consumer (iOS Chirp, Android gallery, chirp-tui).

## Context

The current content rendering path on all three platforms has a structural gap: when
a Nostr event's content contains an event reference (`nostr:nevent1…` / `nostr:naddr1…`),
the embedded event is rendered by a **bespoke card widget** that is separate from the
main content rendering engine. This means:

- All embedded events render identically regardless of kind — kind:30023 articles look
  the same as kind:1 short notes inside an embed card.
- Each platform implements its own embed card (`NostrQuoteCard` on iOS and TUI,
  `EmbedCard` on Android), creating three divergent rendering surfaces.
- Adding support for a new event kind (classified ads, zap receipts, group metadata,
  etc.) requires modifying every embed card on every platform independently, with no
  shared contract.

The correct shape — as demonstrated by the NDK Svelte registry — is: **one rendering
engine, recursive by nature, kind-dispatched**. An embedded event feeds back through the
same pipeline as a top-level event. The embed chrome (border, indentation, depth cue) is
separate from the content it wraps. Adding a new kind handler is a single new registry
component, installable via `nmp add component`.

This ADR locks in three architectural commitments that all implementation work in
F-08 / F-CR-* must follow.

## Decision

### Commitment 1 — `EmbedKindProjection` is a Rust-owned typed envelope

Rust decides **what shape of data** each event kind produces for rendering. The native
registry binds **which widget** renders each shape. This is the same principle as
ADR-0032 applied to the embed boundary: Rust owns the data contract, native owns the
presentation.

Concretely:

```rust
/// Typed data envelope emitted by the Rust resolver for one embedded event.
/// The variant tag drives native widget dispatch; the variant payload is the
/// complete typed data the widget renders — it never re-parses the raw event.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "variant", content = "data", rename_all = "camelCase")]
pub enum EmbedKindProjection {
    ShortNote(ShortNoteProjection),   // kind:1
    Article(ArticleProjection),        // kind:30023
    Highlight(HighlightProjection),    // kind:9802
    Profile(ProfileProjection),        // kind:0
    Unknown(UnknownProjection),        // all other kinds, or kinds with no registered Rust handler
}
```

All fields in every variant follow ADR-0032 — raw protocol data only:
- Pubkeys as 64-char lowercase hex strings.
- Timestamps as Unix `u64` integers (seconds).
- Display names verbatim from kind:0 (`display_name → displayName → name`, first
  non-empty). Absent when kind:0 is unknown.
- Picture URLs verbatim from kind:0.
- NO pre-computed initials, color hex, abbreviated npubs, relative-time labels, or
  pluralised counts. Native widgets compute those from the raw fields.

The `Unknown` variant is the extensibility primitive for hundreds of kinds:

```rust
pub struct UnknownProjection {
    pub kind: u32,
    pub author_pubkey: String,                // hex
    pub author_display_name: Option<String>,
    pub author_picture_url: Option<String>,
    pub created_at: u64,
    pub content: String,                      // raw event content string
    pub content_tree: ContentTreeWire,        // parsed content (same engine as kind:1)
    pub tags: Vec<Vec<String>>,               // raw NIP-01 tags for custom extraction
    pub alt_text: Option<String>,             // NIP-31 `alt` tag, if present
}
```

With `tags` available, a native kind handler for classified ads (kind:30402) can extract
`price`, `location`, etc. without a Rust-side projection variant. A new typed variant is
only warranted when the kind's data shape is meaningfully different from `Unknown` AND
the kind is widely used enough to justify the coordinated Rust + native change.

The full `EmbeddedEventEnvelope` that flows across the wire:

```rust
pub struct EmbeddedEventEnvelope {
    pub uri: String,                            // the original nostr: URI string
    pub primary_id: String,                     // event id or naddr coord
    pub render_context: RenderContextWire,      // depth + visited for collapse guard
    pub projection: EmbedKindProjection,
    pub collapsed: bool,
    pub collapse_reason: Option<String>,        // "depth_limit" | "cycle" | "unsupported"
}

pub struct RenderContextWire {
    pub depth: u8,
    pub max_depth: u8,
    pub visited: Vec<String>,                   // event ids already on this render path
}
```

### Commitment 2 — `ContentTreeWire` is the single wire format across all platforms

iOS and TUI already consume `ContentTreeWire`. The Android **app module** now consumes
`ContentTreeWire` (`android/app/src/main/java/nmp/content/ContentTreeWire.kt`) and renders
embeds through the kind registry. `ContentTreeDto` / `SegmentDto` / `MarkdownNodeDto`
**remain** in `crates/nmp-content-fixtures::dto` as a fixtures-only serde mirror used to
build deterministic golden bundles for the standalone gallery showcases — they are NOT on
the production embed-host wire path. (An earlier draft of this ADR said they would be
"deleted"; that is corrected here — they are legitimately retained as a fixtures-only
type. The production Android wire format is `ContentTreeWire`.)

Rationale: kind-dispatch requires one rendering engine; one rendering engine requires one
wire format on the production path. The fixtures DTO is a separate, build-time-only
concern and does not duplicate any kind handler.

### Commitment 3 — `nmp-content::RenderContext` is the single recursion guard

`crates/nmp-content/src/context.rs` already implements `RenderContext { depth, max_depth,
visited }` with `should_collapse` and `descend`. Android gallery already has a faithful
Kotlin port. iOS and TUI must consume the same contract — no platform invents a local
depth guard. The `RenderContextWire` (Commitment 1) is the byte-stable representation
that crosses the wire; `RenderContext` is the in-memory type.

## The open registry pattern — extensibility to hundreds of kinds

The key design enabling "hundreds of kind renderers" without a monolithic registry:

**Each kind handler is a separate, independently installable `nmp-cli` registry
component.** The core dispatch machinery is one component; every kind handler is another.

```sh
# Install the core dispatch machinery (once per project)
nmp add component swiftui/content-kind-registry
nmp add component compose/content-kind-registry
nmp add component tui/content-kind-registry

# Install kind handlers a-la-carte
nmp add component swiftui/content-kind-1        # short notes (kind:1)
nmp add component swiftui/content-kind-30023    # long-form articles
nmp add component swiftui/content-kind-9802     # highlights (NIP-84)
nmp add component swiftui/content-kind-0        # profile cards
nmp add component swiftui/content-kind-30402    # classified ads (NIP-99)
nmp add component swiftui/content-kind-9735     # zap receipts
nmp add component swiftui/content-kind-39000    # NIP-29 group metadata
```

The app's startup code wires them:

```swift
// App startup — register whatever handlers are installed
let registry = NostrKindRegistry.makeDefault()   // registers ShortNote + Unknown built-ins
registry.register(ArticleKindRenderer())          // if content-kind-30023 is installed
registry.register(HighlightKindRenderer())        // if content-kind-9802 is installed
// …
```

The `Unknown` variant catches all kinds that have no registered handler. Native handlers
can further dispatch within `Unknown` by checking `projection.kind`:

```swift
// A handler for classified ads (kind:30402) installed from content-kind-30402:
struct ClassifiedAdRenderer: KindRenderer {
    func body(projection: EmbedKindProjection) -> some View {
        guard case .unknown(let p) = projection, p.kind == 30402 else { return EmptyView() }
        let price = p.tags.first(where: { $0.first == "price" })?[safe: 1]
        let location = p.tags.first(where: { $0.first == "location" })?[safe: 1]
        // render the classified ad card using price, location, p.contentTree
    }
}
```

Third parties can publish kind handler components to any jsrepo-compatible registry
(compatible with the M16 component format) and distribute them independently.

## Consequences

### What this changes

- `crates/nmp-content-fixtures/src/dto.rs`: `ContentTreeDto` / `SegmentDto` /
  `MarkdownNodeDto` are **retained** as a fixtures-only serde mirror for the gallery
  golden bundles. They are not on the production embed-host wire path (`ContentTreeWire`
  is). (Corrected from the original draft, which incorrectly said they would be deleted.)
- `crates/nmp-cli/registry/*/content-quote-card/`: **DELETED (F-CR-04).** The
  `NostrQuoteCard` widgets are superseded by the built-in `ShortNote` + `Unknown` (and
  Article/Highlight/Profile) handlers inside `content-kind-registry`. All three platform
  registries (`swiftui` / `compose` / `tui`) now ship a `content-kind-registry`
  component; the three `content-view` components depend on it and route every
  `nostr:` event ref through `EmbeddedEvent` instead of a quote card.
- `ios/Chirp/.../NostrContentView.swift`: **DONE.** The `quoteCardProvider` closure API is
  removed; event refs render through the `NostrKindRegistry` seam, bound by the app via
  `.embedEnvelopeSource(source, claimSink:registry:)` (defined in the registry's
  `EmbedHostEnvironment.swift`). `ios/Chirp/.../NostrQuoteCard.swift` is deleted.
- TUI `NostrQuoteCard` (`tui/content-quote-card`): **DELETED.** Markdown blockquote
  rendering moved inline into `content-view`; event refs render through the kind registry.
- `android/gallery/.../EmbedCard.kt`: **NOT YET migrated (deferred).** The standalone
  Android `gallery` module still renders its static `GalleryBundle` `EmbedEntry` DTOs via
  `EmbedCard.kt`; migrating it requires regenerating the bundle format to ship
  `EmbedKindProjection` envelopes (a separate bundle-generation change, out of F-CR-04
  scope). The production Android **app** already routes embeds through the kind registry
  (`android/app/.../ui/embed/EmbeddedEvent.kt` + `NostrKindRegistry.kt`). The standalone
  iOS/desktop/web gallery showcases similarly keep their own quote-card demo surfaces.

### What this does NOT change

- The `ContentTreeWire` wire format itself — already stable (M16-C1, 38 golden files).
- The `EmbedClaimRegistry` (claim/release/dedup machinery) — unaffected.
- `nmp-content::RenderContext` — extended with `RenderContextWire` serialisation but
  otherwise unchanged.
- ADR-0032 — projection fields remain raw protocol data.

### Risks

- Android `ContentTreeDto → ContentTreeWire` migration is the largest single task (F-CR-02).
  It must land before Android can use the kind registry.
- The `EmbedKindProjection` variants for well-known kinds (ShortNote, Article, Highlight,
  Profile) create coordinated Rust + native change requirements. Mitigated by the `Unknown`
  variant handling all other cases without Rust changes.
