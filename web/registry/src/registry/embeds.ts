import type { Component } from "./types";

// Embeds & Kinds — SwiftUI
// embed-article and embed-highlight reuse the per-kind SwiftUI components from the
// content-kind-* vendor dirs; embed-profile and embed-note have no vendor files yet.
import swiftuiArticleEmbedSwift from "../vendor/swiftui/content-kind-30023/ArticleEmbed.swift?raw";
import swiftuiHighlightEmbedSwift from "../vendor/swiftui/content-kind-9802/HighlightEmbed.swift?raw";

// Embeds & Kinds — Compose (Android)
import composeArticleCardKotlin from "../vendor/compose/content-kind-30023/NostrArticleCard.kt?raw";

// Embeds & Kinds — Ratatui
// The article and highlight embeds are rendered by the default renderers that ship
// inline in the kind registry, identical to the content-kind-* TUI components.
import tuiKindRegistryRust from "../vendor/tui/content-kind-registry/nostr_kind_registry.rs?raw";

export const embedComponents: Component[] = [
  {
    slug: "embed-article",
    routeId: "embed-article",
    version: "0.1.0",
    description: "Kind:30023 long-form article — hero image, title, summary",
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
          "`NostrArticleCard` is the Compose NIP-23 card — a 16:9 Coil hero, the `title` headline, an optional `summary`, and an author byline (avatar + display name + `article \u00b7 kind:30023`). `NostrContentView`'s `EventRefBlock` dispatches kind:30023 event refs to it via an `articleCardProvider`, so the article renders inline within the surrounding note text.",
        files: [
          { source: "compose/content-kind-30023/NostrArticleCard.kt", target: "Components/NostrContent/NostrArticleCard.kt", role: "source", content: composeArticleCardKotlin },
        ],
        screenshots: ["embed-article-kotlin-preview.png"],
        customization: [
          "Swap the Coil `SubcomposeAsyncImage` hero loader for your app's image pipeline; the layout is unchanged.",
          "Register the typed card by passing `articleCardProvider` to `NostrContentView`; other kinds fall back to the quote card.",
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
        screenshots: ["tui-embed-article.png", "tui-embed-article-preview.png"],
        customization: [
          "Replace `DefaultArticleRenderer` by registering your own `KindRenderer` for `ArticleProjection` — the default lives inline in `nostr_kind_registry.rs` for easy copy-paste editing.",
          "Author byline pulls `author_display_name` straight from `ArticleProjection`; the Rust kernel resolves kind:0 enrichment before the snapshot reaches the TUI.",
        ],
      },
    },
  },
  {
    slug: "embed-profile",
    routeId: "embed-profile",
    version: "0.1.0",
    description: "Inline npub mention chip — kind:0 profile",
    platforms: {
      swiftui: {
        status: "soon",
        installId: "swiftui/embed-profile",
        version: "0.1.0",
        dependencies: ["content-kind-registry", "user-avatar"],
        longDescription:
          "Inline kind:0 profile embed — renders an npub mention as an avatar + display-name chip resolved from the kernel profile projection.",
        files: [],
        screenshots: ["embed-profile-ios-gallery-preview.png", "tui-embed-profile-preview.png"],
        customization: [],
      },
      compose: {
        status: "stable",
        installId: "compose/content-mention-chip",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "Android renders the inline npub mention through `NostrContentView` — the kind:0 profile resolves to an avatar + display-name chip from the kernel profile projection (the same path the user-* components use). No embed claim is required for `npub:` URIs.",
        files: [],
        screenshots: ["embed-profile-kotlin-preview.png"],
        customization: [],
      },
    },
  },
  {
    slug: "embed-note",
    routeId: "embed-note",
    version: "0.1.0",
    description: "Kind:1 short text note via nevent claim",
    platforms: {
      swiftui: {
        status: "soon",
        installId: "swiftui/embed-note",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "Kind:1 short text note embed — claims the referenced `nevent` and renders the resolved note inline through the kind registry.",
        files: [],
        screenshots: ["embed-note-ios-gallery-preview.png", "tui-embed-note-preview.png"],
        customization: [],
      },
      compose: {
        status: "stable",
        installId: "compose/content-view",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "Android claims the referenced `nevent` and renders the resolved kind:1 note inline through `NostrContentView` — author + content paint between the surrounding prose, with a formatted relative timestamp.",
        files: [],
        screenshots: ["embed-note-kotlin-preview.png"],
        customization: [],
      },
    },
  },
  {
    slug: "embed-highlight",
    routeId: "embed-highlight",
    version: "0.1.0",
    description: "Kind:9802 highlight — pull-quote + source",
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
      compose: {
        status: "stable",
        installId: "compose/content-view",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "Android resolves the kind:9802 highlight and renders it inline via `NostrContentView`'s generic quote card (pull-quote text + author + relative time). A typed Compose highlight renderer (matching the SwiftUI/TUI `HighlightEmbed`) is not built yet — `EventRefBlock` only dispatches kind:30023 to a typed card today.",
        files: [],
        screenshots: ["embed-highlight-kotlin-preview.png"],
        customization: [],
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
    },
  },
];
