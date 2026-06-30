import type { PlatformImpl } from "./types";

import webArticleCardTsx from "../../../packages/components-web/src/content-kind-30023/NostrArticleCard.tsx?raw";
import webHighlightCardTsx from "../../../packages/components-web/src/content-kind-9802/NostrHighlightCard.tsx?raw";
import webKindRegistryTsx from "../../../packages/components-web/src/content-kind-registry/NostrKindRegistry.tsx?raw";
import webEventRefResolverTsx from "../../../packages/components-web/src/component-host/EventRefResolver.tsx?raw";
import webComponentHostProviderTsx from "../../../packages/components-web/src/component-host/NmpComponentHostProvider.tsx?raw";
import webResolvedEventEmbedsTsx from "../../../packages/components-web/src/component-host/ResolvedEventEmbeds.tsx?raw";
import webContentCoreTs from "../../../packages/components-web/src/content-core/decodeContentTree.ts?raw";
import webMediaGridTsx from "../../../packages/components-web/src/content-media-grid/NostrMediaGrid.tsx?raw";
import webMentionChipTsx from "../../../packages/components-web/src/content-mention-chip/NostrMentionChip.tsx?raw";
import webContentMinimalTsx from "../../../packages/components-web/src/content-minimal/NostrMinimalContentView.tsx?raw";
import webQuoteCardTsx from "../../../packages/components-web/src/content-quote-card/NostrQuoteCard.tsx?raw";
import webContentViewTsx from "../../../packages/components-web/src/content-view/NostrContentView.tsx?raw";

export const webContentCore: PlatformImpl = {
  status: "stable",
  installId: "web/content-core",
  version: "0.1.0",
  dependencies: [],
  longDescription:
    "`decodeContentTree(bytes)` is the one place the web stack turns the kernel's NFCT bytes (`refs.event` row payloads / feed projections) into a decoded `ContentTreeWire` — every web content component (content-view, content-minimal) consumes the decoded tree, never raw bytes. Re-exports the generated `ContentTreeWire` + `WireNodeKind` so consumers import the tree type and the decoder from one module, mirroring the native content-core wire-type + renderer split. Also ships `isTreeRenderable` — the honesty gate (non-empty AND placeholder-free) the gallery uses to refuse the raw-string fallback. Pure; never fetches or mocks.",
  files: [
    { source: "web/content-core/decodeContentTree.ts", target: "src/components/nostr-content/decodeContentTree.ts", role: "source", content: webContentCoreTs },
  ],
  screenshots: ["content-core-web-preview.png"],
  customization: [
    "`ContentTreeWire` is the generated FlatBuffers binding from the `nmp-content` schema — regenerate it with the web flatc pin when the schema changes; this module only adds the decode + honesty-gate helpers around it.",
    "Decode NFCT bytes once in your runtime and pass the decoded tree to the content components; they are pure walkers and never decode bytes themselves.",
  ],
};

export const webContentMinimal: PlatformImpl = {
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
};

