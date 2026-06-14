import type { Component } from "./types";

// Content — SwiftUI
import contentCoreSwift from "../vendor/swiftui/content-core/NostrContentRenderer.swift?raw";
import contentCoreWireSwift from "../vendor/swiftui/content-core/ContentTreeWire.swift?raw";
import contentMinimalSwift from "../vendor/swiftui/content-minimal/NostrMinimalContentView.swift?raw";
import contentMinimalPreviewSwift from "../vendor/swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift?raw";
import contentViewSwift from "../vendor/swiftui/content-view/NostrContentView.swift?raw";
import contentGroupingSwift from "../vendor/swiftui/content-view/NostrContentGrouping.swift?raw";
import contentViewPreviewSwift from "../vendor/swiftui/content-view/Examples/NostrContentViewPreview.swift?raw";
import mentionChipSwift from "../vendor/swiftui/content-mention-chip/NostrMentionChip.swift?raw";
import quoteCardSwift from "../vendor/swiftui/content-quote-card/NostrQuoteCard.swift?raw";
import mediaGridSwift from "../vendor/swiftui/content-media-grid/NostrMediaGrid.swift?raw";

// Content — Compose
import composeContentRendererKotlin from "../vendor/compose/content-core/NostrContentRenderer.kt?raw";
import composeContentTreeWireKotlin from "../vendor/compose/content-core/ContentTreeWire.kt?raw";
import composeContentViewKotlin from "../vendor/compose/content-view/NostrContentView.kt?raw";
import composeContentGroupingKotlin from "../vendor/compose/content-view/NostrContentGrouping.kt?raw";
import composeMentionChipKotlin from "../vendor/compose/content-mention-chip/NostrMentionChip.kt?raw";
import composeQuoteCardKotlin from "../vendor/compose/content-quote-card/NostrQuoteCard.kt?raw";
import composeMediaGridKotlin from "../vendor/compose/content-media-grid/NostrMediaGrid.kt?raw";

// Content — Ratatui
import tuiContentTreeWireRust from "../vendor/tui/content-core/content_tree_wire.rs?raw";
import tuiContentRenderDataRust from "../vendor/tui/content-core/content_render_data.rs?raw";
import tuiTextWrapRust from "../vendor/tui/content-core/ratatui_text_wrap.rs?raw";
import tuiContentViewRust from "../vendor/tui/content-view/nostr_content_view.rs?raw";
import tuiContentWidgetRust from "../vendor/tui/content-view/nostr_content_widget.rs?raw";
import tuiMentionChipRust from "../vendor/tui/content-mention-chip/nostr_mention_chip.rs?raw";
import tuiMinimalContentRust from "../vendor/tui/content-minimal/nostr_minimal_content.rs?raw";
import tuiMediaGridRust from "../vendor/tui/content-media-grid/nostr_media_grid.rs?raw";
import tuiQuoteCardRust from "../vendor/tui/content-quote-card/nostr_quote_card.rs?raw";
import tuiKindRegistryModRust from "../vendor/tui/content-kind-registry/mod.rs?raw";
import tuiKindRendererRust from "../vendor/tui/content-kind-registry/kind_renderer.rs?raw";
import tuiKindRegistryRust from "../vendor/tui/content-kind-registry/nostr_kind_registry.rs?raw";
import tuiEmbedChromeRust from "../vendor/tui/content-kind-registry/embed_chrome_container.rs?raw";
import tuiEmbeddedEventRust from "../vendor/tui/content-kind-registry/embedded_event.rs?raw";

// Content — SwiftUI kind-dispatch registry + per-kind components
import swiftuiEmbedKindProjectionSwift from "../vendor/swiftui/content-kind-registry/EmbedKindProjection.swift?raw";
import swiftuiEmbedChromeContainerSwift from "../vendor/swiftui/content-kind-registry/EmbedChromeContainer.swift?raw";
import swiftuiNostrKindRegistrySwift from "../vendor/swiftui/content-kind-registry/NostrKindRegistry.swift?raw";
import swiftuiEmbeddedEventSwift from "../vendor/swiftui/content-kind-registry/EmbeddedEvent.swift?raw";
import swiftuiArticleEmbedSwift from "../vendor/swiftui/content-kind-30023/ArticleEmbed.swift?raw";
import composeArticleCardKotlin from "../vendor/compose/content-kind-30023/NostrArticleCard.kt?raw";
import desktopArticleCardRust from "../vendor/desktop/content-kind-30023/embed_article.rs?raw";
import swiftuiHighlightEmbedSwift from "../vendor/swiftui/content-kind-9802/HighlightEmbed.swift?raw";

