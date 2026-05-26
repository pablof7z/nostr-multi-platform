# M16 Kind Dispatch: Foundation

This page expands the first two M16 kind-dispatch backlog items. The canonical
backlog entries remain in [BACKLOG.md](../BACKLOG.md); this page exists only to
make the implementation steps concrete without bloating the milestone index.

## F-CR-01: Rust Projection Envelope

Create `crates/nmp-content/src/embed_projection/` with these public exports:

- `EmbedKindProjection`
- `ShortNoteProjection`
- `ArticleProjection`
- `HighlightProjection`
- `ProfileProjection`
- `UnknownProjection`
- `EmbeddedEventEnvelope`
- `RenderContextWire`
- `resolve_embed_projection(event, ctx)`

`resolve_embed_projection` is the only content-rendering dispatch table that may
name Nostr event kinds. It belongs in `nmp-content`, not `nmp-core`, because it is
a rendering projection sidecar rather than substrate protocol policy.

The initial variants are:

| Variant | Event kind | Purpose |
|---|---:|---|
| `ShortNote` | 1 | Render quoted notes with the normal content tree plus preview media. |
| `Article` | 30023 | Render long-form article previews from title, summary, image, and body tree. |
| `Highlight` | 9802 | Render NIP-84 highlights with source references. |
| `Profile` | 0 | Render profile cards from kind:0 profile data. |
| `Unknown` | any other kind | Preserve raw kind, content, tags, parsed tree, and `alt` text for custom renderers. |

All fields must follow ADR-0032:

- Pubkeys and event ids stay as lowercase hex.
- Timestamps stay as Unix seconds.
- Display names, picture URLs, and tag values stay verbatim from events.
- Native-facing strings are not preformatted in Rust.
- Unknown kinds keep raw `tags` and `content_tree` so native handlers do not need
  Rust changes for every new Nostr kind.

`EmbeddedEventEnvelope` is the FFI/wire unit:

- `uri`: original `nostr:` URI that triggered the embed.
- `primary_id`: event id hex or address coordinate.
- `render_context`: serializable recursion state.
- `projection`: the kind-dispatched payload.
- `collapsed`: depth/cycle/unsupported guard result.
- `collapse_reason`: machine-readable reason when collapsed.

Required tests:

- One resolver test for each initial variant.
- Serde round-trip coverage for the tagged enum shape.
- Unknown-kind coverage proving `kind`, `tags`, `content`, `content_tree`, and
  `alt_text` survive without a custom Rust variant.

## F-CR-02: Android Gallery Wire Migration

Migrate `android/gallery/` from the legacy DTO tree to `ContentTreeWire`.

Required changes:

- Copy or generate `ContentTreeWire.kt` from the Compose registry component.
- Rename `SegmentDtoView.kt` to `WireNodeView.kt`.
- Rewrite rendering around arena lookup: `ContentTreeWire` plus root indices.
- Delete `SegmentDto`, `ContentTreeDto`, and `MarkdownNodeDto` once no call sites remain.
- Change `EmbedEntry.rendered` from `ContentTreeDto?` to `ContentTreeWire?`.
- Leave `WireNode.EventRef` ready to call `EmbeddedEvent` when F-CR-07 lands.

Verification:

```sh
./gradlew :gallery:test
```
