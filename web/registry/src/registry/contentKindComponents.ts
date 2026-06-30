import { nativeSource } from "./vendorSource";
import type { Component } from "./types";
import { webContentKind30023, webContentKind9802, webContentKindRegistry } from "./contentWeb";

const tuiKindRegistryModRust = nativeSource("registry/tui/content-kind-registry/mod.rs");
const tuiKindRendererRust = nativeSource("registry/tui/content-kind-registry/kind_renderer.rs");
const tuiKindRegistryRust = nativeSource("registry/tui/content-kind-registry/nostr_kind_registry.rs");
const tuiEmbedChromeRust = nativeSource("registry/tui/content-kind-registry/embed_chrome_container.rs");
const tuiEmbeddedEventRust = nativeSource("registry/tui/content-kind-registry/embedded_event.rs");

const swiftuiEmbedKindProjectionSwift = nativeSource("registry/swiftui/content-kind-registry/EmbedKindProjection.swift");
const swiftuiEmbedChromeContainerSwift = nativeSource("registry/swiftui/content-kind-registry/EmbedChromeContainer.swift");
const swiftuiNostrKindRegistrySwift = nativeSource("registry/swiftui/content-kind-registry/NostrKindRegistry.swift");
const swiftuiEmbeddedEventSwift = nativeSource("registry/swiftui/content-kind-registry/EmbeddedEvent.swift");
const swiftuiEmbedHostEnvironmentSwift = nativeSource("registry/swiftui/content-kind-registry/EmbedHostEnvironment.swift");
const composeEmbedKindProjectionKotlin = nativeSource("registry/compose/content-kind-registry/EmbedKindProjection.kt");
const composeEmbedChromeContainerKotlin = nativeSource("registry/compose/content-kind-registry/EmbedChromeContainer.kt");
const composeNostrKindRegistryKotlin = nativeSource("registry/compose/content-kind-registry/NostrKindRegistry.kt");
const composeEmbeddedEventKotlin = nativeSource("registry/compose/content-kind-registry/EmbeddedEvent.kt");
const swiftuiProfileEmbedSwift = nativeSource("registry/swiftui/content-kind-0/ProfileEmbed.swift");
const swiftuiArticleEmbedSwift = nativeSource("registry/swiftui/content-kind-30023/ArticleEmbed.swift");
const composeProfileCardKotlin = nativeSource("registry/compose/content-kind-0/NostrProfileCard.kt");
const composeArticleCardKotlin = nativeSource("registry/compose/content-kind-30023/NostrArticleCard.kt");
const composeHighlightCardKotlin = nativeSource("registry/compose/content-kind-9802/NostrHighlightCard.kt");
const desktopProfileCardRust = nativeSource("registry/desktop/content-kind-0/profile_card.rs");
const desktopArticleCardRust = nativeSource("registry/desktop/content-kind-30023/embed_article.rs");
const swiftuiHighlightEmbedSwift = nativeSource("registry/swiftui/content-kind-9802/HighlightEmbed.swift");

