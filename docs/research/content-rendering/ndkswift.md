# NDKSwift — Content & Embed Rendering Deep-Dive

## §1 Public API surface

There is **no `NDKContentView`**. There are **three** entry points, and they overlap:

1. **`NDKRichText`** — typealias for `NDKUIRichTextView` with default renderers (`Sources/NDKSwiftUI/Components/NDKUIRichTextView.swift:278-285`). Used for kind:1-style content with inline Nostr entities, hashtags, URLs.
2. **`NDKMarkdown`** — typealias for `NDKUIMarkdownView` with default renderers (`Sources/NDKSwiftUI/Components/NDKUIMarkdownView.swift:287-294`). Used for full markdown (kind:30023, READMEs, etc.).
3. **`NDKUIMarkdownRenderer`** — older AttributedString-based renderer (`Sources/NDKSwiftUI/Components/NDKUIMarkdownRenderer.swift:42-69`). Still public, still referenced by examples (`Examples/Apps/MarkdownDemo/.../FeedView.swift:147`), wraps content in `ScrollView` on its own. Overlaps in scope with `NDKMarkdown` — a clear pattern smell.

Canonical call site (from `Examples/Apps/Chirp/.../FeedView.swift:428`):

```swift
NDKRichText(content: event.content, tags: event.tags)
    .ndk(ndk)
```

The `.ndk(ndk)` modifier (`Sources/NDKSwiftUI/Components/Renderers/Core/RendererEnvironment.swift:76-78`) injects the NDK instance via SwiftUI environment; without it, parsing is skipped and the view falls back to raw `Text(content)` (`NDKUIRichTextView.swift:36-39, 46-50`).

Public-API surface of the rendering layer is roughly **30+ public types** across renderer protocols, default views, alternative views (compact/pill/embed), event/article cards, and one event-preview loader.

## §2 The tokenizer

Two pipelines exist:

- **`ContentParser.parseContentWithContext`** (`Sources/NDKSwiftCore/Core/Utilities/ContentParser.swift:41-273`) is the regex tokenizer used by `NDKRichText`. It walks the content once and emits `NDKParsedContent.Component` cases. Surfaced on `NDK` via `parseContent(_:tags:currentUserPubkey:)` (`Sources/NDKSwiftCore/Core/NDK.swift:761-768`).
- **`MarkdownParser.parse`** (`Sources/NDKSwiftUI/Components/MarkdownParser.swift:37-89`) is a hand-rolled markdown block parser used by `NDKMarkdown`. It produces `MarkdownBlock`/`MarkdownInline` (lines 6-25) and embeds `ContentParser` lookups for nostr entities inside inlines (`MarkdownParser.swift:546-617`).

`NDKParsedContent.Component` cases (`Sources/NDKSwiftCore/Core/Utilities/NDKParsedContent.swift:48-78`):

- `.text(String)`
- `.userMention(pubkey:, npub:)`
- `.nprofileMention(String)`
- `.eventMention(String)` (hex event id from `#[i]` tag-reference resolution)
- `.noteMention(String)` (`note1...`)
- `.neventMention(String)` (`nevent1...`)
- `.naddrMention(String)` (`naddr1...`)
- `.hashtag(String)`
- `.url(URL)`

Regex patterns (`ContentParser.swift:106-112`):

```
(@|nostr:)(npub1|nprofile1|note1|nevent1|naddr1)[a-zA-Z0-9]+   → "nostr"
(?<=\s|^)(#[^\s!@#$%^&*()=+./,\[{\]};:'"?><]+)                  → "hashtag"
https?://[^\s<>"{}|\\^`\[\]]+                                   → "url"
```

`#[index]` legacy NIP-08 references are resolved up-front from the event's `p`/`e` tags (`ContentParser.swift:55-97`). Tokenizing runs on a background actor (the `.task` modifier hops via `await ndk.parseContent`, `NDKUIRichTextView.swift:46-55`).

