import { nativeSource } from "./vendorSource";
import type { Component } from "./types";
import { webContentCore, webContentMediaGrid, webContentMentionChip, webContentMinimal, webContentView } from "./contentWeb";
import { contentKindComponents } from "./contentKindComponents";

// Content — SwiftUI
const contentCoreSwift = nativeSource("registry/swiftui/content-core/NostrContentRenderer.swift");
const contentCoreWireSwift = nativeSource("registry/swiftui/content-core/ContentTreeWire.swift");
const contentMinimalSwift = nativeSource("registry/swiftui/content-minimal/NostrMinimalContentView.swift");
const contentMinimalPreviewSwift = nativeSource("registry/swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift");
const contentViewSwift = nativeSource("registry/swiftui/content-view/NostrContentView.swift");
const contentGroupingSwift = nativeSource("registry/swiftui/content-view/NostrContentGrouping.swift");
const contentHighlightsSwift = nativeSource("registry/swiftui/content-view/NostrContentHighlights.swift");
const contentAttributedSwift = nativeSource("registry/swiftui/content-view/NostrContentAttributed.swift");
const contentArticleViewSwift = nativeSource("registry/swiftui/content-view/NostrContentArticleView.swift");
const contentSelectableTextSwift = nativeSource("registry/swiftui/content-view/NostrSelectableText.swift");
const contentViewPreviewSwift = nativeSource("registry/swiftui/content-view/Examples/NostrContentViewPreview.swift");
const mentionChipSwift = nativeSource("registry/swiftui/content-mention-chip/NostrMentionChip.swift");
const mediaGridSwift = nativeSource("registry/swiftui/content-media-grid/NostrMediaGrid.swift");

// Content — Compose
const composeContentRendererKotlin = nativeSource("registry/compose/content-core/NostrContentRenderer.kt");
const composeContentTreeWireKotlin = nativeSource("registry/compose/content-core/ContentTreeWire.kt");
const composeContentViewKotlin = nativeSource("registry/compose/content-view/NostrContentView.kt");
const composeContentGroupingKotlin = nativeSource("registry/compose/content-view/NostrContentGrouping.kt");
const composeMentionChipKotlin = nativeSource("registry/compose/content-mention-chip/NostrMentionChip.kt");
const composeMediaGridKotlin = nativeSource("registry/compose/content-media-grid/NostrMediaGrid.kt");

// Content — Ratatui
const tuiContentTreeWireRust = nativeSource("registry/tui/content-core/content_tree_wire.rs");
const tuiContentRenderDataRust = nativeSource("registry/tui/content-core/content_render_data.rs");
const tuiTextWrapRust = nativeSource("registry/tui/content-core/ratatui_text_wrap.rs");
const tuiContentViewRust = nativeSource("registry/tui/content-view/nostr_content_view.rs");
const tuiContentWidgetRust = nativeSource("registry/tui/content-view/nostr_content_widget.rs");
const tuiMentionChipRust = nativeSource("registry/tui/content-mention-chip/nostr_mention_chip.rs");
const tuiMinimalContentRust = nativeSource("registry/tui/content-minimal/nostr_minimal_content.rs");
const tuiMediaGridRust = nativeSource("registry/tui/content-media-grid/nostr_media_grid.rs");

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
      web: webContentCore,
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
      web: webContentMinimal,
    },
  },
  {
    slug: "content-view",
    routeId: "content-view",
    version: "0.3.0",
    description:
      "Full ContentTreeWire renderer. Stitches text runs, mentions, kind-dispatched event-ref embeds, and media grids into one view. Article-reading surfaces opt into text selection → highlight-creation, NIP-84 range overlays, and footnote markers + scroll-to navigation.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-view",
        version: "0.3.0",
        dependencies: ["content-core", "content-media-grid", "content-kind-registry"],
        files: [{ source: "swiftui/content-view/NostrContentView.swift", target: "Components/NostrContent/NostrContentView.swift", role: "source", content: contentViewSwift }, { source: "swiftui/content-view/NostrContentGrouping.swift", target: "Components/NostrContent/NostrContentGrouping.swift", role: "source", content: contentGroupingSwift }, { source: "swiftui/content-view/NostrContentHighlights.swift", target: "Components/NostrContent/NostrContentHighlights.swift", role: "source", content: contentHighlightsSwift }, { source: "swiftui/content-view/NostrContentAttributed.swift", target: "Components/NostrContent/NostrContentAttributed.swift", role: "source", content: contentAttributedSwift }, { source: "swiftui/content-view/NostrContentArticleView.swift", target: "Components/NostrContent/NostrContentArticleView.swift", role: "source", content: contentArticleViewSwift }, { source: "swiftui/content-view/NostrSelectableText.swift", target: "Components/NostrContent/NostrSelectableText.swift", role: "source", content: contentSelectableTextSwift }, { source: "swiftui/content-view/Examples/NostrContentViewPreview.swift", target: "Components/NostrContent/Examples/NostrContentViewPreview.swift", role: "example", content: contentViewPreviewSwift }],
        screenshots: ["content-view-ios-gallery-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content`. Each tree node maps to a sub-component you installed alongside it.",
          "Event refs render through the kind-dispatch registry (`content-kind-registry`): bind `NostrProfileHost`, `EmbedEnvelopeSource`, `EventRefResolverProtocol`, and `NostrKindRegistry` once via `.nmpComponentHost(...)`.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-view",
        version: "0.2.0",
        dependencies: ["content-core", "content-media-grid", "content-kind-registry"],
        files: [{ source: "compose/content-view/NostrContentView.kt", target: "Components/NostrContent/NostrContentView.kt", role: "source", content: composeContentViewKotlin }, { source: "compose/content-view/NostrContentGrouping.kt", target: "Components/NostrContent/NostrContentGrouping.kt", role: "source", content: composeContentGroupingKotlin }],
        screenshots: ["content-view-kotlin-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` and dispatches each block-level group to the matching sub-component. Customizing usually means editing the sub-component rather than this dispatcher.",
          "Event refs render through the kind-dispatch registry (`content-kind-registry`): bind `LocalNostrProfileHost`, `LocalResolvedEventEmbeds`, `LocalEventRefResolver`, and `LocalNostrKindRegistry` once via `NmpComponentHostProvider(...)`.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-view",
        version: "0.2.0",
        dependencies: ["content-core", "content-kind-registry", "content-mention-chip", "content-media-grid"],
        files: [
          { source: "tui/content-view/nostr_content_view.rs", target: "src/components/nostr_content/nostr_content_view.rs", role: "source", content: tuiContentViewRust },
          { source: "tui/content-view/nostr_content_widget.rs", target: "src/components/nostr_content/nostr_content_widget.rs", role: "source", content: tuiContentWidgetRust },
        ],
        screenshots: ["tui-content-view-preview.png"],
        customization: [
          "`NostrContentView` dispatches each `ContentTreeWire` node to the matching Ratatui sub-widget and renders event refs through the kind-dispatch registry (`EmbeddedEvent`) when a host is wired.",
          "Host apps provide terminal image protocols for media URLs; the widget renders inline images when those protocols are present and falls back to text rows otherwise.",
        ],
      },
      web: webContentView,
    },
  },
  ...contentKindComponents,
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
      web: webContentMentionChip,
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
      web: webContentMediaGrid,
    },
  },
];