export const webContentView: PlatformImpl = {
  status: "stable",
  installId: "web/content-view",
  version: "0.1.0",
  dependencies: ["content-kind-registry"],
  longDescription:
    "`<NostrContentView tree={...} fallback={...} />` is a SolidJS component that walks a kernel-decoded `ContentTreeWire` (NFCT) — the `nmp-content` tokenizer running behind the kernel's content-parser seam — into HTML: paragraphs, headings, lists, blockquotes, code, inline emphasis/strong/links, hashtags, URLs, emoji, media, and Nostr references. Mention refs render as raw `nostr:` anchors. Event refs render through the app-provided `NmpComponentHostProvider` when the host has a resolved `refs.event.envelopes` entry, and fall back to raw links when the host or embed is missing. It never parses, fetches, or mocks; when no tree is present it renders the raw `fallback` string verbatim (honest-empty per D6). Verified live in the NMP web gallery: a real kind:1 note and a real kind:30023 long-form article, both claimed from real relays and parsed by the real WASM kernel.",
  files: [
    { source: "web/content-view/NostrContentView.tsx", target: "src/components/nostr-content/NostrContentView.tsx", role: "source", content: webContentViewTsx },
  ],
  screenshots: ["content-view-web-preview.png"],
  customization: [
    "Style via the `nostr-*` element classes (`nostr-p`, `nostr-h`, `nostr-url`, `nostr-blockquote`, `nostr-code-block`, …); the component emits semantic HTML with no inline styles.",
    "The `tree` prop is a decoded `ContentTreeWire` from your kernel snapshot's `refs.event` row payload or feed projection; decode the NFCT bytes once in your runtime and pass the root object — the component is a pure walker.",
    "Bind `NmpComponentHostProvider` at the app root to upgrade event-ref nodes to resolved cards. The provider consumes your app's `refs.event.envelopes` mirror and event-ref resolver; `NostrContentView` still renders raw links while an embed is unresolved.",
  ],
};

export const webContentKindRegistry: PlatformImpl = {
  status: "stable",
  installId: "web/content-kind-registry",
  version: "0.1.0",
  dependencies: ["content-kind-30023", "content-kind-9802", "content-quote-card", "user-avatar"],
  longDescription:
    "`<NmpComponentHostProvider ...>` is the web app-root host for `@nmp/components-web`: it binds `NostrProfileHost`, the decoded `refs.event.envelopes` map, an app-owned event-ref resolver, and the `NostrKindRegistry` context once. `<NostrEmbeddedEvent event={...} />` is the web kind-dispatch table underneath that host. The host passes fully resolved `EmbeddedEventModel` values decoded from the Rust-generated `refs.event.envelopes` / NEMB sidecar; that sidecar is composed from authoritative `refs.event` rows by `nmp-content`. The registry routes on `projection.variant`: `article` -> `NostrArticleCard`, `highlight` -> `NostrHighlightCard`, everything else -> `NostrQuoteCard`. It maps the pre-resolved projection fields into each card's model and does not re-parse raw NIP-23/NIP-84 tags. Verified live in the NMP web gallery dispatching a real article, highlight, and note. The web twin of the SwiftUI/TUI `NostrKindRegistry` + `EmbeddedEvent`.",
  files: [
    { source: "web/component-host/EventRefResolver.tsx", target: "src/components/nostr-content/EventRefResolver.tsx", role: "source", content: webEventRefResolverTsx },
    { source: "web/component-host/ResolvedEventEmbeds.tsx", target: "src/components/nostr-content/ResolvedEventEmbeds.tsx", role: "source", content: webResolvedEventEmbedsTsx },
    { source: "web/component-host/NmpComponentHostProvider.tsx", target: "src/components/nostr-content/NmpComponentHostProvider.tsx", role: "source", content: webComponentHostProviderTsx },
    { source: "web/content-kind-registry/NostrKindRegistry.tsx", target: "src/components/nostr-content/NostrKindRegistry.tsx", role: "source", content: webKindRegistryTsx },
  ],
  screenshots: ["content-kind-registry-web-preview.png"],
  customization: [
    "Bind the provider once at the app root: `profileHost` reads `refs.profile`, `resolvedEventEmbeds` mirrors derived `refs.event.envelopes`, `eventRefResolver` forwards visible event refs to `resolve_ref(Event, Embed, CacheOk)`, and `kindRegistry` is optional when the default handlers are enough.",
    "Add a kind by registering the variant in the Rust resolver (`nmp-content`) and extending the registry renderer to map the new `projection.data` fields into a card model — the cards themselves stay pure model renderers and the web never parses raw tags.",
    "`refs.event` remains the event-ref source of truth. `refs.event.envelopes` is render data derived from it by `nmp-content`; do not populate the embed map from raw event JSON or legacy whole-map claims.",
  ],
};