Image/video classification is **not** in the tokenizer — it's done at render time by `NDKUIRichTextView.isImageURL`/`isVideoURL` against a hard-coded extension list (`NDKUIRichTextView.swift:238-272`). Consecutive image URLs are post-grouped into carousels by `ImageGroupingUtils.groupConsecutiveImages` (`Sources/NDKSwiftUI/Components/Renderers/Core/ImageGroupingUtils.swift:12-54`), with whitespace/punctuation-only text bridging the groups.

What is **NOT tokenized at all**: lightning invoices (`lnbc...`), bolt12 offers, Cashu tokens, custom emoji shortcodes (NIP-30 `:shortcode:`), code blocks within kind:1 content, or quoted blockquotes in kind:1 content.

## §3 Embed fetch lifecycle

Every nostr-entity component that resolves to an event becomes an `EventPreviewLoader<Event>` view (`NDKUIRichTextView.swift:198-209`).

The loader (`Sources/NDKSwiftUI/Components/Renderers/EventPreviewLoader.swift`):

- **Fetch is per-view, async, started by `.task`** (line 105). One subscription per embed instance.
- **Placeholder**: a `ProgressView` + truncated event-id row inside a `Button` (lines 68-91). Tapping the placeholder opens `RelaySelectionSheet` (lines 108-119, 245-317) — an unusual UX that lets the user manually pick a relay (from embedded `nevent1` relay hints) and retry.
- **Error fallback**: when the subscription's first batch returns `nil`/never arrives, the view flips to `isLoading = false` and shows an "Event not found" warning row (lines 92-103). There is no "deleted" branch and no decay to plain text.
- **Click-through**: tap forwards to `onTap ?? envOnTap` from `EventTapHandler` (`DefaultEventView.swift:71-73`).
- **The fetch leaks until view-disappear**. `EventPreviewLoader.loadEvent` calls `ndk.subscribe(filter: filter)` (line 154) with `maxAge: 0`, which means `closeOnEose` defaults to `false` per NDK's "smart default" (`NDK.swift:646-647`). The body `break`s after the first event (lines 156-164), so iteration stops, but the subscription only fully cancels on `Task` cancellation when the loader's `.task` is torn down.
- **No central embed cache**. Each `EventPreviewLoader` instance owns its own state; scrolling away and back re-runs the `.task` and re-subscribes.
- **naddr** uses kind+author+d-tag filter (lines 130-144), not event ID.

## §4 Composability / registry

Renderers are pluggable via **generic type parameters**, not a registry. The six protocols (`Sources/NDKSwiftUI/Components/Renderers/Core/RendererProtocols.swift:16-44`):

```swift
public protocol MentionRenderer: View { init(pubkey: String, npub: String, onTap: MentionTapHandler?) }
public protocol HashtagRenderer: View { init(tag: String, onTap: HashtagTapHandler?) }
public protocol LinkRenderer:    View { init(url: URL, onTap: LinkTapHandler?) }
public protocol ImageRenderer:   View { init(urls: [URL], onTap: ImageTapHandler?) }
public protocol VideoRenderer:   View { init(url: URL, onTap: VideoTapHandler?) }
public protocol EventRenderer:   View { var event: NDKEvent { get }; init(event: NDKEvent, onTap: EventTapHandler?) }
```

Apps compose by typealias (`Examples/Apps/MarkdownDemo/.../AlternativeRenderers.swift:132-249`):

```swift
public typealias PillStyleRichText = NDKUIRichTextView<
    PillMentionView, PillHashtagView, PillLinkView,
    DefaultImageView, DefaultVideoView, DefaultEventView
>
```

Tap callbacks have a hybrid override pattern (`DefaultMentionView.swift:11, 28-30`): per-instance `onTap` parameter, otherwise read `@Environment(\.onMentionTap)`. Set via `.onMentionTap { … }` modifier (`RendererEnvironment.swift:80-103`).

