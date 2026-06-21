import type { PlatformImpl } from "./types";

import webArticleCardTsx from "@nmp/components-web/src/content-kind-30023/NostrArticleCard.tsx?raw";
import webHighlightCardTsx from "@nmp/components-web/src/content-kind-9802/NostrHighlightCard.tsx?raw";
import webKindRegistryTsx from "@nmp/components-web/src/content-kind-registry/NostrKindRegistry.tsx?raw";
import webContentCoreTs from "@nmp/components-web/src/content-core/decodeContentTree.ts?raw";
import webMediaGridTsx from "@nmp/components-web/src/content-media-grid/NostrMediaGrid.tsx?raw";
import webMentionChipTsx from "@nmp/components-web/src/content-mention-chip/NostrMentionChip.tsx?raw";
import webContentMinimalTsx from "@nmp/components-web/src/content-minimal/NostrMinimalContentView.tsx?raw";
import webQuoteCardTsx from "@nmp/components-web/src/content-quote-card/NostrQuoteCard.tsx?raw";
import webContentViewTsx from "@nmp/components-web/src/content-view/NostrContentView.tsx?raw";

export const webContentCore: PlatformImpl = {
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
};

export const webContentKindRegistry: PlatformImpl = {
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
};

export const webContentKind30023: PlatformImpl = {
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
};

export const webContentKind9802: PlatformImpl = {
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
    "`<NostrQuoteCard quote={...} nowSeconds={...} />` is the web quoted-note card (the kind:1 `embed-note` body). Pure renderer: the host hydrates a `NostrQuoteCardModel` from a resolved `claimed_events` entry (the referenced `nevent` claimed + kind:0-enriched). Renders an author header (avatar + name + relative time) above a content preview, in a subtle bordered card. Ships a pure `relativeTime(createdAt, now)` helper (now injected for testability). Verified live in the NMP web gallery against the real showcase note. Mirrors the SwiftUI/Compose `NostrQuoteCard`.",
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
