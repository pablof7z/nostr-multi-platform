import { nativeSource } from "./vendorSource";
import type { Component } from "./types";

// Embeds & Kinds — SwiftUI
// embed-article and embed-highlight reuse the per-kind SwiftUI components from the
// content-kind-* vendor dirs; embed-profile and embed-note have no vendor files yet.
const swiftuiArticleEmbedSwift = nativeSource("registry/swiftui/content-kind-30023/ArticleEmbed.swift");
const swiftuiHighlightEmbedSwift = nativeSource("registry/swiftui/content-kind-9802/HighlightEmbed.swift");

// Embeds & Kinds — Compose (Android)
const composeArticleCardKotlin = nativeSource("registry/compose/content-kind-30023/NostrArticleCard.kt");
const composeHighlightCardKotlin = nativeSource("registry/compose/content-kind-9802/NostrHighlightCard.kt");

// Embeds & Kinds — Ratatui
// The article and highlight embeds are rendered by the default renderers that ship
// inline in the kind registry, identical to the content-kind-* TUI components.
const tuiKindRegistryRust = nativeSource("registry/tui/content-kind-registry/nostr_kind_registry.rs");

// Embeds & Kinds — Desktop (iced)
const desktopArticleCardRust = nativeSource("registry/desktop/content-kind-30023/embed_article.rs");

