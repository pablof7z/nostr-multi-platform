# M16 Component Registry - App-Owned Native UI Kits

> Part of [M16 - CLI + starter app + recipe book](m16-cli-starter.md).
> This is the implementation plan for installable, updateable, app-owned
> native UI source components. The first shipped kit is Nostr content rendering
> for iOS SwiftUI and Android Compose.

## Product Goal

An app developer can start with the framework substrate and install source
components they own:

```sh
nmp add component swiftui/content-minimal
nmp add component swiftui/content-social-rich --with example test
nmp update component swiftui/content-social-rich
```

The installed files live in the app tree, are normal app source, and can be
edited freely. Later updates are interactive source updates against a recorded
upstream baseline; no command silently overwrites local edits.

## Boundary

NMP owns the reusable protocol and projection contract:

- `nmp-content` tokenization, media grouping, Nostr URI classification,
  markdown shape, custom emoji, invoice detection, and recursion helpers.
- Kernel/view-module ownership of fetch, dedupe, tombstone, profile, and embed
  resolution facts.
- `ContentTreeWire` and related generated binding types as the stable renderer
  input.

Apps own the native rendering source:

- SwiftUI and Compose files copied into app source directories.
- Visual styling, density, gesture affordances, preview presentation, and route
  callbacks.
- App-specific renderer branches, such as podcast cards or venue cards.

Native components may branch on render node type and UI variant. They must not
decide relay policy, fetch policy, business state, cache ownership, retry
behavior, or derived display strings that belong in Rust projections.

## End State

M16 is complete when NMP has:

1. A registry manifest format for app-owned source components.
2. `nmp add component` and `nmp update component` with a lock file recording
   component version, registry source, installed files, upstream hashes, and
   required wire-schema versions.
3. A local, offline component registry used by `nmp init` and tests.
4. Optional jsrepo-compatible export of the same registry for developers who
   already use jsrepo-style workflows.
5. iOS SwiftUI content-rendering kits that can render `ContentTreeWire`.
6. Android Compose content-rendering kits with matching behavior and naming.
7. Fixture-driven visual and decoder tests for both platforms.
8. At least one real app, preferably Chirp, consuming copied components rather
   than private one-off renderers.

## Component Model

Each registry item has:

- `id`: stable name such as `swiftui/content/mention-rich-preview`.
- `version`: semver for the source item.
- `target`: platform family, for example `swiftui` or `compose`.
- `requires`: NMP crate/schema contracts, for example
  `content-tree-wire = "1"` and `kernel-mention-profile = "1"`.
- `files`: source files to copy, with role metadata: `source`, `doc`,
  `example`, `test`, `fixture`.
- `dependencies`: other registry items that must be installed first.
- `default_path`: app-local install path, overridable by the app.

The registry supports items and bundles. Items are narrow building blocks.
Bundles install a coherent renderer set.

Example content bundles:

| Bundle | Purpose |
|---|---|
| `swiftui/content-minimal` | Inline text, minimal mentions, links, media placeholders, compact quote fallback. |
| `swiftui/content-social-rich` | Avatar mentions, media grid/lightbox, rich quote cards, article cards, long-press mention preview. |
| `compose/content-minimal` | Android equivalent of the minimal SwiftUI kit. |
| `compose/content-social-rich` | Android equivalent of the rich SwiftUI kit. |

Example installable variants:

| Item | Behavior |
|---|---|
| `swiftui/content/mention-minimal` | Inline `@name`; tap callback only. |
| `swiftui/content/mention-avatar` | Inline avatar + display name chip. |
| `swiftui/content/mention-rich-preview` | Minimal inline face plus press-and-hold profile preview fed by Rust projection data. |
| `swiftui/content/event-quote-compact` | One-line author/body preview with tap callback. |
| `swiftui/content/event-quote-card` | Full quoted-event card with recursion collapse state. |
| `swiftui/content/media-grid` | Image grouping and video/audio placeholders from `ContentTreeWire`. |
| `compose/content/mention-minimal` | Compose parity for minimal mentions. |
| `compose/content/mention-rich-preview` | Compose long-press profile preview using the same wire facts. |

## Renderer Configuration

Do not copy NDK-svelte's mutable global singleton. App-owned source can still
borrow its useful shape:

- Per-token renderers: mention, hashtag, link, media, emoji, invoice, fallback.
- Per-kind embedded-event renderers: kind 1 note, kind 30023 article, generic
  unsupported event, deleted event.
- Resolution order: explicit renderer passed to the view, platform context,
  local default.
- Renderer snapshots for per-instance callbacks, so a timeline row can bind
  `onMentionTap`, `onMentionPreview`, `onEventTap`, and `onHashtagTap` without
  mutating global state.

SwiftUI shape:

```swift
struct NostrContentRenderer {
    var mention: MentionRenderer
    var event: EventRenderer
    var media: MediaRenderer
    var fallback: FallbackRenderer
}
```

Use `EnvironmentValues` for subtree defaults and explicit parameters for
per-view overrides.

Compose shape:

```kotlin
data class NostrContentRenderer(
    val mention: MentionRenderer,
    val event: EventRenderer,
    val media: MediaRenderer,
    val fallback: FallbackRenderer,
)
```