Critical gap for app-level kind-dispatch: there is **no per-event-kind switch in the default `EventRenderer`**. `DefaultEventView` shows a chrome-free card with kind icon + label (`DefaultEventView.swift:40-74, 100-131`). To get a kind:30023 article card vs a kind:1 note card vs a kind:39089 follow-pack card inside an embed, **the app must write a custom `EventRenderer`** that `switch`es on `event.kind`. The doc-comment block at `DefaultEventView.swift:9-28` shows the recommended pattern:

```swift
struct AppEventRenderer: EventRenderer {
    let event: NDKEvent; let onTap: EventTapHandler?
    @ViewBuilder var body: some View {
        switch event.kind {
        case 30023, 30024: ArticleCardCompact(event: event, onTap: onTap)
        case 39089:        FollowPackCard(event: event, onTap: onTap)
        default:           DefaultEventView(event: event, onTap: onTap)
        }
    }
}
```

There is no built-in registry, no `@_dynamicReplacement`, no `EnvironmentKey` for swapping individual renderer types.

## §5 Markdown handling

**Markdown is opt-in at the call site.** `NDKRichText` does not auto-detect markdown — it just tokenizes and treats `**` as literal text. There is **no per-kind dispatch anywhere**: an app rendering kind:1 must call `NDKRichText`; an app rendering kind:30023 must call `NDKMarkdown`. No `kind` is ever inspected to pick a parser.

This becomes self-evident inside embedded event cards: `EventCardCompactView` and `EventCardInlineView` render the embed body with raw `Text(event.content)` (`EventCardCompactView.swift:44`, `EventCardInlineView.swift:42`) — no recursion into NDKRichText, so a quoted-note's inner nostr entities or URLs are shown as plain text, not parsed.

The block parser supports headings, code blocks, blockquotes, lists, horizontal rule, bold, italic, inline code, links, images (`MarkdownParser.swift:6-25, 37-89`). Bold/italic flatten children to a single `Text` so nested markup loses tap-handlers (`NDKUIMarkdownView.swift:147-155, 185-199`). User-toggle: no — styling is configurable via `MarkdownBlockConfig` presets `.default`, `.minimal`, `.compact` (`MarkdownBlockConfig.swift:61-74`), but the parse mode itself is fixed by which view you instantiate.

## §6 NIP-19 / NIP-21 plumbing

Decoding goes through `ContentTagger.decodeNostrEntity(_:)` (`Sources/NDKSwiftCore/Utils/ContentTagger.swift:302`), called inline from `EventPreviewLoader` (lines 32, 36, 53, 55, 132, 180, 192) and `NDKUIRichTextView` (line 177). It returns a `DecodedNostrEntity` with `pubkey`, `eventId`, `kind`, `identifier`, `relays`. Bech32 npub→hex uses `Bech32.pubkey(from:)` (`NDKUIMarkdownView.swift:228`).

Profile fetch flow for a mention:

1. Parser emits `.userMention(pubkey, npub)`.
2. `NDKUIRichTextView.renderSingleComponent` constructs `Mention(pubkey:npub:onTap:)` (line 174).
3. `DefaultMentionView.body` calls `ndk.profile(for: pubkey).displayName` (`DefaultMentionView.swift:22`).
4. `NDK.profile(for:)` (`NDK+ProfileAPI.swift:10-12`) is `@MainActor` and returns the same `@Observable NDKProfile` instance per pubkey from an LRU cache (limit 500 — `NDK.swift:184, 196-214`).
5. `NDKProfile.startObservation` (`NDKProfile.swift:71-114`) opens a `kind:0` subscription with `closeOnEose: true` and observes `ndk.cache.observeProfile(pubkey:)` for reactive metadata updates. Before metadata arrives, `displayName` falls back to a truncated `npub1234...` (lines 11-20).

`NDKUIProfilePicture` follows the same pattern but holds the profile in `@State` so SwiftUI subscribes to its `@Observable` changes (`NDKUIProfilePicture.swift:41, 91-94`).

## §7 Reactivity