export const webContentKind30023: PlatformImpl = {
  status: "stable",
  installId: "web/content-kind-30023",
  version: "0.1.0",
  dependencies: ["content-kind-registry"],
  longDescription:
    "`<NostrArticleCard article={...} />` is the web NIP-23 long-form card. Pure renderer: the host hydrates a `NostrArticleCardModel` from the `ArticleProjection` inside derived `refs.event.envelopes` (the `image`/`title`/`summary` fields + the kernel-enriched author). Renders the image as a 16:9 hero, the title headline, an optional summary, then an author byline (avatar + name + `article · kind:30023`). Verified live in the NMP web gallery against the real showcase article. Mirrors the SwiftUI `ArticleEmbed` / Compose `NostrArticleCard`.",
  files: [
    { source: "web/content-kind-30023/NostrArticleCard.tsx", target: "src/components/nostr-content/NostrArticleCard.tsx", role: "source", content: webArticleCardTsx },
  ],
  screenshots: ["content-kind-30023-web-preview.png"],
  customization: [
    "Swap the hero `<img>` for your own lazy/loader component; the rest of the layout is plain semantic HTML styled by `nostr-article-card__*` classes.",
    "The host projects the article tags into the model (see `content-kind-registry`); the card itself never reads tags.",
  ],
};

export const webContentKind9802: PlatformImpl = {
  status: "stable",
  installId: "web/content-kind-9802",
  version: "0.1.0",
  dependencies: ["content-kind-registry"],
  longDescription:
    "`<NostrHighlightCard highlight={...} />` is the web NIP-84 highlight card. Pure renderer: the host hydrates a `NostrHighlightCardModel` from the `HighlightProjection` inside derived `refs.event.envelopes`. Renders the highlighted text as a pull-quote in a yellow-accented box, an optional `context` line, and a source footer from the resolved projection fields. Verified live in the NMP web gallery against the real showcase highlight. Mirrors the SwiftUI/TUI `HighlightEmbed`.",
  files: [
    { source: "web/content-kind-9802/NostrHighlightCard.tsx", target: "src/components/nostr-content/NostrHighlightCard.tsx", role: "source", content: webHighlightCardTsx },
  ],
  screenshots: ["content-kind-9802-web-preview.png"],
  customization: [
    "Tweak the accent colour by editing the `nostr-highlight-card` left-border / background literals in your stylesheet.",
    "Extend `sourceLabel` to render a rich preview when the `e`-tag source event has already been claimed.",
  ],
};

export const webContentMentionChip: PlatformImpl = {
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
};

export const webContentQuoteCard: PlatformImpl = {
  status: "stable",
  installId: "web/content-quote-card",
  version: "0.1.0",
  dependencies: [],
  longDescription:
    "`<NostrQuoteCard quote={...} nowSeconds={...} />` is the web quoted-note card (the kind:1 `embed-note` body). Pure renderer: the host hydrates a `NostrQuoteCardModel` from the `ShortNoteProjection` inside derived `refs.event.envelopes`. Renders an author header (avatar + name + relative time) above a content preview, in a subtle bordered card. Ships a pure `relativeTime(createdAt, now)` helper (now injected for testability). Verified live in the NMP web gallery against the real showcase note. Mirrors the SwiftUI/Compose `NostrQuoteCard`.",
  files: [
    { source: "web/content-quote-card/NostrQuoteCard.tsx", target: "src/components/nostr-content/NostrQuoteCard.tsx", role: "source", content: webQuoteCardTsx },
  ],
  screenshots: ["content-quote-card-web-preview.png"],
  customization: [
    "Pass `nowSeconds` from your app clock so the relative-time label stays pure/testable rather than reading `Date.now()` inside render.",
    "Style the card via the `nostr-quote-card__*` classes; the component emits plain semantic HTML.",
  ],
};

export const webContentMediaGrid: PlatformImpl = {
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
};