export const contentKindComponents: Component[] = [
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
          "Swift mirror of `tui/content-kind-registry`. `NostrKindRegistry` is a SwiftUI-friendly @MainActor dispatch table mapping `EmbedKindProjection` variants to `KindRenderer` implementations. `EmbeddedEvent` owns the resolve/release lifecycle (via `.task(id:)` + `.onDisappear`), reads the resolved envelope from the app-level host/provider over Rust-derived `refs.event.envelopes`, and dispatches through the registry. `EmbedChromeContainer` mirrors the TUI's depth-graded accent stripe so nested embeds visually scale identically across platforms.",
        files: [
          { source: "swiftui/content-kind-registry/EmbedKindProjection.swift", target: "Components/NostrContent/EmbedKindProjection.swift", role: "source", content: swiftuiEmbedKindProjectionSwift },
          { source: "swiftui/content-kind-registry/EmbedHostEnvironment.swift", target: "Components/NostrContent/EmbedHostEnvironment.swift", role: "source", content: swiftuiEmbedHostEnvironmentSwift },
          { source: "swiftui/content-kind-registry/EmbedChromeContainer.swift", target: "Components/NostrContent/EmbedChromeContainer.swift", role: "source", content: swiftuiEmbedChromeContainerSwift },
          { source: "swiftui/content-kind-registry/NostrKindRegistry.swift", target: "Components/NostrContent/NostrKindRegistry.swift", role: "source", content: swiftuiNostrKindRegistrySwift },
          { source: "swiftui/content-kind-registry/EmbeddedEvent.swift", target: "Components/NostrContent/EmbeddedEvent.swift", role: "source", content: swiftuiEmbeddedEventSwift },
        ],
        screenshots: ["embed-article-ios-gallery-preview.png"],
        customization: [
          "Build the registry once at app start with `NostrKindRegistry.makeDefault()` then `registry.setArticle(ArticleEmbed())` / `registry.setHighlight(HighlightEmbed())` to swap in richer per-kind components.",
          "Bind the app-level host/provider once with `.nmpComponentHost(profileHost: embedSource: eventRefResolver: kindRegistry:)` — `NostrContentView` and `EmbeddedEvent` both read the registry, resolver, and Rust-derived envelope mirror from there.",
          "Conform your envelope holder to `EmbedEnvelopeSource` over the `refs.event.envelopes` sidecar and implement `EventRefResolverProtocol` against your kernel FFI; the embed view owns lifecycle, not protocol parsing.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-kind-registry",
        version: "0.1.0",
        dependencies: ["content-core", "user-avatar"],
        longDescription:
          "Compose mirror of `tui/content-kind-registry`. `NostrKindRegistry` is a dispatch table mapping `EmbedKindProjection` variants to `KindRenderer` composables. `EmbeddedEvent` owns the resolve/release lifecycle (via `DisposableEffect`), reads the Rust-derived `refs.event.envelopes` mirror from `LocalResolvedEventEmbeds`, and dispatches through `LocalNostrKindRegistry`. `EmbedChromeContainer` mirrors the depth-graded accent stripe so nested embeds scale identically across platforms.",
        files: [
          { source: "compose/content-kind-registry/EmbedKindProjection.kt", target: "Components/NostrContent/EmbedKindProjection.kt", role: "source", content: composeEmbedKindProjectionKotlin },
          { source: "compose/content-kind-registry/EmbedChromeContainer.kt", target: "Components/NostrContent/EmbedChromeContainer.kt", role: "source", content: composeEmbedChromeContainerKotlin },
          { source: "compose/content-kind-registry/NostrKindRegistry.kt", target: "Components/NostrContent/NostrKindRegistry.kt", role: "source", content: composeNostrKindRegistryKotlin },
          { source: "compose/content-kind-registry/EmbeddedEvent.kt", target: "Components/NostrContent/EmbeddedEvent.kt", role: "source", content: composeEmbeddedEventKotlin },
        ],
        screenshots: ["embed-article-kotlin-preview.png"],
        customization: [
          "Build the registry once with `NostrKindRegistry.makeDefault()` then `setArticle(...)` / `setHighlight(...)` to swap in richer per-kind composables.",
          "Wrap your app root with `NmpComponentHostProvider(...)` so `LocalNostrKindRegistry`, `LocalResolvedEventEmbeds`, and `LocalEventRefResolver` are bound once; `LocalResolvedEventEmbeds` mirrors derived `refs.event.envelopes`, not authoritative event rows.",
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
      web: webContentKindRegistry,
    },
  },
  {
    slug: "content-kind-0",
    routeId: "content-kind-0",
    version: "0.1.0",
    description: "Profile metadata (NIP-01, kind:0) embed renderer — avatar, display name, pubkey chip, NIP-05, and about preview.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-kind-0",
        version: "0.1.0",
        dependencies: ["content-kind-registry", "user-avatar"],
        longDescription:
          "`ProfileEmbed` is the SwiftUI kind:0 profile renderer. Install it with `registry.setProfile(ProfileEmbed())`; Rust still owns kind:0 parsing and ships a typed `ProfileProjection`, while the component formats the raw pubkey/profile fields into an avatar row, NIP-05 line, pubkey chip, and about preview.",
        files: [
          { source: "swiftui/content-kind-0/ProfileEmbed.swift", target: "Components/NostrContent/ProfileEmbed.swift", role: "source", content: swiftuiProfileEmbedSwift },
        ],
        screenshots: ["embed-profile-ios-gallery-preview.png"],
        customization: [
          "Swap the `NostrAvatar` loader by editing your installed `user-avatar` component; `ProfileEmbed` only composes it.",
          "Keep the projection raw: format pubkeys and copy/share affordances in the presentation layer, not in Rust snapshot builders.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-kind-0",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`NostrProfileCard` is the Compose kind:0 renderer. The host hydrates `NostrProfileCardModel` from Rust's typed `ProfileProjection`; the component renders avatar/identicon, display name, NIP-05, pubkey chip, and about text without parsing kind:0 JSON.",
        files: [
          { source: "compose/content-kind-0/NostrProfileCard.kt", target: "Components/NostrContent/NostrProfileCard.kt", role: "source", content: composeProfileCardKotlin },
        ],
        screenshots: ["embed-profile-kotlin-preview.png"],
        customization: [
          "Replace Coil `SubcomposeAsyncImage` with your app's image pipeline; the fallback identicon comes from `content-core`.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-kind-0",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`DefaultProfileRenderer` is the built-in kind:0 profile renderer shipped with `tui/content-kind-registry`. It renders the display label, Rust-formatted pubkey fallback, and about preview from the already-resolved `ProfileProjection`.",
        files: [
          { source: "tui/content-kind-registry/nostr_kind_registry.rs", target: "src/components/nostr_content/content_kind_registry/nostr_kind_registry.rs", role: "source", content: tuiKindRegistryRust },
        ],
        screenshots: ["tui-embed-profile-preview.png"],
        customization: [
          "Replace `DefaultProfileRenderer` by registering your own `KindRenderer` for `ProfileProjection`; the default lives inline in `nostr_kind_registry.rs`.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/content-kind-0",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`ProfileCard::new(&ProfileProjection)` is the iced kind:0 card. It renders a display label, Rust-formatted npub fallback, optional NIP-05, and about preview inside a bordered container. The caller supplies the already-resolved projection; the card performs no kind:0 parsing.",
        files: [
          { source: "desktop/content-kind-0/profile_card.rs", target: "src/components/nostr_content/profile_card.rs", role: "source", content: desktopProfileCardRust },
        ],
        screenshots: [],
        customization: [
          "Add an avatar image row beside the label when your desktop host has a cached image handle; keep loading in update code, not in view construction.",
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
          "`ArticleEmbed` is the canonical NIP-23 card. Install via `registry.setArticle(ArticleEmbed())` on a `NostrKindRegistry`. Renders the article's `image` tag as a 16:9 hero, `title` as the headline, optional `summary` line, then an author byline with `NostrAvatar` + display name. The host's `EmbedHost` reads the `ArticleProjection` inside Rust-derived `refs.event.envelopes`; Swift never parses the raw event row.",
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
        dependencies: ["content-kind-registry"],
        longDescription:
          "`NostrArticleCard` is the canonical NIP-23 Compose card. Per-kind dispatch lives in `NostrContentView.EventRefBlock` via an `articleCardProvider`, so the card stays self-contained on `compose/content-core` — no separate kind-registry component. Renders the article hero image as a 16:9 hero (Coil `SubcomposeAsyncImage` with a placeholder fallback), the title as a semibold headline, an optional summary line, then an author byline with an avatar (`NostrIdenticon` fallback) + display name + `article · kind:30023` tag. The app hydrates a `NostrArticleCardModel` from the `ArticleProjection` inside derived `refs.event.envelopes`; the card only renders.",
        files: [
          { source: "compose/content-kind-30023/NostrArticleCard.kt", target: "Components/NostrContent/NostrArticleCard.kt", role: "source", content: composeArticleCardKotlin },
        ],
        screenshots: ["embed-article-kotlin-preview.png"],
        customization: [
          "Replace the hero `SubcomposeAsyncImage` with Glide or a custom `Painter` — the rest of the layout stays untouched.",
          "Wire per-kind dispatch by passing an `articleCardProvider` to `NostrContentView.EventRefBlock`; tap routes through `NostrContentCallbacks.onEventRefTap` unless you supply your own `onTap`.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/content-kind-30023",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`DefaultArticleRenderer` is the built-in NIP-23 long-form article renderer shipped with `tui/content-kind-registry`. It lays out an optional hero image (terminal image protocol when present, ASCII fallback otherwise), the article title styled as a heading, a summary paragraph, and an author byline that resolves the kind:0 display name reactively from the projection's raw `author_pubkey` (component-owned claiming; the projection no longer carries a static display name). Registered automatically on `NostrKindRegistry::with_defaults()`; swap it out per-app via `registry.set_article(Arc::new(MyArticleRenderer))`.",
        files: [
          { source: "tui/content-kind-registry/nostr_kind_registry.rs", target: "src/components/nostr_content/content_kind_registry/nostr_kind_registry.rs", role: "source", content: tuiKindRegistryRust },
        ],
        screenshots: ["tui-embed-article.png"],
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
          "iced `ArticleCard::new(&ArticleProjection, author_name)` renders the kind:30023 article inline: title (bold), a byline (red dot + host-resolved author label + short date), and a summary snippet, in a rounded bordered container. The author label is resolved by the displaying renderer (component-owned claiming), not the projection's static field. NOTE: the iced surface does NOT load the hero image yet — title + byline + summary only; hero loading is a documented follow-on in `embed_article.rs`.",
        files: [
          { source: "desktop/content-kind-30023/embed_article.rs", target: "src/components/nostr_content/embed_article.rs", role: "source", content: desktopArticleCardRust },
        ],
        screenshots: ["embed-article-desktop-preview.png"],
        customization: [
          "Hero image loading is a documented follow-on — add an `iced::widget::image` row above the title once the host decodes the article `image` tag into a `Handle`.",
        ],
      },
      web: webContentKind30023,
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
      compose: {
        status: "stable",
        installId: "compose/content-kind-9802",
        version: "0.1.0",
        dependencies: ["content-kind-registry"],
        longDescription:
          "`NostrHighlightCardRenderer` is the Compose NIP-84 renderer. Install it with `registry.setHighlight(NostrHighlightCardRenderer)` to replace the default highlight handler with a richer card: yellow-accent pull-quote, optional `context` line, `r`/`e`/`a` source footer, and highlighted-by byline. The component maps Rust's typed `HighlightProjection` into `NostrHighlightCardModel`; it never parses tags in Kotlin.",
        files: [
          { source: "compose/content-kind-9802/NostrHighlightCard.kt", target: "Components/NostrContent/NostrHighlightCard.kt", role: "source", content: composeHighlightCardKotlin },
        ],
        screenshots: ["embed-highlight-kotlin-preview.png"],
        customization: [
          "Swap the yellow accent by editing the local `accent` color in `PullQuote`; no registry format changes are needed.",
          "Use `NostrHighlightCard(model = ...)` directly for custom card surfaces, or keep `NostrHighlightCardRenderer` registered for kind-dispatched embeds.",
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
          "Replace `DefaultHighlightRenderer` by registering your own `KindRenderer` for `HighlightProjection`; the default lives inline in `nostr_kind_registry.rs` for easy copy-paste editing.",
          "The source footer branches on `source_url` → `source_event_id` → `source_event_addr` in priority order; extend the match arms to render richer previews when the referenced event has been claimed.",
        ],
      },
      web: webContentKind9802,
    },
  },
];
