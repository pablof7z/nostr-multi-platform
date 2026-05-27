# Recipe: Nostr Content Rendering with NMP Components

Nostr content arrives as plaintext with embedded NIP-27 mentions, hashtags, and note references. Rendering it correctly — on every screen, in every context — is exactly the detail you should not be reimplementing per app.

Three patterns. Copy the component, edit to taste, ship.

## Prerequisites

Clone the NMP repo and install the CLI:

```sh
git clone https://github.com/pablof7z/nostr-multi-platform
cd nostr-multi-platform
cargo install --path crates/nmp-cli
```

## Recipe 1 — Minimal Inline Text

Best for: notifications, DM previews, compact list rows where layout space
is tight and you only need inline text, mentions, hashtags, and links.

### Install

```sh
nmp add component swiftui/content-minimal
```

This copies two files into your project:
- `Components/NostrContent/NostrContentRenderer.swift` — theming environment
- `Components/NostrContent/NostrMinimalContentView.swift` — the view

### Usage

```swift
import SwiftUI

struct NoteRow: View {
    // content_runs comes from your NMP projection snapshot
    let runs: [NostrContentRun]

    var body: some View {
        NostrMinimalContentView(runs: runs)
            .lineLimit(3)
    }
}
```

### Wiring to NMP projections

The `content_runs` field is produced by `nmp-content` and shipped in the
`TimelineEventCard` snapshot. In Swift:

```swift
let runs = snapshot.content_runs.map { r in
    NostrContentRun(
        id: r.id,
        label: r.label,
        kind: r.kind
    )
}
```

### Theming

Wrap the root of your view hierarchy with a renderer:

```swift
ContentView()
    .nostrContentRenderer(NostrContentRenderer(
        mentionColor: .accentColor,
        hashtagColor: .purple,
        linkColor: .teal
    ))
```

---

## Recipe 2 — Social Timeline with Full Content Kit

Best for: a Nostr social timeline where notes have images, quote cards,
inline mentions with avatars, and rich markdown rendering.

### Install

```sh
nmp add component swiftui/content-view
```

This resolves and installs the full tree:
- `swiftui/content-core` — `ContentTreeWire.swift` + renderer environment
- `swiftui/content-media-grid` — photo grid (1/2/3/4+ images)
- `swiftui/content-mention-chip` — avatar mention chip
- `swiftui/content-quote-card` — embedded event card
- `swiftui/content-view` — the main renderer

To also include examples:

```sh
nmp add component swiftui/content-view --with example
```

### Usage

```swift
import SwiftUI

struct NoteRow: View {
    let tree: ContentTreeWire           // from your NMP snapshot
    let mentionProfiles: [String: Profile]
    let quotedEvents: [String: QuotedEvent]

    var body: some View {
        NostrContentView(
            tree: tree,
            mentionLabel: { uri in
                mentionProfiles[uri.primaryId]?.displayName ?? shortenHex(uri.primaryId)
            },
            quoteCardProvider: { uri in
                guard let event = quotedEvents[uri.primaryId] else { return nil }
                return NostrQuoteCardModel(
                    id: event.id,
                    authorDisplayName: event.authorDisplayName,
                    content: event.contentPreview,
                    mediaThumbnailUrl: event.firstImageUrl,
                    createdAtDisplay: event.relativeTime
                )
            }
        )
    }
}
```

### Decoding `ContentTreeWire` from JSON

If your snapshot ships `content_tree_wire` as JSON (the default NMP codegen
output), decode it once and pass the struct:

```swift
let decoder = JSONDecoder()
let tree = try decoder.decode(ContentTreeWire.self, from: jsonData)
```

### Quote card states

The quote card has four variants controlled by what you pass to
`quoteCardProvider`:

| State | How to trigger |
|---|---|
| `.collapsed` | Pass `nil` for `quoteCardProvider` |
| `.rich` | Provider returns a `NostrQuoteCardModel` |
| `.missing` | Provider is set but returns `nil` |
| `.compact` | App calls `NostrQuoteCard(model:variant:.compact)` directly |

---

## Recipe 3 — App-Local Renderer Override

Best for: when you need to replace just one rendering behavior — for example,
using a richer mention preview or adding a custom kind card — without forking
the entire kit.

### Override a single behavior

Because you own the source, the simplest override is to edit the file:

```sh
# After installing the kit, edit the mention chip to use your design system
$EDITOR Components/NostrContent/NostrMentionChip.swift
```

The `nmp update component` command will detect the edit and show you a diff
when upstream releases a new version — your local change is never silently
overwritten.

### Add a custom event kind card

Open `NostrContentView.swift` in your project and extend `eventRefView(_:)`:

```swift
private func eventRefView(_ uri: NostrWireUri) -> some View {
    // Add your app-specific kind card here before the default path
    if uri.eventKind == 30023 {
        return AnyView(ArticleCard(uri: uri))
    }
    // ... existing logic
}
```

### Override colors at the call site

The renderer environment is per-subtree, so different timelines can use
different themes:

```swift
// DM thread — quieter palette
DMBubble(tree: tree)
    .nostrContentRenderer(NostrContentRenderer(
        mentionColor: .secondary,
        hashtagColor: .secondary,
        linkColor: .accentColor
    ))
```

### Update while keeping your overrides

```sh
nmp update component swiftui/content-view
```

For files you edited, the CLI reports a conflict instead of overwriting:

```
conflict: Components/NostrContent/NostrContentView.swift
  upstream changed, local edits detected — merge manually
```

Files you left untouched are updated silently.

---

## Updating the kit

After NMP releases new component versions:

```sh
nmp update component swiftui/content-view
```

The lock file records each file's upstream SHA-256. Files you haven't
touched are updated silently. Files with local edits show a conflict
report so you can cherry-pick the upstream changes manually.

---

## See Also

- [`docs/plan/m16-component-registry.md`](../plan/m16-component-registry.md) — full M16 spec
- [`crates/nmp-cli/registry/registry.toml`](../../crates/nmp-cli/registry/registry.toml) — registry manifest
- [`web/registry/`](../../web/registry/) — component showcase website
