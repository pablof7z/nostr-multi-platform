/*
 * Static component manifest for the registry showcase site.
 *
 * Install-critical metadata is mirrored from the CLI manifest at
 * `crates/nmp-cli/registry/registry.toml`. The nmp-cli integration tests
 * compare ids, versions, targets, dependencies, and file mappings so this
 * showcase cannot drift from the offline registry apps actually install.
 */

import contentCoreSwift from "../../../crates/nmp-cli/registry/swiftui/content-core/NostrContentRenderer.swift?raw";
import contentCoreWireSwift from "../../../crates/nmp-cli/registry/swiftui/content-core/ContentTreeWire.swift?raw";
import contentMinimalSwift from "../../../crates/nmp-cli/registry/swiftui/content-minimal/NostrMinimalContentView.swift?raw";
import contentMinimalPreviewSwift from "../../../crates/nmp-cli/registry/swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift?raw";
import contentViewSwift from "../../../crates/nmp-cli/registry/swiftui/content-view/NostrContentView.swift?raw";
import contentGroupingSwift from "../../../crates/nmp-cli/registry/swiftui/content-view/NostrContentGrouping.swift?raw";
import contentViewPreviewSwift from "../../../crates/nmp-cli/registry/swiftui/content-view/Examples/NostrContentViewPreview.swift?raw";
import mentionChipSwift from "../../../crates/nmp-cli/registry/swiftui/content-mention-chip/NostrMentionChip.swift?raw";
import quoteCardSwift from "../../../crates/nmp-cli/registry/swiftui/content-quote-card/NostrQuoteCard.swift?raw";
import mediaGridSwift from "../../../crates/nmp-cli/registry/swiftui/content-media-grid/NostrMediaGrid.swift?raw";

// Compose (M16-C4): mirror the SwiftUI components in behavior.
import composeContentRendererKotlin from "../../../crates/nmp-cli/registry/compose/content-core/NostrContentRenderer.kt?raw";
import composeContentTreeWireKotlin from "../../../crates/nmp-cli/registry/compose/content-core/ContentTreeWire.kt?raw";
import composeContentViewKotlin from "../../../crates/nmp-cli/registry/compose/content-view/NostrContentView.kt?raw";
import composeContentGroupingKotlin from "../../../crates/nmp-cli/registry/compose/content-view/NostrContentGrouping.kt?raw";
import composeMentionChipKotlin from "../../../crates/nmp-cli/registry/compose/content-mention-chip/NostrMentionChip.kt?raw";
import composeQuoteCardKotlin from "../../../crates/nmp-cli/registry/compose/content-quote-card/NostrQuoteCard.kt?raw";
import composeMediaGridKotlin from "../../../crates/nmp-cli/registry/compose/content-media-grid/NostrMediaGrid.kt?raw";

export type ComponentFile = {
  /** Source path in the registry tree (where `nmp add component` reads from). */
  source: string;
  /** Target path inside the user's app (where the file is installed). */
  target: string;
  /** `source` (main implementation) or `example` (preview / demo). */
  role: "source" | "example";
  /**
   * The raw file body. `null` if the file is not yet committed to the
   * registry (in-flight on a feature branch).
   */
  content: string | null;
};

export type Component = {
  id: string;
  /** Short label for the sidebar (e.g. `content-view`). */
  slug: string;
  /** Path segment for `/components/<slug>` routes. */
  routeId: string;
  version: string;
  target: "swiftui" | "compose";
  description: string;
  longDescription?: string;
  dependencies: string[];
  files: ComponentFile[];
  /** Set when the component is described in the spec but not yet in the registry. */
  inFlight?: boolean;
  /** Optional list of screenshot variants under `/screenshots/`. */
  screenshots: string[];
  /** Extra notes specific to the component (rendered as paragraphs). */
  customization: string[];
};