- **Embed events**: `EventPreviewLoader.event` is `@State`; assignment on the main actor flips the view from placeholder to `Event(event:onTap:)` (`EventPreviewLoader.swift:17, 158-160`). Late-arriving embeds update in place.
- **Profiles**: `NDKProfile` is `@Observable` and `@MainActor`. Mention views read `ndk.profile(for: pubkey).displayName` directly in `body`; when the underlying `metadata` updates, the access in `body` triggers a re-render.
- **Parsed content**: `@State` in both `NDKUIRichTextView` (line 21) and `NDKUIMarkdownView` (line 18); `.task(id: content)` re-parses if `content` changes (lines 41, 36). Tag changes alone do **not** re-trigger parsing.

## §8 Memory + perf

- **`NDKRichText` uses `VStack`** (`NDKUIRichTextView.swift:95`). Every component renders eagerly — no virtualization. In a feed of 100 notes each with 1-3 embeds, all `EventPreviewLoader` instances mount when the row mounts.
- **`NDKMarkdown` uses `LazyVStack`** (`NDKUIMarkdownView.swift:31`) — virtualized block rendering. This asymmetry is a notable perf gap if you put many `NDKRichText` instances in a `List`.
- **Profile cache** is global LRU (500 entries) holding strong references (`NDK.swift:184-214`). One `NDKProfile` per pubkey is shared across all mentions site-wide.
- **Embed subscriptions** are per-view, not per-event-id deduplicated. Two `EventPreviewLoader`s pointing to the same `nevent` open two subscriptions. They close when the SwiftUI `.task` cancels (view disappears), but if the loader breaks out of the events loop after one event (line 162), the underlying subscription only stops on `deinit` after task cancellation — not at the `break`.
- **Tokenizer cost**: regex pass over `content`, runs once per content change on a background actor, result published to MainActor. No memoization across views with identical content.
- **Image carousel**: `DefaultImageView` uses Kingfisher's `KFImage` (`DefaultImageView.swift:1, 81`); networking + disk cache are Kingfisher-managed.

## §9 Gaps and pain points

Things NDKSwift's rendering layer **does not** do — that an app builder has to write themselves:

- **No per-kind dispatch** for embedded events. Default `EventRenderer` is a chrome-free placeholder; the app must subclass and `switch` on `event.kind`.
- **No recursive content rendering inside embeds**. `EventCardCompactView.swift:44` shows raw `Text(event.content)`, so a quoted note's nostr entities, URLs, hashtags, images are all dead text inside the embed card. NMP will hit this immediately.
- **No auto-detect markdown**. `NDKRichText` vs `NDKMarkdown` is a manual call-site decision. Kind:30023 is not implicit anywhere.
- **No lightning invoice rendering** (no `lnbc...` parser).
- **No NIP-30 custom emoji** (`:shortcode:` with `emoji` tags).
- **No bolt11/bolt12/Cashu token parsing**.
- **No code blocks / blockquotes in `NDKRichText`** — those are only in the markdown pipeline.
- **No OpenGraph / link preview** by default. `LinkEmbedView` (`LinkEmbedView.swift:18-44`) is a static card with just host + URL — no metadata fetch. Apps opt in via `LinkEmbedRichText` typealias.
- **`EventPreviewLoader` doesn't share fetches**. Same event referenced twice in the same feed = two subscriptions.
- **Subscription doesn't close at the `break`** — only when the SwiftUI task cancels.
- **No "deleted note"** state (NIP-09); the loader only knows "loaded" vs "not loaded".
- **Bold/italic flatten children**, so a mention inside `**` becomes inert text (`NDKUIMarkdownView.swift:147-155`).
- **Three overlapping APIs** (`NDKRichText`, `NDKMarkdown`, `NDKUIMarkdownRenderer`) with no clear deprecation; choose wrong and you get different tap-handler patterns (closures-on-the-view vs environment-modifiers).
- **`VStack` in `NDKRichText`** is unfit for long timelines without a `LazyVStack` wrapper from the caller.
- **No `EnvironmentKey` for renderer swapping**. To change one renderer type, the whole `NDKUIRichTextView<…>` typealias has to be re-declared.

