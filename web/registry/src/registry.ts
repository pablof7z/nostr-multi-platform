/*
 * Static component manifest for the registry showcase site.
 *
 * This is the SINGLE SOURCE OF TRUTH the site renders. It is hand-mirrored
 * from `crates/nmp-cli/registry/registry.toml` (the CLI's manifest).
 * Where a component exists in the registry, the `source` field is a string
 * (imported via Vite `?raw`). Where a component is still being built on a
 * feature branch, `source` is null and the page renders a placeholder.
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
    version: "0.1.0",
    target: "swiftui",
    description:
      "Shared SwiftUI renderer configuration for app-owned Nostr content components.",
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
    version: "0.1.0",
    target: "swiftui",
    description:
      "Full `ContentTreeWire` renderer. Stitches text runs, mentions, quote cards, and media grids into one view.",
    dependencies: [
      "swiftui/content-core",
      "swiftui/content-minimal",
      "swiftui/content-mention-chip",
      "swiftui/content-quote-card",
      "swiftui/content-media-grid",
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
    version: "0.1.0",
    target: "swiftui",
    description:
      "Quoted-note card — author header, content preview, subtle border. Drops into any feed.",
    dependencies: ["swiftui/content-core", "swiftui/content-minimal"],
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
      "Renders a `ContentTreeWire` recursively, so a quoted note that itself contains quotes renders correctly to whatever depth your app chooses to allow (the renderer caps recursion at three levels by default).",
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
    status: "soon",
    components: [],
  },
];

export function findComponent(id: string): Component | undefined {
  return COMPONENTS.find((c) => c.routeId === id);
}

/** CLI install string for a component, e.g. `nmp add component swiftui/content-view`. */
export function installCommand(c: Component): string {
  return `nmp add component ${c.id}`;
}