// Content — Web (SolidJS)
import webContentViewTsx from "../vendor/web/content-view/NostrContentView.tsx?raw";
import webContentCoreTs from "../vendor/web/content-core/decodeContentTree.ts?raw";
import webContentMinimalTsx from "../vendor/web/content-minimal/NostrMinimalContentView.tsx?raw";
import webArticleCardTsx from "../vendor/web/content-kind-30023/NostrArticleCard.tsx?raw";
import webHighlightCardTsx from "../vendor/web/content-kind-9802/NostrHighlightCard.tsx?raw";
import webMentionChipTsx from "../vendor/web/content-mention-chip/NostrMentionChip.tsx?raw";
import webQuoteCardTsx from "../vendor/web/content-quote-card/NostrQuoteCard.tsx?raw";
import webMediaGridTsx from "../vendor/web/content-media-grid/NostrMediaGrid.tsx?raw";
import webKindRegistryTsx from "../vendor/web/content-kind-registry/NostrKindRegistry.tsx?raw";

export const contentComponents: Component[] = [
  {
    slug: "content-core",
    routeId: "content-core",
    version: "0.2.0",
    description:
      "Shared renderer configuration + ContentTreeWire wire type for app-owned Nostr content components.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-core",
        version: "0.2.0",
        dependencies: [],
        longDescription:
          "`NostrContentRenderer` is the small environment-injected struct every content component reads to pick colors and tap callbacks. Install it once; every other content component picks it up automatically.",
        files: [
          { source: "swiftui/content-core/NostrContentRenderer.swift", target: "Components/NostrContent/NostrContentRenderer.swift", role: "source", content: contentCoreSwift },
          { source: "swiftui/content-core/ContentTreeWire.swift", target: "Components/NostrContent/ContentTreeWire.swift", role: "source", content: contentCoreWireSwift },
        ],
        screenshots: ["content-core-ios-gallery-preview.png"],
        customization: [
          "Edit `NostrContentRenderer.swift` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
          "Inject a per-screen renderer with `.nostrContentRenderer(...)` on any SwiftUI view; child components pick it up via `@Environment(\\.nostrContentRenderer)`.",
          "`nmp update component` is a structural three-way merge: edits that don't touch upstream lines are preserved automatically.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-core",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrContentRenderer` is the small CompositionLocal-injected data class every content component reads to pick colors and tap callbacks. Install it once; every other Compose content component picks it up automatically.",
        files: [
          { source: "compose/content-core/NostrContentRenderer.kt", target: "Components/NostrContent/NostrContentRenderer.kt", role: "source", content: composeContentRendererKotlin },
          { source: "compose/content-core/ContentTreeWire.kt", target: "Components/NostrContent/ContentTreeWire.kt", role: "source", content: composeContentTreeWireKotlin },
        ],
        screenshots: ["content-core-kotlin-preview.png"],
        customization: [
          "Edit `NostrContentRenderer.kt` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
          "Inject a per-screen renderer with `CompositionLocalProvider(LocalNostrContentRenderer provides ...)`; child components pick it up via `LocalNostrContentRenderer.current`.",
          "`ContentTreeWire.kt` uses `kotlinx.serialization` with `@JsonClassDiscriminator(\"kind\")` so the JSON emitted by the Rust `nmp-content` crate decodes drift-free.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-core",
        version: "0.1.1",
        dependencies: [],
        longDescription:
          "`ContentTreeWire` mirrors the Rust `nmp-content` projection and `ContentRenderData` carries kernel-owned profile/event facts for resolved mentions and embedded notes. Widgets consume these values; they do not fetch or decode Nostr entities themselves.",
        files: [
          { source: "tui/content-core/content_tree_wire.rs", target: "src/components/nostr_content/content_tree_wire.rs", role: "source", content: tuiContentTreeWireRust },
          { source: "tui/content-core/content_render_data.rs", target: "src/components/nostr_content/content_render_data.rs", role: "source", content: tuiContentRenderDataRust },
          { source: "tui/content-core/ratatui_text_wrap.rs", target: "src/components/nostr_content/ratatui_text_wrap.rs", role: "source", content: tuiTextWrapRust },
        ],
        screenshots: ["tui-content-core-preview.png"],
        customization: [
          "Keep the wire types aligned with the kernel snapshot; app shells should only translate them into Ratatui lines/widgets.",
          "`ContentRenderData` is optional so cold-start rows can render immediately and hydrate when kind:0 or quoted events arrive.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-core",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`decodeContentTree(bytes)` is the one place the web stack turns the kernel's NFCT bytes (`claimed_events.content_tree_bytes` / feed projections) into a decoded `ContentTreeWire` — every web content component (content-view, content-minimal) consumes the decoded tree, never raw bytes. Re-exports the generated `ContentTreeWire` + `WireNodeKind` so consumers import the tree type and the decoder from one module, mirroring the native content-core wire-type + renderer split. Also ships `isTreeRenderable` — the honesty gate (non-empty AND placeholder-free) the gallery uses to refuse the raw-string fallback. Pure; never fetches or mocks.",
        files: [
          { source: "web/content-core/decodeContentTree.ts", target: "src/components/nostr-content/decodeContentTree.ts", role: "source", content: webContentCoreTs },
        ],
        screenshots: ["content-core-web-preview.png"],
        customization: [
          "`ContentTreeWire` is the generated FlatBuffers binding from the `nmp-content` schema — regenerate it with the web flatc pin when the schema changes; this module only adds the decode + honesty-gate helpers around it.",
          "Decode NFCT bytes once in your runtime and pass the decoded tree to the content components; they are pure walkers and never decode bytes themselves.",
        ],
      },
    },
  },
  {
    slug: "content-minimal",
    routeId: "content-minimal",
    version: "0.2.0",
    description: "Minimal Nostr content renderer with inline text, mentions, links, and hashtags.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-minimal",
        version: "0.2.0",
        dependencies: ["content-core", "render-identity"],
        longDescription:
          "A flow-layout view that walks an array of `NostrContentRun` values and renders text, mentions, hashtags, and links inline. The simplest component that gets you a working timeline cell.",
        files: [
          { source: "swiftui/content-minimal/NostrMinimalContentView.swift", target: "Components/NostrContent/NostrMinimalContentView.swift", role: "source", content: contentMinimalSwift },
          { source: "swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift", target: "Components/NostrContent/Examples/NostrMinimalContentPreview.swift", role: "example", content: contentMinimalPreviewSwift },
        ],
        screenshots: ["content-minimal-ios-gallery-preview.png"],
        customization: [
          "Pure SwiftUI — no UIKit, no third-party packages. Swap `FlowLayout` for `HStack` if you want different wrapping behaviour.",
          "The view reads `@Environment(\\.nostrContentRenderer)` for colors and callbacks, so customizing the look usually means tweaking the parent's renderer modifier rather than editing this file.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-minimal",
        version: "0.1.1",
        dependencies: ["content-core", "content-mention-chip"],
        longDescription:
          "A dense Ratatui line renderer for timeline cells. It resolves mention labels and quote previews from `ContentRenderData`, falling back to short ids until the kernel hydrates the referenced facts.",
        files: [
          { source: "tui/content-minimal/nostr_minimal_content.rs", target: "src/components/nostr_content/nostr_minimal_content.rs", role: "source", content: tuiMinimalContentRust },
        ],
        screenshots: ["tui-content-minimal-preview.png"],
        customization: [
          "Use this in list rows where a single preview line matters more than full block layout.",
          "Pair it with the host kernel render-intent loop that claims visible profile and event references.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-minimal",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "`<NostrMinimalContentView tree={...} fallback={...} />` walks a decoded `ContentTreeWire` and renders inline runs (text, mentions, event refs, hashtags, URLs, emphasis/strong) as a single flowing line, ignoring block structure — the simplest timeline-cell renderer. Falls back to the raw `fallback` string when no tree is present (honest-empty per D6). Verified live in the NMP web gallery against a real kind:1 note. For full block layout use `content-view`.",
        files: [
          { source: "web/content-minimal/NostrMinimalContentView.tsx", target: "src/components/nostr-content/NostrMinimalContentView.tsx", role: "source", content: webContentMinimalTsx },
        ],
        screenshots: ["content-minimal-web-preview.png"],
        customization: [
          "Block containers (paragraphs, headings, lists) flatten to their inline content with a trailing space — edit the `default` switch arm to change that behaviour.",
          "Style via the same `nostr-url` / `nostr-mention` / `nostr-hashtag` classes the full content-view uses, so the inline look matches.",
        ],
      },
    },
  },
  {
    slug: "content-view",
    routeId: "content-view",
    version: "0.1.1",
    description:
      "Full ContentTreeWire renderer. Stitches text runs, mentions, quote cards, and media grids into one view.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-view",
        version: "0.1.1",
        dependencies: [
          "content-core",
          "content-media-grid",
          "content-quote-card",
        ],
        files: [
          { source: "swiftui/content-view/NostrContentView.swift", target: "Components/NostrContent/NostrContentView.swift", role: "source", content: contentViewSwift },
          { source: "swiftui/content-view/NostrContentGrouping.swift", target: "Components/NostrContent/NostrContentGrouping.swift", role: "source", content: contentGroupingSwift },
          { source: "swiftui/content-view/Examples/NostrContentViewPreview.swift", target: "Components/NostrContent/Examples/NostrContentViewPreview.swift", role: "example", content: contentViewPreviewSwift },
        ],
        screenshots: ["content-view-ios-gallery-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content`. Each tree node maps to a sub-component you installed alongside it.",
          "Pin the tree's media layout, quote card style, and mention chip palette by overriding the `NostrContentRenderer` environment value on the parent view.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-view",
        version: "0.1.0",
        dependencies: ["content-core", "content-media-grid", "content-quote-card"],
        files: [
          { source: "compose/content-view/NostrContentView.kt", target: "Components/NostrContent/NostrContentView.kt", role: "source", content: composeContentViewKotlin },
          { source: "compose/content-view/NostrContentGrouping.kt", target: "Components/NostrContent/NostrContentGrouping.kt", role: "source", content: composeContentGroupingKotlin },
        ],
        screenshots: ["content-view-kotlin-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` and dispatches each block-level group to the matching sub-component. Customizing usually means editing the sub-component rather than this dispatcher.",
          "Inline runs are concatenated into a single `AnnotatedString` and rendered through `ClickableText` for tap-offset routing.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-view",
        version: "0.1.3",
        dependencies: ["content-core", "content-kind-registry", "content-mention-chip", "content-media-grid", "content-quote-card"],
        files: [
          { source: "tui/content-view/nostr_content_view.rs", target: "src/components/nostr_content/nostr_content_view.rs", role: "source", content: tuiContentViewRust },
          { source: "tui/content-view/nostr_content_widget.rs", target: "src/components/nostr_content/nostr_content_widget.rs", role: "source", content: tuiContentWidgetRust },
        ],
        screenshots: ["tui-content-view-preview.png"],
        customization: [
          "`NostrContentView` dispatches each `ContentTreeWire` node to the matching Ratatui sub-widget and keeps event refs as quote cards when render data is present.",
          "Host apps provide terminal image protocols for media URLs; the widget renders inline images when those protocols are present and falls back to text rows otherwise.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-view",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`<NostrContentView tree={...} fallback={...} />` is a SolidJS component that walks a kernel-decoded `ContentTreeWire` (NFCT) — the `nmp-content` tokenizer running behind the kernel's content-parser seam — into HTML: paragraphs, headings, lists, blockquotes, code, inline emphasis/strong/links, hashtags, URLs, emoji, media, and `nostr:` mention/event-ref anchors. It never parses, fetches, or mocks; when no tree is present it renders the raw `fallback` string verbatim (honest-empty per D6). Verified live in the NMP web gallery: a real kind:1 note and a real kind:30023 long-form article, both claimed from real relays and parsed by the real WASM kernel.",
        files: [
          { source: "web/content-view/NostrContentView.tsx", target: "src/components/nostr-content/NostrContentView.tsx", role: "source", content: webContentViewTsx },
        ],
        screenshots: ["content-view-web-preview.png"],
        customization: [
          "Style via the `nostr-*` element classes (`nostr-p`, `nostr-h`, `nostr-url`, `nostr-blockquote`, `nostr-code-block`, …); the component emits semantic HTML with no inline styles.",
          "The `tree` prop is a decoded `ContentTreeWire` from your kernel snapshot's `claimed_events` / feed projection; decode the NFCT bytes once in your runtime and pass the root object — the component is a pure walker.",
          "Mention and event-ref nodes render as `nostr:` anchors; install the embed-component layer to upgrade them to profile chips and quoted-event cards.",
        ],
      },
    },
  },
  {
    slug: "content-kind-registry",
    routeId: "content-kind-registry",
    version: "0.1.0",
    description: "Kind-dispatch registry for embedded Nostr events.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-kind-registry",
        version: "0.1.0",
        dependencies: ["content-core", "user-avatar"],
        longDescription:
          "Swift mirror of `tui/content-kind-registry`. `NostrKindRegistry` is a SwiftUI-friendly @MainActor dispatch table mapping `EmbedKindProjection` variants to `KindRenderer` implementations. `EmbeddedEvent` owns the claim/release lifecycle (via `.task(id:)` + `.onDisappear`), reads the resolved envelope from the app's `EmbedHost`, and dispatches through the registry. `EmbedChromeContainer` mirrors the TUI's depth-graded accent stripe so nested embeds visually scale identically across platforms.",
        files: [
          { source: "swiftui/content-kind-registry/EmbedKindProjection.swift", target: "Components/NostrContent/EmbedKindProjection.swift", role: "source", content: swiftuiEmbedKindProjectionSwift },
          { source: "swiftui/content-kind-registry/EmbedChromeContainer.swift", target: "Components/NostrContent/EmbedChromeContainer.swift", role: "source", content: swiftuiEmbedChromeContainerSwift },
          { source: "swiftui/content-kind-registry/NostrKindRegistry.swift", target: "Components/NostrContent/NostrKindRegistry.swift", role: "source", content: swiftuiNostrKindRegistrySwift },
          { source: "swiftui/content-kind-registry/EmbeddedEvent.swift", target: "Components/NostrContent/EmbeddedEvent.swift", role: "source", content: swiftuiEmbeddedEventSwift },
        ],
        screenshots: ["embed-article-ios-gallery-preview.png"],
        customization: [
          "Build the registry once at app start with `NostrKindRegistry.makeDefault()` then `registry.setArticle(ArticleEmbed())` / `registry.setHighlight(HighlightEmbed())` to swap in richer per-kind components.",
          "Inject it into the SwiftUI environment via `.environment(\\.nostrKindRegistry, registry)` — `NostrContentView` and `EmbeddedEvent` both read from there.",
          "Implement `EventClaimSinkProtocol` against your kernel FFI and inject it as `.environment(\\.embedClaimSink, sink)`; the embed view owns lifecycle, not your app code.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-kind-registry",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "`NostrKindRegistry` maps Rust-owned `EmbedKindProjection` envelopes to Ratatui renderers. It ships default short-note and unknown-kind handlers plus the `EmbeddedEvent` chrome wrapper.",
        files: [
          { source: "tui/content-kind-registry/mod.rs", target: "src/components/nostr_content/content_kind_registry/mod.rs", role: "source", content: tuiKindRegistryModRust },
          { source: "tui/content-kind-registry/kind_renderer.rs", target: "src/components/nostr_content/content_kind_registry/kind_renderer.rs", role: "source", content: tuiKindRendererRust },
          { source: "tui/content-kind-registry/nostr_kind_registry.rs", target: "src/components/nostr_content/content_kind_registry/nostr_kind_registry.rs", role: "source", content: tuiKindRegistryRust },
          { source: "tui/content-kind-registry/embed_chrome_container.rs", target: "src/components/nostr_content/content_kind_registry/embed_chrome_container.rs", role: "source", content: tuiEmbedChromeRust },
          { source: "tui/content-kind-registry/embedded_event.rs", target: "src/components/nostr_content/content_kind_registry/embedded_event.rs", role: "source", content: tuiEmbeddedEventRust },
        ],
        screenshots: ["tui-content-view-preview.png"],
        customization: [
          "Register additional `KindRenderer` implementations at app startup for event kinds your app cares about.",
          "Keep projection data in Rust; TUI renderers should only choose layout and styling for the typed envelope they receive.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-kind-registry",
        version: "0.1.0",
        dependencies: ["content-kind-30023", "content-kind-9802", "content-quote-card"],
        longDescription:
          "`<NostrEmbeddedEvent event={...} />` is the web kind-dispatch table. The host hydrates an `EmbeddedEventModel` from a resolved `claimed_events` entry (kind + content + tags + kernel-enriched author) and the registry routes it: kind:30023 → `NostrArticleCard`, kind:9802 → `NostrHighlightCard`, everything else → `NostrQuoteCard`. It also projects the raw event tags into each card's typed model, so the cards stay pure. Verified live in the NMP web gallery dispatching a real article, highlight, and note. The web twin of the SwiftUI/TUI `NostrKindRegistry` + `EmbeddedEvent`.",
        files: [
          { source: "web/content-kind-registry/NostrKindRegistry.tsx", target: "src/components/nostr-content/NostrKindRegistry.tsx", role: "source", content: webKindRegistryTsx },
        ],
        screenshots: ["content-kind-registry-web-preview.png"],
        customization: [
          "Add a kind by extending the `Switch` and writing a `toX(model)` tag-projection — the cards themselves stay pure model renderers.",
          "The host owns the claim/resolve lifecycle and passes a fully resolved envelope; the registry only chooses the renderer.",
        ],
      },
    },
  },
  {
    slug: "content-kind-30023",
    routeId: "content-kind-30023",
    version: "0.1.0",
    description: "Long-form article (NIP-23, kind:30023) embed renderer — hero image, title, summary, author chip.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-kind-registry", "user-avatar"],
        longDescription:
          "`ArticleEmbed` is the canonical NIP-23 card. Install via `registry.setArticle(ArticleEmbed())` on a `NostrKindRegistry`. Renders the article's `image` tag as a 16:9 hero, `title` as the headline, optional `summary` line, then an author byline with `NostrAvatar` + display name. The host's `EmbedHost` decodes the kind:30023 event into an `ArticleProjection` via the same `resolve_embed_projection` branch the Rust kernel uses.",
        files: [
          { source: "swiftui/content-kind-30023/ArticleEmbed.swift", target: "Components/NostrContent/ArticleEmbed.swift", role: "source", content: swiftuiArticleEmbedSwift },
        ],
        screenshots: ["embed-article-ios-gallery-preview.png"],
        customization: [
          "Replace the hero `AsyncImage` with your own loader (Nuke / Kingfisher) — the rest of the layout stays untouched.",
          "Bind a tap callback by wrapping the returned `AnyView` with `.onTapGesture` at the call site; the renderer itself is purely declarative.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "`NostrArticleCard` is the canonical NIP-23 Compose card. Per-kind dispatch lives in `NostrContentView.EventRefBlock` via an `articleCardProvider`, so the card stays self-contained on `compose/content-core` — no separate kind-registry component. Renders the article's `image` tag as a 16:9 hero (Coil `SubcomposeAsyncImage` with a placeholder fallback), the `title` as a semibold headline, an optional `summary` line, then an author byline with an avatar (`NostrIdenticon` fallback) + display name + `article · kind:30023` tag. The app hydrates a `NostrArticleCardModel` from a resolved `claimed_events` entry; the card only renders.",
        files: [
          { source: "compose/content-kind-30023/NostrArticleCard.kt", target: "Components/NostrContent/NostrArticleCard.kt", role: "source", content: composeArticleCardKotlin },
        ],
        screenshots: ["embed-article-kotlin-preview.png"],
        customization: [
          "Replace the hero `SubcomposeAsyncImage` with Glide or a custom `Painter` loader — the rest of the layout stays untouched.",
          "Wire per-kind dispatch by passing an `articleCardProvider` to `NostrContentView.EventRefBlock`; tap routes through `NostrContentCallbacks.onEventRefTap` unless you supply your own `onTap`.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`DefaultArticleRenderer` is the built-in NIP-23 long-form article renderer shipped with `tui/content-kind-registry`. It lays out an optional hero image (terminal image protocol when present, ASCII fallback otherwise), the article title styled as a heading, a summary paragraph, and an author byline that resolves the kind:0 display name from the kernel-projected `ArticleProjection`. Registered automatically on `NostrKindRegistry::with_defaults()`; swap it out per-app via `registry.set_article(Arc::new(MyArticleRenderer))`.",
        files: [
          { source: "tui/content-kind-registry/nostr_kind_registry.rs", target: "src/components/nostr_content/content_kind_registry/nostr_kind_registry.rs", role: "source", content: tuiKindRegistryRust },
        ],
        screenshots: ["tui-embed-article.png"],
        customization: [
          "Replace `DefaultArticleRenderer` by registering your own `KindRenderer` for `ArticleProjection` — the default lives inline in `nostr_kind_registry.rs` for easy copy-paste editing.",
          "Author byline pulls `author_display_name` straight from `ArticleProjection`; the Rust kernel resolves kind:0 enrichment before the snapshot reaches the TUI.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/content-kind-30023",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "iced `ArticleCard::new(&ArticleProjection, author_name)` renders the kind:30023 article inline: title (bold), a byline (red dot + host-resolved author label + short date), and a summary snippet, in a rounded bordered container. The author label is resolved by the displaying renderer (component-owned claiming), not the projection's static field. NOTE: the iced surface does NOT load the hero image yet — title + byline + summary only; hero loading is a documented follow-on in `embed_article.rs`.",
        files: [
          { source: "desktop/content-kind-30023/embed_article.rs", target: "src/components/nostr_content/embed_article.rs", role: "source", content: desktopArticleCardRust },
        ],
        screenshots: ["embed-article-desktop-preview.png"],
        customization: [
          "Hero image loading is a documented follow-on — add an `iced::widget::image` row above the title once the host decodes the article `image` tag into a `Handle`.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`<NostrArticleCard article={...} />` is the web NIP-23 long-form card. Pure renderer: the host hydrates a `NostrArticleCardModel` from a resolved `claimed_events` kind:30023 entry (the `image`/`title`/`summary` tags + the kernel-enriched author). Renders the `image` tag as a 16:9 hero, the `title` headline, an optional `summary`, then an author byline (avatar + name + `article · kind:30023`). Verified live in the NMP web gallery against the real showcase article. Mirrors the SwiftUI `ArticleEmbed` / Compose `NostrArticleCard`.",
        files: [
          { source: "web/content-kind-30023/NostrArticleCard.tsx", target: "src/components/nostr-content/NostrArticleCard.tsx", role: "source", content: webArticleCardTsx },
        ],
        screenshots: ["content-kind-30023-web-preview.png"],
        customization: [
          "Swap the hero `<img>` for your own lazy/loader component; the rest of the layout is plain semantic HTML styled by `nostr-article-card__*` classes.",
          "The host projects the article tags into the model (see `content-kind-registry`); the card itself never reads tags.",
        ],
      },
    },
  },
  {
    slug: "content-kind-9802",
    routeId: "content-kind-9802",
    version: "0.1.0",
    description: "Highlight (NIP-84, kind:9802) embed renderer — pull-quote, optional context line, source footer.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-kind-9802",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`HighlightEmbed` renders a NIP-84 highlight as a pull-quote: italic body inside a yellow-accented box, optional surrounding `context` line, and a source footer that branches on the highlight's `r` (URL), `e` (event id), or `a` (addressable event) tag. Install via `registry.setHighlight(HighlightEmbed())`.",
        files: [
          { source: "swiftui/content-kind-9802/HighlightEmbed.swift", target: "Components/NostrContent/HighlightEmbed.swift", role: "source", content: swiftuiHighlightEmbedSwift },
        ],
        screenshots: ["embed-highlight-ios-gallery-preview.png"],
        customization: [
          "Tweak the accent colour by editing the literal `Color.yellow.opacity(0.7)` — it merges cleanly on `nmp update component`.",
          "Extend `sourceFooter` to render rich previews when an `e` tag's referenced note has already been claimed.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-kind-9802",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`DefaultHighlightRenderer` is the built-in NIP-84 highlight renderer shipped with `tui/content-kind-registry`. It renders the highlighted text inside a yellow accent block, an optional context line in a muted tone, and a source footer that branches on the highlight's `r` (URL), `e` (event id), or `a` (addressable event) tag. Registered automatically on `NostrKindRegistry::with_defaults()`; swap it out per-app via `registry.set_highlight(Arc::new(MyHighlightRenderer))`.",
        files: [
          { source: "tui/content-kind-registry/nostr_kind_registry.rs", target: "src/components/nostr_content/content_kind_registry/nostr_kind_registry.rs", role: "source", content: tuiKindRegistryRust },
        ],
        screenshots: ["tui-embed-highlight-preview.png"],
        customization: [
          "Replace `DefaultHighlightRenderer` by registering your own `KindRenderer` for `HighlightProjection` — the default lives inline in `nostr_kind_registry.rs` for easy copy-paste editing.",
          "The source footer branches on `source_url` → `source_event_id` → `source_event_addr` in priority order; extend the match arms to render richer previews when the referenced event has been claimed.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-kind-9802",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`<NostrHighlightCard highlight={...} />` is the web NIP-84 highlight card. Pure renderer: the host hydrates a `NostrHighlightCardModel` from a resolved `claimed_events` kind:9802 entry. Renders the highlighted text as a pull-quote in a yellow-accented box, an optional `context` line, and a source footer that branches on the `r` (URL) → `e` (event id) → `a` (addressable) tag in priority order. Verified live in the NMP web gallery against the real showcase highlight. Mirrors the SwiftUI/TUI `HighlightEmbed`.",
        files: [
          { source: "web/content-kind-9802/NostrHighlightCard.tsx", target: "src/components/nostr-content/NostrHighlightCard.tsx", role: "source", content: webHighlightCardTsx },
        ],
        screenshots: ["content-kind-9802-web-preview.png"],
        customization: [
          "Tweak the accent colour by editing the `nostr-highlight-card` left-border / background literals in your stylesheet.",
          "Extend `sourceLabel` to render a rich preview when the `e`-tag source event has already been claimed.",
        ],
      },
    },
  },
  {
    slug: "content-mention-chip",
    routeId: "content-mention-chip",
    version: "0.1.0",
    description: "Avatar + display-name chip used inline anywhere a Nostr profile is referenced.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-mention-chip",
        version: "0.1.0",
        dependencies: ["content-core"],
        files: [
          { source: "swiftui/content-mention-chip/NostrMentionChip.swift", target: "Components/NostrContent/NostrMentionChip.swift", role: "source", content: mentionChipSwift },
        ],
        screenshots: ["content-mention-chip-ios-gallery-preview.png"],
        customization: [
          "Includes a tiny avatar loader fallback. Replace `AsyncImage` with your own image cache (Kingfisher, Nuke) if you already have one.",
          "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push your own profile view.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-mention-chip",
        version: "0.1.0",
        dependencies: ["content-core"],
        files: [
          { source: "compose/content-mention-chip/NostrMentionChip.kt", target: "Components/NostrContent/NostrMentionChip.kt", role: "source", content: composeMentionChipKotlin },
        ],
        screenshots: ["content-mention-chip-kotlin-preview.png"],
        customization: [
          "Uses Coil's `SubcomposeAsyncImage` for the avatar. Swap to Glide or a custom Painter by replacing the loader call in `MentionAvatar`.",
          "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push into your own navigator.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-mention-chip",
        version: "0.1.1",
        dependencies: ["content-core"],
        files: [
          { source: "tui/content-mention-chip/nostr_mention_chip.rs", target: "src/components/nostr_content/nostr_mention_chip.rs", role: "source", content: tuiMentionChipRust },
        ],
        screenshots: ["tui-content-mention-chip-preview.png"],
        customization: [
          "The chip displays the kernel-projected kind:0 name when available and shortens the npub/pubkey fallback locally.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-mention-chip",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        longDescription:
          "`<NostrMentionChip profile={...} />` is the inline avatar + display-name chip used anywhere a profile is referenced (the `embed-profile` body, `nostr:npub…` mentions). Pure renderer: takes a resolved `ProfileWire` (the host owns claim/resolve via the same projection the user-* components read). Reuses the `user-avatar` identicon palette + `displayLabel` so the look matches the avatar/name components exactly. Verified live in the NMP web gallery against a real resolved profile. Mirrors the SwiftUI/Compose `NostrMentionChip`.",
        files: [
          { source: "web/content-mention-chip/NostrMentionChip.tsx", target: "src/components/nostr-content/NostrMentionChip.tsx", role: "source", content: webMentionChipTsx },
        ],
        screenshots: ["content-mention-chip-web-preview.png"],
        customization: [
          "Picture vs. identicon is decided by `profile.pictureUrl`; the identicon color comes from `user-avatar`'s `identiconColor` so it stays consistent.",
          "Wrap the chip in an `<a>` at the call site to route mention taps to your own profile view.",
        ],
      },
    },
  },
  {
    slug: "content-quote-card",
    routeId: "content-quote-card",
    version: "0.1.1",
    description:
      "Quoted-note card — author header, content preview, subtle border. Drops into any feed.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-quote-card",
        version: "0.1.1",
        dependencies: ["content-core"],
        files: [
          { source: "swiftui/content-quote-card/NostrQuoteCard.swift", target: "Components/NostrContent/NostrQuoteCard.swift", role: "source", content: quoteCardSwift },
        ],
        screenshots: ["content-quote-card-ios-gallery-preview.png"],
        customization: [
          "Renders a hydrated `NostrQuoteCardModel`; apps resolve quoted events from their own state and pass preview text, author display data, and optional media thumbnails.",
          "Adjust the border, corner radius, and padding directly in the source file — they're literals, not configuration knobs, so they merge cleanly on `nmp update`.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-quote-card",
        version: "0.1.1",
        dependencies: ["content-core"],
        files: [
          { source: "compose/content-quote-card/NostrQuoteCard.kt", target: "Components/NostrContent/NostrQuoteCard.kt", role: "source", content: composeQuoteCardKotlin },
        ],
        screenshots: ["content-quote-card-kotlin-preview.png"],
        customization: [
          "Pick the variant per call-site — `Rich` for inline quote cards, `Collapsed` for a `View quote` affordance, `Missing` for an unresolved reference, `Compact` for dense feeds.",
          "Border, corner radius, and padding are literals so they merge cleanly on `nmp update component`.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-quote-card",
        version: "0.1.1",
        dependencies: ["content-core"],
        files: [
          { source: "tui/content-quote-card/nostr_quote_card.rs", target: "src/components/nostr_content/nostr_quote_card.rs", role: "source", content: tuiQuoteCardRust },
        ],
        screenshots: ["tui-content-quote-card-preview.png"],
        customization: [
          "Feed it a `WireNode::EventRef` plus `ContentRenderData`; unresolved references stay visible as a quote placeholder instead of raw `nostr:nevent...` text.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-quote-card",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`<NostrQuoteCard quote={...} nowSeconds={...} />` is the web quoted-note card (the kind:1 `embed-note` body). Pure renderer: the host hydrates a `NostrQuoteCardModel` from a resolved `claimed_events` entry (the referenced `nevent` claimed + kind:0-enriched). Renders an author header (avatar + name + relative time) above a content preview, in a subtle bordered card. Ships a pure `relativeTime(createdAt, now)` helper (now injected for testability). Verified live in the NMP web gallery against the real showcase note. Mirrors the SwiftUI/Compose `NostrQuoteCard`.",
        files: [
          { source: "web/content-quote-card/NostrQuoteCard.tsx", target: "src/components/nostr-content/NostrQuoteCard.tsx", role: "source", content: webQuoteCardTsx },
        ],
        screenshots: ["content-quote-card-web-preview.png"],
        customization: [
          "Pass `nowSeconds` from your app clock so the relative-time label stays pure/testable rather than reading `Date.now()` inside render.",
          "Style the card via the `nostr-quote-card__*` classes; the component emits plain semantic HTML.",
        ],
      },
    },
  },
  {
    slug: "content-media-grid",
    routeId: "content-media-grid",
    version: "0.1.0",
    description: "Adaptive 1–4 image / video grid for inline media attached to a note.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-media-grid",
        version: "0.1.0",
        dependencies: ["content-core"],
        files: [
          { source: "swiftui/content-media-grid/NostrMediaGrid.swift", target: "Components/NostrContent/NostrMediaGrid.swift", role: "source", content: mediaGridSwift },
        ],
        screenshots: ["content-media-grid-ios-gallery-preview.png"],
        customization: [
          "Grid layout is computed from the count: 1 = full-width 16:9, 2 = side-by-side, 3 = one-large + two-stacked, 4 = 2×2.",
          "Replace the `AsyncImage` calls with your own loader; the file exposes a `MediaThumbnailLoader` typealias for that swap.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-media-grid",
        version: "0.1.0",
        dependencies: ["content-core"],
        files: [
          { source: "compose/content-media-grid/NostrMediaGrid.kt", target: "Components/NostrContent/NostrMediaGrid.kt", role: "source", content: composeMediaGridKotlin },
        ],
        screenshots: ["content-media-grid-kotlin-preview.png"],
        customization: [
          "Layout is count-driven: 1 = full-width 16:9, 2 = side-by-side, 3 = one large + two stacked, 4+ = 2×2 with `+N more` overlay — identical to the SwiftUI variant.",
          "Replace `SubcomposeAsyncImage` with your own loader if you already use Glide/Picasso. The cell composable is intentionally small to make the swap painless.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-media-grid",
        version: "0.1.1",
        dependencies: ["content-core"],
        files: [
          { source: "tui/content-media-grid/nostr_media_grid.rs", target: "src/components/nostr_content/nostr_media_grid.rs", role: "source", content: tuiMediaGridRust },
        ],
        screenshots: ["tui-content-media-grid-preview.png"],
        customization: [
          "Pass host-created `ratatui-image` protocols for URLs that have already been fetched and decoded. The widget lays out up to four inline images and leaves fetching/caching outside the display component.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-media-grid",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`<NostrMediaGrid urls={...} />` is the adaptive 1–4 image grid for inline note media. Layout is count-driven: 1 = full-width 16:9, 2 = side-by-side, 3/4 = grid, with a `+N` overlay on the 4th cell when more remain — identical to the SwiftUI/Compose variants. Pure renderer: the host extracts the URLs (from the content tree's `Media`/`Image` nodes or `imeta` tags) and owns fetching; the grid only lays out the `<img>` cells. Verified live in the NMP web gallery against the real images the kernel parsed out of the showcase article.",
        files: [
          { source: "web/content-media-grid/NostrMediaGrid.tsx", target: "src/components/nostr-content/NostrMediaGrid.tsx", role: "source", content: webMediaGridTsx },
        ],
        screenshots: ["content-media-grid-web-preview.png"],
        customization: [
          "Swap the `<img>` for your own lazy/loader cell; the grid layout (driven by `data-count`) stays the same.",
          "Validate URLs load before passing them in if you want to guarantee no broken cells — the NMP gallery preloads candidates and only renders images that decode.",
        ],
      },
    },
  },
];