## §10 What's worth borrowing for NMP

**Do this:**

1. **Six-protocol generic-typealias pattern** for renderer plug points (`RendererProtocols.swift:13-44`). Type-safe, no `AnyView`, app composition is one `typealias`. The model-T of Swift extensibility.
2. **Per-instance `onTap` + `@Environment` callback fallback** (`DefaultMentionView.swift:11, 28-30`, `RendererEnvironment.swift:80-103`). Lets apps wire navigation once at the root and override locally.
3. **Centralized `@Observable` profile cache** keyed by pubkey with LRU bounds (`NDK.swift:184-214`, `NDKProfile.swift:6-114`). Single subscription per pubkey, automatic re-render on metadata arrival, fallback display name before fetch. Apply the same shape to embedded-event resolution.
4. **`closeOnEose: true` for replaceable subscriptions** (`NDKProfile.swift:83`). Profile / `naddr` / kind:0 fetches must not stay open forever.
5. **`ImageGroupingUtils`-style post-tokenization grouping** for consecutive media — produces tasteful carousels instead of a column of image cards. Cite `ImageGroupingUtils.swift:12-54` as the reference algorithm.
6. **`.task(id: content)` re-parse on change** — clean SwiftUI-native pattern (`NDKUIRichTextView.swift:41-43`).
7. **Custom NSRegularExpression tokenizer** is small (~270 LoC), fast, and dependency-free — adopt the structure of `ContentParser.parseContentWithContext`.

**Don't do this:**

1. **Three overlapping public APIs.** NMP should ship **one** entry point, possibly with a `mode: .markdown | .plain | .auto` parameter, not three sibling types. The `NDKUIMarkdownRenderer` legacy proves what happens otherwise.
2. **`VStack` in the default content view.** Use `LazyVStack` from day one (`NDKMarkdown` got this right, `NDKRichText` didn't).
3. **No recursive embed content rendering.** NMP should render embed-body content with the same parser, not raw `Text`. Quote-tweets with inner mentions are table-stakes.
4. **No kind-dispatch in the default event renderer.** Ship a default that already switches on common kinds (1, 30023, 30311 live, 1063 file, 9802 highlight, podcast kinds) instead of forcing every app to re-implement.
5. **Per-embed subscriptions with no deduplication.** Build an `EventRegistry` analogous to NDKSwift's `profileCache` — one subscription per id, view-attaches/detaches don't re-fetch.
6. **Break-out without explicit subscription close.** When the first event arrives, call `subscription.cancel()` (or use `closeOnEose: true`); don't rely on `deinit` timing.
7. **`RelaySelectionSheet` as the loading placeholder.** Tappable "manually pick a relay" UX in a feed is novel but probably wrong as a default; gate it behind a long-press or debug-mode flag in NMP.
8. **Inline regex of URL extensions for image/video detection.** Centralize this — both `NDKUIRichTextView` and `NDKUIMarkdownView` have identical, duplicated `isImageURL`/`isVideoURL` functions.
9. **No NIP-30 custom emoji / no lnbc parsing.** These are common in real Nostr content; add token types from the start.

---

**Numerical summary:** ~50 Swift files in `Sources/NDKSwiftUI`, of which roughly 25 are the rendering layer (`Components/` + `Components/Renderers/`). Largest single file: `MarkdownParser.swift` at 619 lines, then `NDKUIMarkdownRenderer.swift` at 494, `NDKUIRichTextView.swift` at 364, `NDKUIMarkdownView.swift` at 330, `EventPreviewLoader.swift` at 317. Six renderer protocols; one event-preview loader; three public content-rendering entry points (`NDKRichText`, `NDKMarkdown`, `NDKUIMarkdownRenderer`). Dominant pattern: **SwiftUI generic View parameterized by protocol-conforming sub-Views**, composed via typealiases. No view-models, no closure-based rendering; all `@ViewBuilder` switches over an enum.