// Embeds & Kinds — Web (SolidJS). Like SwiftUI, the embeds reuse the per-kind
// content-* components: article → NostrArticleCard, highlight → NostrHighlightCard,
// note → NostrQuoteCard, profile → NostrMentionChip.
import webArticleCardTsx from "../../../packages/components-web/src/content-kind-30023/NostrArticleCard.tsx?raw";
import webHighlightCardTsx from "../../../packages/components-web/src/content-kind-9802/NostrHighlightCard.tsx?raw";
import webQuoteCardTsx from "../../../packages/components-web/src/content-quote-card/NostrQuoteCard.tsx?raw";
import webMentionChipTsx from "../../../packages/components-web/src/content-mention-chip/NostrMentionChip.tsx?raw";

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
          "Author byline resolves the display name reactively from the projection's raw `author_pubkey` via component-owned kind:0 claiming (display separation #2514) — `ArticleProjection` no longer carries a static `author_display_name`.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/content-kind-30023",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`ArticleCard::new(&ArticleProjection, author_name)` is the iced kind:30023 card. It renders the article title (bold), a byline (red dot + presentation-resolved author label + short date), and a summary snippet (first 300 chars), wrapped in a rounded bordered container. The byline uses an author label the displaying renderer resolved from a profile it claimed (component-owned claiming, mirroring iOS #833), not the projection's static `author_display_name`. Note: this card does NOT load the hero image yet — the `image` tag is rendered on iOS/Compose but the iced surface only shows title + byline + summary, with hero loading documented as a follow-on in `embed_article.rs`.",
        files: [
          { source: "desktop/content-kind-30023/embed_article.rs", target: "src/components/nostr_content/embed_article.rs", role: "source", content: desktopArticleCardRust },
        ],
        screenshots: ["embed-article-desktop-preview.png"],
        customization: [
          "The author label is passed in by the caller (`ArticleCard::new(article, author_name)`), so the host owns profile claiming and resolution; the card stays purely declarative.",
          "Hero image loading is a documented follow-on — add an `iced::widget::image` row above the title once the host decodes the article `image` tag into a `Handle`.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`<NostrArticleCard article={...} />` renders a resolved kind:30023 inline: a 16:9 hero image, the title headline, an optional summary, and an author byline (avatar + name). The host hydrates the model from the `ArticleProjection` inside derived `refs.event.envelopes`; per-kind dispatch lives in `content-kind-registry`'s `NostrEmbeddedEvent`. Verified live in the NMP web gallery against the real showcase article (author byline gated on the kernel resolving the author's kind:0 — never an unresolved 'unknown').",
        files: [
          { source: "web/content-kind-30023/NostrArticleCard.tsx", target: "src/components/nostr-content/NostrArticleCard.tsx", role: "source", content: webArticleCardTsx },
        ],
        screenshots: ["embed-article-web-preview.png"],
        customization: [
          "Swap the hero `<img>` for your own loader; the layout is plain HTML styled by `nostr-article-card__*` classes.",
          "Dispatch kind:30023 event refs to this card via `content-kind-registry`; other kinds fall back to the quote card.",
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
      web: {
        status: "stable",
        installId: "web/content-mention-chip",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        longDescription:
          "Web renders the inline npub mention as a `<NostrMentionChip profile={...} />` — an avatar + display-name chip resolved from the kernel profile projection (the same path the user-* components use). No embed claim is required for `npub:` URIs. Verified live in the NMP web gallery against a real resolved profile.",
        files: [
          { source: "web/content-mention-chip/NostrMentionChip.tsx", target: "src/components/nostr-content/NostrMentionChip.tsx", role: "source", content: webMentionChipTsx },
        ],
        screenshots: ["embed-profile-web-preview.png"],
        customization: [
          "The chip reuses `user-avatar`'s identicon + `displayLabel`, so its look matches the avatar/name components.",
        ],
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
      web: {
        status: "stable",
        installId: "web/content-quote-card",
        version: "0.1.0",
        dependencies: ["content-quote-card"],
        longDescription:
          "Web resolves the referenced `nevent` and renders the resolved kind:1 note as a `<NostrQuoteCard quote={...} />` — author header (avatar + name + relative time) above the content preview. The host hydrates the model from the `ShortNoteProjection` inside derived `refs.event.envelopes`; the card only renders. Verified live in the NMP web gallery against the real showcase note.",
        files: [
          { source: "web/content-quote-card/NostrQuoteCard.tsx", target: "src/components/nostr-content/NostrQuoteCard.tsx", role: "source", content: webQuoteCardTsx },
        ],
        screenshots: ["embed-note-web-preview.png"],
        customization: [
          "Pass `nowSeconds` from your app clock so the relative-time label stays pure.",
        ],
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
        installId: "compose/content-kind-9802",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`NostrHighlightCardRenderer` is the Compose NIP-84 card for kind-dispatched embeds. Install via `registry.setHighlight(NostrHighlightCardRenderer)` to render the highlight as a yellow-accented pull-quote with optional context, source footer (`r` → `e` → `a`), and highlighted-by byline. The model is hydrated from Rust's typed `HighlightProjection`; Kotlin does not parse tags.",
        files: [
          { source: "compose/content-kind-9802/NostrHighlightCard.kt", target: "Components/NostrContent/NostrHighlightCard.kt", role: "source", content: composeHighlightCardKotlin },
        ],
        screenshots: ["embed-highlight-kotlin-preview.png"],
        customization: [
          "Register `NostrHighlightCardRenderer` on your app's `NostrKindRegistry`; `EmbeddedEvent` continues to own resolve/release and dispatch.",
          "Use `NostrHighlightCard(model = ...)` directly when a screen already has a resolved `HighlightProjection` and wants a standalone card.",
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
          "`<NostrHighlightCard highlight={...} />` renders a resolved kind:9802 highlight: the highlighted text as a pull-quote in a yellow-accented box, an optional `context` line, and a source footer from the resolved projection fields. The host hydrates the model from the `HighlightProjection` inside derived `refs.event.envelopes`; dispatch lives in `content-kind-registry`. Verified live in the NMP web gallery against the real showcase highlight. Mirrors the SwiftUI/TUI `HighlightEmbed`.",
        files: [
          { source: "web/content-kind-9802/NostrHighlightCard.tsx", target: "src/components/nostr-content/NostrHighlightCard.tsx", role: "source", content: webHighlightCardTsx },
        ],
        screenshots: ["embed-highlight-web-preview.png"],
        customization: [
          "Tweak the accent colour via the `nostr-highlight-card` border/background literals.",
          "Extend the source footer to render a rich preview when the `e`-tag source event has been claimed.",
        ],
      },
    },
  },
];
