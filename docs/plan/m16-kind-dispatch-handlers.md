# M16 Kind Dispatch: Handlers and Validation

This page expands the post-registry work: kind-specific handlers, nested embed
fixtures, and legacy deletion.

## F-CR-09: Long-Form Articles

Installable components:

- `crates/nmp-cli/registry/swiftui/content-kind-30023/`
- `crates/nmp-cli/registry/tui/content-kind-30023/`
- `crates/nmp-cli/registry/compose/content-kind-30023/`

Renderer behavior:

- Use `EmbedKindProjection::Article`.
- Show title, summary, author, and optional hero image.
- Treat the article body tree as navigation target content, not as the entire
  inline embed body.
- Register with the platform `NostrKindRegistry`.

Android should promote the existing article preview behavior from the old
`EmbedCard.kt` instead of re-inventing the card.

## F-CR-10: NIP-84 Highlights

Installable components:

- `crates/nmp-cli/registry/swiftui/content-kind-9802/`
- `crates/nmp-cli/registry/tui/content-kind-9802/`
- `crates/nmp-cli/registry/compose/content-kind-9802/`

Renderer behavior:

- Use `EmbedKindProjection::Highlight`.
- Render highlighted text with a left accent.
- Prefer source footer order: URL, addressable event, event id.
- Keep source values raw; formatting stays in the renderer.

## F-CR-11: Profile Cards

Installable components:

- `crates/nmp-cli/registry/swiftui/content-kind-0/`
- `crates/nmp-cli/registry/tui/content-kind-0/`
- `crates/nmp-cli/registry/compose/content-kind-0/`

Renderer behavior:

- Use `EmbedKindProjection::Profile`.
- Show avatar or identicon, display name, npub/pubkey chip, and about preview.
- Do not fetch profile data directly from native UI; data arrives through the Rust
  projection/envelope path.

## F-CR-12: Nested Fixtures

Add fixture scenarios under `crates/nmp-content-fixtures/src/scenarios/`:

- Kind:1 quoting a kind:30023 article.
- Kind:1 quoting itself transitively, collapsed as a cycle.
- A chain at `max_depth`, collapsed as a depth-limit case.
- Kind:1 quoting an unknown kind, rendered through unknown fallback.
- Kind:1 quoting a kind:9802 highlight.

Each scenario should produce `ContentTreeWire` plus an embeds map keyed by event id
to `EmbeddedEventEnvelope`.

Required platform coverage:

- iOS snapshot tests with `NostrKindRegistry.makeDefault()`.
- TUI buffer snapshots for each scenario.
- Compose instrumentation tests asserting the selected renderer.

## F-CR-04: Legacy Deletion

Open only after F-CR-12 is green in CI.

Delete:

- Legacy `EmbedEntry.article` and `EmbedEntry.list` DTO fields.
- `ArticleHeaderDto`, `ListDto`, and `ListRowDto`.
- Platform `content-quote-card` registry components once their default renderer logic
  has moved into `content-kind-registry`.
- iOS `NostrQuoteCard.swift`.
- Android `EmbedDto.kt` residuals.

This must be a deletion PR. Do not leave compatibility stubs unless BACKLOG gets a
specific staged deadline before the PR opens.