Use `CompositionLocal` for subtree defaults and explicit parameters for
per-view overrides.

## Milestones

### M16-C0 - Registry Decision And Contracts

Deliverables:

- ADR or plan update fixing the component-source model as app-owned source, not
  platform UI packages.
- Component manifest schema and `nmp.components.lock` schema.
- Registry root layout, initially local to this repo.
- Policy gates: generated bindings are never copied as registry source;
  components declare the generated module names and wire-schema versions they
  expect.

Exit gate:

- A fixture registry with one no-op component can be validated without network
  access.
- Doctrine lint or a targeted registry check rejects component files that import
  forbidden Rust-policy names or contain network/fetch/cache ownership.

### M16-C1 - Content Wire Contract Freeze

Deliverables:

- `ContentTreeWire` schema versioning.
- Generated or mirrored Swift and Kotlin DTO coverage for every current
  `WireNode` variant, including best-effort placeholder nodes.
- Fixture JSON bundles covering the content gallery matrix.
- Decoder tests that prove older bundles degrade through placeholders instead
  of crashing.

Exit gate:

- iOS and Android can decode the same content fixture bundle.
- Unknown node kinds render through the fallback path, never blank content.

### M16-C2 - `nmp add component`

Deliverables:

- CLI command copies registry item files into the app tree.
- Supports role filtering with `--with doc example test fixture`.
- Resolves item dependencies within the same registry.
- Writes `nmp.components.lock`.
- Defaults to the local offline registry so `nmp init` remains network-free.

Exit gate:

- Temp-app integration test installs `swiftui/content-minimal`.
- Re-running `add` detects already-installed files and requires explicit
  overwrite or update.

### M16-C3 - iOS SwiftUI Content Kits

Deliverables:

- `NostrRichText.swift` wire-tree walker.
- `NostrContentRenderer.swift` configuration model and environment key.
- Mention variants: minimal, avatar chip, rich preview.
- Event variants: compact quote, rich quote card, deleted/missing/collapsed
  fallback.
- Media variants: basic media row and image grid.
- Markdown/article renderer for supported `WireNode` markdown blocks.
- Example fixtures and snapshot tests.

Exit gate:

- iOS NmpGallery renders every content-gallery fixture through installed
  registry source.
- Chirp can replace its private content renderer with a copied kit while
  preserving app-local styling edits.

### M16-C4 - Android Compose Content Kits

Deliverables:

- Compose wire-tree walker.
- `NostrContentRenderer` data class and `LocalNostrContentRenderer`.
- Mention variants matching iOS naming and behavior.
- Event, media, fallback, and markdown variants matching iOS fixture behavior.
- Android fixture app or screen that renders the shared content-gallery bundle.

Exit gate:

- Android and iOS fixture matrices agree on node coverage and fallback states.
- Long-press mention preview uses projection data or app callbacks; it performs
  no native fetch or cache mutation.

### M16-C5 - `nmp update component`

Deliverables:

- Fetches the installed item's upstream version and baseline hashes.
- Computes local-change status for every installed file.
- Applies clean updates automatically only when the local file matches the
  previous upstream baseline.
- Presents conflict files as explicit merge work; never discards app edits.
- Supports pinning a component to a version.

Exit gate:

- Integration test edits a copied mention renderer, updates the upstream item,
  and proves the local edit is preserved with a conflict report.
- Updating an untouched copied component produces a clean patch and lock update.

### M16-C6 - jsrepo-Compatible Export

Deliverables:

- Generate a jsrepo-compatible registry from the NMP component manifest.
- Publish or dry-run publish a scoped registry such as `@nmp/native-ui`.
- Preserve NMP's own CLI as the canonical offline path.
- Document when to use `nmp add component` versus `jsrepo add`.

Exit gate:

- A smoke project can install the SwiftUI content bundle through jsrepo.
- The installed files and metadata match the NMP CLI install result.

### M16-C7 - Recipes And Real-App Adoption

Deliverables:

- Recipe: minimal Nostr timeline app with `content-minimal`.
- Recipe: social timeline with `content-social-rich`.
- Recipe: app-local renderer override, such as replacing mention rendering or
  adding a custom kind card.
- Chirp migration report documenting which files became copied component source
  and which app-specific edits Chirp kept.

Exit gate:

- An external developer or clean-room agent can scaffold an app, install a
  content kit, customize one renderer, update the kit, and keep the
  customization.

## Acceptance Tests

- `cargo test -p nmp-cli component_registry` for add/update/lock behavior.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` after registry checks
  land.
- iOS gallery render test over content fixtures.
- Android gallery render test over the same fixtures.
- Manual update drill: local edit plus upstream update produces a visible
  conflict report, not an overwrite.

## Non-Goals

- No published `nmp-content-swiftui` or `nmp-content-compose` framework package.
- No native fetchers, relay subscriptions, cache writers, or policy engines in
  copied components.
- No generated DTOs copied from the registry. Generated bindings are produced by
  `nmp-codegen` or the platform binding build.
- No requirement that app developers install Node or jsrepo for the default
  `nmp init` path.