export const COMPONENTS: Component[] = [
  {
    id: "swiftui/content-core",
    slug: "content-core",
    routeId: "content-core",
    version: "0.2.0",
    target: "swiftui",
    description:
      "Shared SwiftUI renderer configuration + ContentTreeWire Codable mirror for app-owned Nostr content components.",
    longDescription:
      "`NostrContentRenderer` is the small environment-injected struct every content component reads to pick colors and tap callbacks. Install it once; every other content component on this page picks it up automatically.",
    dependencies: [],
    files: [
      {
        source: "swiftui/content-core/NostrContentRenderer.swift",
        target: "Components/NostrContent/NostrContentRenderer.swift",
        role: "source",
        content: contentCoreSwift,
      },
      {
        source: "swiftui/content-core/ContentTreeWire.swift",
        target: "Components/NostrContent/ContentTreeWire.swift",
        role: "source",
        content: contentCoreWireSwift,
      },
    ],
    screenshots: ["content-core-preview.png"],
    customization: [
      "Edit `NostrContentRenderer.swift` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
      "Inject a per-screen renderer with `.nostrContentRenderer(...)` on any SwiftUI view; child content components pick it up via `@Environment(\\.nostrContentRenderer)`.",
      "`nmp update component` is a structural three-way merge: edits that don't touch upstream lines are preserved automatically; conflicts surface as `.orig` files you can resolve before committing.",
    ],
  },
  {
    id: "swiftui/content-minimal",
    slug: "content-minimal",
    routeId: "content-minimal",
    version: "0.1.0",
    target: "swiftui",
    description:
      "Minimal SwiftUI Nostr content renderer with inline text, mentions, links, and hashtags.",
    longDescription:
      "A flow-layout view that walks an array of `NostrContentRun` values and renders text, mentions, hashtags, and links inline. The simplest component that gets you a working timeline cell.",
    dependencies: ["swiftui/content-core"],
    files: [
      {
        source: "swiftui/content-minimal/NostrMinimalContentView.swift",
        target: "Components/NostrContent/NostrMinimalContentView.swift",
        role: "source",
        content: contentMinimalSwift,
      },
      {
        source: "swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift",
        target: "Components/NostrContent/Examples/NostrMinimalContentPreview.swift",
        role: "example",
        content: contentMinimalPreviewSwift,
      },
    ],
    screenshots: ["content-minimal-preview.png"],
    customization: [
      "Pure SwiftUI — no UIKit, no third-party packages. Swap `FlowLayout` for `HStack` or your own layout if you want different wrapping behaviour.",
      "The view reads `@Environment(\\.nostrContentRenderer)` for colors and callbacks, so customizing the look usually means tweaking the parent's `.nostrContentRenderer(...)` modifier rather than editing this file.",
      "Add new `NostrContentRun.Kind` cases (emoji, custom badges, code spans) and extend `runView` — the structural three-way merge in `nmp update component` will preserve your additions when upstream evolves.",
    ],
  },
  {
    id: "swiftui/content-view",
    slug: "content-view",
    routeId: "content-view",
    version: "0.1.1",
    target: "swiftui",
    description:
      "Full `ContentTreeWire` renderer. Stitches text runs, mentions, quote cards, and media grids into one view.",
    dependencies: [
      "swiftui/content-core",
      "swiftui/content-media-grid",
      "swiftui/content-quote-card",
    ],
    files: [
      {
        source: "swiftui/content-view/NostrContentView.swift",
        target: "Components/NostrContent/NostrContentView.swift",
        role: "source",
        content: contentViewSwift,
      },
      {
        source: "swiftui/content-view/NostrContentGrouping.swift",
        target: "Components/NostrContent/NostrContentGrouping.swift",
        role: "source",
        content: contentGroupingSwift,
      },
      {
        source: "swiftui/content-view/Examples/NostrContentViewPreview.swift",
        target: "Components/NostrContent/Examples/NostrContentViewPreview.swift",
        role: "example",
        content: contentViewPreviewSwift,
      },
    ],
    screenshots: ["content-view-preview.png"],
    customization: [
      "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content`. Each tree node maps to a sub-component you installed alongside it, so customizing the look usually means editing the sub-component file rather than this dispatcher.",
      "Pin the tree's media layout, quote card style, and mention chip palette by overriding the `NostrContentRenderer` environment value on the parent view.",
    ],
  },
  {
    id: "swiftui/content-mention-chip",
    slug: "content-mention-chip",
    routeId: "content-mention-chip",
    version: "0.1.0",
    target: "swiftui",
    description:
      "Avatar + display-name chip used inline anywhere a Nostr profile is referenced.",
    dependencies: ["swiftui/content-core"],
    files: [
      {
        source: "swiftui/content-mention-chip/NostrMentionChip.swift",
        target: "Components/NostrContent/NostrMentionChip.swift",
        role: "source",
        content: mentionChipSwift,
      },
    ],
    screenshots: ["content-mention-chip-preview.png"],
    customization: [
      "Includes a tiny avatar loader fallback. Replace the `AsyncImage` with your own image cache if you already have one (Kingfisher, Nuke, etc).",
      "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push your own profile view.",
    ],
  },
  {
    id: "swiftui/content-quote-card",
    slug: "content-quote-card",
    routeId: "content-quote-card",
    version: "0.1.1",
    target: "swiftui",
    description:
      "Quoted-note card — author header, content preview, subtle border. Drops into any feed.",
    dependencies: ["swiftui/content-core"],
    files: [
      {
        source: "swiftui/content-quote-card/NostrQuoteCard.swift",
        target: "Components/NostrContent/NostrQuoteCard.swift",
        role: "source",
        content: quoteCardSwift,
      },
    ],
    screenshots: ["content-quote-card-preview.png"],
    customization: [
      "Renders a hydrated `NostrQuoteCardModel`; apps resolve quoted events from their own state and pass preview text, author display data, and optional media thumbnails.",
      "Adjust the border, corner radius, and padding directly in the source file — they're literals, not configuration knobs, so they merge cleanly on `nmp update`.",
    ],
  },
  {
    id: "swiftui/content-media-grid",
    slug: "content-media-grid",
    routeId: "content-media-grid",
    version: "0.1.0",
    target: "swiftui",
    description:
      "Adaptive 1–4 image / video grid for inline media attached to a note.",
    dependencies: ["swiftui/content-core"],
    files: [
      {
        source: "swiftui/content-media-grid/NostrMediaGrid.swift",
        target: "Components/NostrContent/NostrMediaGrid.swift",
        role: "source",
        content: mediaGridSwift,
      },
    ],
    screenshots: ["content-media-grid-preview.png"],
    customization: [
      "Grid layout is computed from the count: 1 item = full-width 16:9, 2 = side-by-side, 3 = one-large + two-stacked, 4 = 2x2.",
      "Replace the `AsyncImage` calls with your own loader; the file deliberately exposes a `MediaThumbnailLoader` typealias for that swap.",
    ],
  },
  // ---------------------------------------------------------------------
  // Compose components (M16-C4). Mirror the SwiftUI components above in
  // behavior; identical install layout + dependency graph.
  // ---------------------------------------------------------------------
  {
    id: "compose/content-core",
    slug: "compose-content-core",
    routeId: "compose-content-core",
    version: "0.1.0",
    target: "compose",
    description:
      "Shared Compose renderer configuration + ContentTreeWire kotlinx.serialization mirror for app-owned Nostr content components.",
    longDescription:
      "`NostrContentRenderer` is the small CompositionLocal-injected data class every content component reads to pick colors and tap callbacks. Install it once; every other Compose content component on this page picks it up automatically.",
    dependencies: [],
    files: [
      {
        source: "compose/content-core/NostrContentRenderer.kt",
        target: "Components/NostrContent/NostrContentRenderer.kt",
        role: "source",
        content: composeContentRendererKotlin,
      },
      {
        source: "compose/content-core/ContentTreeWire.kt",
        target: "Components/NostrContent/ContentTreeWire.kt",
        role: "source",
        content: composeContentTreeWireKotlin,
      },
    ],
    screenshots: ["content-core-preview.png"],
    customization: [
      "Edit `NostrContentRenderer.kt` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
      "Inject a per-screen renderer with `CompositionLocalProvider(LocalNostrContentRenderer provides ...)`; child content components pick it up via `LocalNostrContentRenderer.current`.",
      "`ContentTreeWire.kt` uses `kotlinx.serialization` with `@JsonClassDiscriminator(\"kind\")` so the JSON arena emitted by the Rust `nmp-content` crate decodes drift-free. Unknown variants are forward-compat: keep `ignoreUnknownKeys = true` on your `Json` config when adopting new framework versions.",
    ],
  },
  {
    id: "compose/content-mention-chip",
    slug: "compose-content-mention-chip",
    routeId: "compose-content-mention-chip",
    version: "0.1.0",
    target: "compose",
    description:
      "Tappable Compose profile mention chip with optional avatar and identicon fallback.",
    dependencies: ["compose/content-core"],
    files: [
      {
        source: "compose/content-mention-chip/NostrMentionChip.kt",
        target: "Components/NostrContent/NostrMentionChip.kt",
        role: "source",
        content: composeMentionChipKotlin,
      },
    ],
    screenshots: ["content-mention-chip-preview.png"],
    customization: [
      "Uses Coil's `SubcomposeAsyncImage` for the avatar. Swap to your own image loader (Glide, custom Painter, etc.) by replacing the loader call in `MentionAvatar`.",
      "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push into your own navigator.",
    ],
  },
  {
    id: "compose/content-media-grid",
    slug: "compose-content-media-grid",
    routeId: "compose-content-media-grid",
    version: "0.1.0",
    target: "compose",
    description:
      "Adaptive Compose 1–4 image grid for inline media attached to a note.",
    dependencies: ["compose/content-core"],
    files: [
      {
        source: "compose/content-media-grid/NostrMediaGrid.kt",
        target: "Components/NostrContent/NostrMediaGrid.kt",
        role: "source",
        content: composeMediaGridKotlin,
      },
    ],
    screenshots: ["content-media-grid-preview.png"],
    customization: [
      "Layout is count-driven: 1 = full-width 16:9, 2 = side-by-side, 3 = one large + two stacked, 4+ = 2×2 with `+N more` overlay — identical to the SwiftUI variant.",
      "Replace `SubcomposeAsyncImage` with your own loader if you already use Glide/Picasso. The cell composable is intentionally small to make the swap painless.",
    ],
  },
  {
    id: "compose/content-quote-card",
    slug: "compose-content-quote-card",
    routeId: "compose-content-quote-card",
    version: "0.1.1",
    target: "compose",
    description:
      "Reusable Compose quote / embed card — collapsed, compact, rich, and missing variants.",
    dependencies: ["compose/content-core"],
    files: [
      {
        source: "compose/content-quote-card/NostrQuoteCard.kt",
        target: "Components/NostrContent/NostrQuoteCard.kt",
        role: "source",
        content: composeQuoteCardKotlin,
      },
    ],
    screenshots: ["content-quote-card-preview.png"],
    customization: [
      "Pick the variant per call-site — `Rich` for inline quote cards, `Collapsed` for a `View quote` affordance, `Missing` for an unresolved reference, `Compact` for dense feeds.",
      "Border, corner radius, and padding are literals so they merge cleanly on `nmp update component`.",
    ],
  },
  {
    id: "compose/content-view",
    slug: "compose-content-view",
    routeId: "compose-content-view",
    version: "0.1.0",
    target: "compose",
    description:
      "Full Compose `ContentTreeWire` renderer. Stitches inline runs, mentions, quote cards, and media grids into one composable.",
    dependencies: [
      "compose/content-core",
      "compose/content-media-grid",
      "compose/content-quote-card",
    ],
    files: [
      {
        source: "compose/content-view/NostrContentView.kt",
        target: "Components/NostrContent/NostrContentView.kt",
        role: "source",
        content: composeContentViewKotlin,
      },
      {
        source: "compose/content-view/NostrContentGrouping.kt",
        target: "Components/NostrContent/NostrContentGrouping.kt",
        role: "source",
        content: composeContentGroupingKotlin,
      },
    ],
    screenshots: ["content-view-preview.png"],
    customization: [
      "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content` and dispatches each block-level group to the matching sub-component. Customizing the look usually means editing the sub-component file rather than this dispatcher.",
      "Inline runs are concatenated into a single `AnnotatedString` and rendered through `ClickableText` so tap-offset → annotation routing dispatches the matching callback in `LocalNostrContentRenderer.current.callbacks`.",
    ],
  },
];

/** Components grouped by target platform for the sidebar. */
export type ComponentGroup = {
  label: string;
  status: "stable" | "soon";
  components: Component[];
};

export const COMPONENT_GROUPS: ComponentGroup[] = [
  {
    label: "SwiftUI",
    status: "stable",
    components: COMPONENTS.filter((c) => c.target === "swiftui"),
  },
  {
    label: "Compose",
    status: "stable",
    components: COMPONENTS.filter((c) => c.target === "compose"),
  },
];

export function findComponent(id: string): Component | undefined {
  return COMPONENTS.find((c) => c.routeId === id);
}

/** CLI install string for a component, e.g. `nmp add component swiftui/content-view`. */
export function installCommand(c: Component): string {
  return `nmp add component ${c.id}`;
}
