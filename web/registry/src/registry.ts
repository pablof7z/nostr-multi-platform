/*
 * Static component manifest for the registry showcase site.
 *
 * Each entry is a LOGICAL component (e.g. "content-core"). Each logical
 * component has a `platforms` map whose keys are the supported target
 * platforms. The ComponentPage renders a platform switcher that drives
 * which code files and screenshots are shown.
 *
 * This is the SINGLE SOURCE OF TRUTH the site renders. It is hand-mirrored
 * from `crates/nmp-cli/registry/registry.toml` (the CLI's manifest).
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
import loginBlockSwift from "../../../crates/nmp-cli/registry/swiftui/login-block/NostrLoginBlock.swift?raw";

// User profile — SwiftUI
import profileWireSwift from "../../../crates/nmp-cli/registry/swiftui/user-core/ProfileWire.swift?raw";
import nostrAvatarSwift from "../../../crates/nmp-cli/registry/swiftui/user-core/NostrAvatar.swift?raw";
import nostrProfileNameSwift from "../../../crates/nmp-cli/registry/swiftui/user-name/NostrProfileName.swift?raw";
import nostrNip05BadgeSwift from "../../../crates/nmp-cli/registry/swiftui/user-nip05/NostrNip05Badge.swift?raw";
import nostrNpubChipSwift from "../../../crates/nmp-cli/registry/swiftui/user-npub/NostrNpubChip.swift?raw";
import nostrUserCardSwift from "../../../crates/nmp-cli/registry/swiftui/user-card/NostrUserCard.swift?raw";

import composeContentRendererKotlin from "../../../crates/nmp-cli/registry/compose/content-core/NostrContentRenderer.kt?raw";
import composeContentTreeWireKotlin from "../../../crates/nmp-cli/registry/compose/content-core/ContentTreeWire.kt?raw";
import composeContentViewKotlin from "../../../crates/nmp-cli/registry/compose/content-view/NostrContentView.kt?raw";
import composeContentGroupingKotlin from "../../../crates/nmp-cli/registry/compose/content-view/NostrContentGrouping.kt?raw";
import composeMentionChipKotlin from "../../../crates/nmp-cli/registry/compose/content-mention-chip/NostrMentionChip.kt?raw";
import composeQuoteCardKotlin from "../../../crates/nmp-cli/registry/compose/content-quote-card/NostrQuoteCard.kt?raw";
import composeMediaGridKotlin from "../../../crates/nmp-cli/registry/compose/content-media-grid/NostrMediaGrid.kt?raw";

// User profile — Compose
import profileWireKotlin from "../../../crates/nmp-cli/registry/compose/user-core/ProfileWire.kt?raw";
import nostrAvatarKotlin from "../../../crates/nmp-cli/registry/compose/user-core/NostrAvatar.kt?raw";
import nostrProfileNameKotlin from "../../../crates/nmp-cli/registry/compose/user-name/NostrProfileName.kt?raw";
import nostrNip05BadgeKotlin from "../../../crates/nmp-cli/registry/compose/user-nip05/NostrNip05Badge.kt?raw";
import nostrNpubChipKotlin from "../../../crates/nmp-cli/registry/compose/user-npub/NostrNpubChip.kt?raw";
import nostrUserCardKotlin from "../../../crates/nmp-cli/registry/compose/user-card/NostrUserCard.kt?raw";

export type Platform = "swiftui" | "compose" | "tui" | "web";

export const PLATFORM_ORDER: Platform[] = ["swiftui", "compose", "tui", "web"];

export const PLATFORM_LABELS: Record<Platform, string> = {
  swiftui: "Swift",
  compose: "Kotlin",
  tui: "TUI",
  web: "Web",
};

export type ComponentFile = {
  source: string;
  target: string;
  role: "source" | "example";
  content: string | null;
};

export type PlatformImpl = {
  status: "stable" | "soon";
  /** CLI install identifier, e.g. `swiftui/content-core`. */
  installId: string;
  /** Component slugs this impl depends on (same platform implied). */
  dependencies: string[];
  files: ComponentFile[];
  screenshots: string[];
  longDescription?: string;
  customization: string[];
};

export type Component = {
  slug: string;
  routeId: string;
  version: string;
  description: string;
  platforms: Partial<Record<Platform, PlatformImpl>>;
};

export const COMPONENTS: Component[] = [
  {
    slug: "content-core",
    routeId: "content-core",
    version: "0.1.0",
    description:
      "Shared renderer configuration + ContentTreeWire wire type for app-owned Nostr content components.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-core",
        dependencies: [],
        longDescription:
          "`NostrContentRenderer` is the small environment-injected struct every content component reads to pick colors and tap callbacks. Install it once; every other content component on this page picks it up automatically.",
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
        screenshots: ["content-core-swiftui-preview.png"],
        customization: [
          "Edit `NostrContentRenderer.swift` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
          "Inject a per-screen renderer with `.nostrContentRenderer(...)` on any SwiftUI view; child content components pick it up via `@Environment(\\.nostrContentRenderer)`.",
          "`nmp update component` is a structural three-way merge: edits that don't touch upstream lines are preserved automatically; conflicts surface as `.orig` files you can resolve before committing.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-core",
        dependencies: [],
        longDescription:
          "`NostrContentRenderer` is the small CompositionLocal-injected data class every content component reads to pick colors and tap callbacks. Install it once; every other Compose content component on this page picks it up automatically.",
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
        screenshots: ["content-core-compose-preview.png"],
        customization: [
          "Edit `NostrContentRenderer.kt` to change the default text, mention, hashtag, and link colors — or to swap the callback signatures for your own routing model.",
          "Inject a per-screen renderer with `CompositionLocalProvider(LocalNostrContentRenderer provides ...)`; child content components pick it up via `LocalNostrContentRenderer.current`.",
          "`ContentTreeWire.kt` uses `kotlinx.serialization` with `@JsonClassDiscriminator(\"kind\")` so the JSON arena emitted by the Rust `nmp-content` crate decodes drift-free. Unknown variants are forward-compat: keep `ignoreUnknownKeys = true` on your `Json` config when adopting new framework versions.",
        ],
      },
    },
  },
  {
    slug: "content-minimal",
    routeId: "content-minimal",
    version: "0.1.0",
    description:
      "Minimal Nostr content renderer with inline text, mentions, links, and hashtags.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-minimal",
        dependencies: ["content-core"],
        longDescription:
          "A flow-layout view that walks an array of `NostrContentRun` values and renders text, mentions, hashtags, and links inline. The simplest component that gets you a working timeline cell.",
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
        screenshots: ["content-minimal-swiftui-preview.png"],
        customization: [
          "Pure SwiftUI — no UIKit, no third-party packages. Swap `FlowLayout` for `HStack` or your own layout if you want different wrapping behaviour.",
          "The view reads `@Environment(\\.nostrContentRenderer)` for colors and callbacks, so customizing the look usually means tweaking the parent's `.nostrContentRenderer(...)` modifier rather than editing this file.",
          "Add new `NostrContentRun.Kind` cases (emoji, custom badges, code spans) and extend `runView` — the structural three-way merge in `nmp update component` will preserve your additions when upstream evolves.",
        ],
      },
    },
  },
  {
    slug: "content-view",
    routeId: "content-view",
    version: "0.1.0",
    description:
      "Full ContentTreeWire renderer. Stitches text runs, mentions, quote cards, and media grids into one view.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-view",
        dependencies: [
          "content-core",
          "content-minimal",
          "content-mention-chip",
          "content-quote-card",
          "content-media-grid",
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
        screenshots: ["content-view-swiftui-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content`. Each tree node maps to a sub-component you installed alongside it, so customizing the look usually means editing the sub-component file rather than this dispatcher.",
          "Pin the tree's media layout, quote card style, and mention chip palette by overriding the `NostrContentRenderer` environment value on the parent view.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-view",
        dependencies: ["content-core", "content-media-grid", "content-quote-card"],
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
        screenshots: ["content-view-compose-preview.png"],
        customization: [
          "`NostrContentView` walks a `ContentTreeWire` decoded from `nmp-content` and dispatches each block-level group to the matching sub-component. Customizing the look usually means editing the sub-component file rather than this dispatcher.",
          "Inline runs are concatenated into a single `AnnotatedString` and rendered through `ClickableText` so tap-offset → annotation routing dispatches the matching callback in `LocalNostrContentRenderer.current.callbacks`.",
        ],
      },
    },
  },
  {
    slug: "content-mention-chip",
    routeId: "content-mention-chip",
    version: "0.1.0",
    description:
      "Avatar + display-name chip used inline anywhere a Nostr profile is referenced.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-mention-chip",
        dependencies: ["content-core"],
        files: [
          {
            source: "swiftui/content-mention-chip/NostrMentionChip.swift",
            target: "Components/NostrContent/NostrMentionChip.swift",
            role: "source",
            content: mentionChipSwift,
          },
        ],
        screenshots: ["content-mention-chip-swiftui-preview.png"],
        customization: [
          "Includes a tiny avatar loader fallback. Replace the `AsyncImage` with your own image cache if you already have one (Kingfisher, Nuke, etc).",
          "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push your own profile view.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-mention-chip",
        dependencies: ["content-core"],
        files: [
          {
            source: "compose/content-mention-chip/NostrMentionChip.kt",
            target: "Components/NostrContent/NostrMentionChip.kt",
            role: "source",
            content: composeMentionChipKotlin,
          },
        ],
        screenshots: ["content-mention-chip-compose-preview.png"],
        customization: [
          "Uses Coil's `SubcomposeAsyncImage` for the avatar. Swap to your own image loader (Glide, custom Painter, etc.) by replacing the loader call in `MentionAvatar`.",
          "Tap routes through `NostrContentCallbacks.onMentionTap`; override at the screen level to push into your own navigator.",
        ],
      },
    },
  },
  {
    slug: "content-quote-card",
    routeId: "content-quote-card",
    version: "0.1.0",
    description:
      "Quoted-note card — author header, content preview, subtle border. Drops into any feed.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-quote-card",
        dependencies: ["content-core", "content-minimal"],
        files: [
          {
            source: "swiftui/content-quote-card/NostrQuoteCard.swift",
            target: "Components/NostrContent/NostrQuoteCard.swift",
            role: "source",
            content: quoteCardSwift,
          },
        ],
        screenshots: ["content-quote-card-swiftui-preview.png"],
        customization: [
          "Renders a `ContentTreeWire` recursively, so a quoted note that itself contains quotes renders correctly to whatever depth your app chooses to allow (the renderer caps recursion at three levels by default).",
          "Adjust the border, corner radius, and padding directly in the source file — they're literals, not configuration knobs, so they merge cleanly on `nmp update`.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-quote-card",
        dependencies: ["content-core"],
        files: [
          {
            source: "compose/content-quote-card/NostrQuoteCard.kt",
            target: "Components/NostrContent/NostrQuoteCard.kt",
            role: "source",
            content: composeQuoteCardKotlin,
          },
        ],
        screenshots: ["content-quote-card-compose-preview.png"],
        customization: [
          "Pick the variant per call-site — `Rich` for inline quote cards, `Collapsed` for a `View quote` affordance, `Missing` for an unresolved reference, `Compact` for dense feeds.",
          "Border, corner radius, and padding are literals so they merge cleanly on `nmp update component`.",
        ],
      },
    },
  },
  {
    slug: "content-media-grid",
    routeId: "content-media-grid",
    version: "0.1.0",
    description:
      "Adaptive 1–4 image / video grid for inline media attached to a note.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/content-media-grid",
        dependencies: ["content-core"],
        files: [
          {
            source: "swiftui/content-media-grid/NostrMediaGrid.swift",
            target: "Components/NostrContent/NostrMediaGrid.swift",
            role: "source",
            content: mediaGridSwift,
          },
        ],
        screenshots: ["content-media-grid-swiftui-preview.png"],
        customization: [
          "Grid layout is computed from the count: 1 item = full-width 16:9, 2 = side-by-side, 3 = one-large + two-stacked, 4 = 2x2.",
          "Replace the `AsyncImage` calls with your own loader; the file deliberately exposes a `MediaThumbnailLoader` typealias for that swap.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/content-media-grid",
        dependencies: ["content-core"],
        files: [
          {
            source: "compose/content-media-grid/NostrMediaGrid.kt",
            target: "Components/NostrContent/NostrMediaGrid.kt",
            role: "source",
            content: composeMediaGridKotlin,
          },
        ],
        screenshots: ["content-media-grid-compose-preview.png"],
        customization: [
          "Layout is count-driven: 1 = full-width 16:9, 2 = side-by-side, 3 = one large + two stacked, 4+ = 2×2 with `+N more` overlay — identical to the SwiftUI variant.",
          "Replace `SubcomposeAsyncImage` with your own loader if you already use Glide/Picasso. The cell composable is intentionally small to make the swap painless.",
        ],
      },
    },
  },
  {
    slug: "login-block",
    routeId: "login-block",
    version: "0.1.0",
    description:
      "Login UI with Amber, Primal, and other local Nostr signer detection, plus a manual key entry fallback.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/login-block",
        dependencies: ["content-core"],
        longDescription:
          "`NostrLoginBlock` probes the device for installed Nostr signer apps (Amber, Primal, nostrconnect-compatible) via `UIApplication.canOpenURL` and surfaces each one as a tappable card. If no signers are found it shows only the manual key entry option with an install hint. Detection happens lazily in `.task {}` — never at module load — so `UIApplication.shared` is always fully active when the probe runs.",
        files: [
          {
            source: "swiftui/login-block/NostrLoginBlock.swift",
            target: "Components/Auth/NostrLoginBlock.swift",
            role: "source",
            content: loginBlockSwift,
          },
        ],
        screenshots: [],
        customization: [
          "Add `LSApplicationQueriesSchemes` to your app's Info.plist listing `nostrsigner`, `primal`, and `nostrconnect`. Without this entry `canOpenURL` always returns `false`, even when the signer is installed.",
          "Extend `NostrSignerDetector.knownSigners` to add future signer apps. Each entry needs its URL scheme listed in Info.plist too.",
          "Theming is driven by `NostrContentRenderer` from the `swiftui/content-core` dependency. Override colors with `.nostrContentRenderer(...)` on a parent view.",
          "Wire `onSignerSelected` to your NIP-46 Nostr Connect or NIP-55 deep-link flow. The `NostrSignerInfo.urlScheme` value is the scheme to use when constructing the handshake URL.",
        ],
      },
    },
  },
  {
    slug: "user-core",
    routeId: "user-core",
    version: "0.1.0",
    description:
      "ProfileWire wire type and NostrAvatar — the foundation for all user profile components.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-core",
        dependencies: [],
        longDescription:
          "`ProfileWire` is the Codable struct decoded from the `nmp-profile` projection. `NostrAvatar` renders the picture URL with a deterministic identicon fallback. Install once; every other user component depends on these.",
        files: [
          {
            source: "swiftui/user-core/ProfileWire.swift",
            target: "Components/NostrUser/ProfileWire.swift",
            role: "source",
            content: profileWireSwift,
          },
          {
            source: "swiftui/user-core/NostrAvatar.swift",
            target: "Components/NostrUser/NostrAvatar.swift",
            role: "source",
            content: nostrAvatarSwift,
          },
        ],
        screenshots: ["user-core-swiftui-preview.png"],
        customization: [
          "Edit the `palette` array in `NostrIdenticon` to match your app's brand colors. The color is deterministic from the pubkey so the same user always gets the same color.",
          "Replace `AsyncImage` in `NostrAvatar` with your own image cache (Kingfisher, Nuke) by swapping the URL loading block — the identicon fallback is self-contained.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-core",
        dependencies: [],
        longDescription:
          "`ProfileWire` is the `@Serializable` data class decoded from the `nmp-profile` projection. `NostrAvatar` renders the picture URL via Coil with a deterministic identicon fallback. Install once; every other Compose user component depends on these.",
        files: [
          {
            source: "compose/user-core/ProfileWire.kt",
            target: "Components/NostrUser/ProfileWire.kt",
            role: "source",
            content: profileWireKotlin,
          },
          {
            source: "compose/user-core/NostrAvatar.kt",
            target: "Components/NostrUser/NostrAvatar.kt",
            role: "source",
            content: nostrAvatarKotlin,
          },
        ],
        screenshots: ["user-core-compose-preview.png"],
        customization: [
          "Edit `IDENTICON_PALETTE` in `NostrAvatar.kt` to match your brand colors.",
          "Replace `SubcomposeAsyncImage` with Glide or a custom `Painter` — the identicon fallback composables are self-contained and don't depend on Coil.",
        ],
      },
    },
  },
  {
    slug: "user-name",
    routeId: "user-name",
    version: "0.1.0",
    description: "Inline display-name text with fallback to Rust-truncated npub.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-name",
        dependencies: ["user-core"],
        files: [
          {
            source: "swiftui/user-name/NostrProfileName.swift",
            target: "Components/NostrUser/NostrProfileName.swift",
            role: "source",
            content: nostrProfileNameSwift,
          },
        ],
        screenshots: ["user-name-swiftui-preview.png"],
        customization: [
          "Pass any `Font` and `Color` — the component has no hardcoded styling. For section headers use `.headline`; for secondary rows use `.subheadline` and a muted color.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-name",
        dependencies: ["user-core"],
        files: [
          {
            source: "compose/user-name/NostrProfileName.kt",
            target: "Components/NostrUser/NostrProfileName.kt",
            role: "source",
            content: nostrProfileNameKotlin,
          },
        ],
        screenshots: ["user-name-compose-preview.png"],
        customization: [
          "Pass any `TextStyle` and `Color` — no hardcoded styling. Use `MaterialTheme.typography.titleMedium` for headers and `bodySmall` for secondary rows.",
        ],
      },
    },
  },
  {
    slug: "user-nip05",
    routeId: "user-nip05",
    version: "0.1.0",
    description: "NIP-05 verified identity badge — checkmark icon and identifier string.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-nip05",
        dependencies: ["user-core"],
        files: [
          {
            source: "swiftui/user-nip05/NostrNip05Badge.swift",
            target: "Components/NostrUser/NostrNip05Badge.swift",
            role: "source",
            content: nostrNip05BadgeSwift,
          },
        ],
        screenshots: ["user-nip05-swiftui-preview.png"],
        customization: [
          "The failable `init?(profile:)` lets you gate the badge in one line: `if let badge = NostrNip05Badge(profile: profile) { badge }`. Use the non-failable `init(nip05:)` when you've already checked for a value.",
          "Swap `Color.accentColor` for your brand verification color on the checkmark icon.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-nip05",
        dependencies: ["user-core"],
        files: [
          {
            source: "compose/user-nip05/NostrNip05Badge.kt",
            target: "Components/NostrUser/NostrNip05Badge.kt",
            role: "source",
            content: nostrNip05BadgeKotlin,
          },
        ],
        screenshots: ["user-nip05-compose-preview.png"],
        customization: [
          "Both overloads are provided: `NostrNip05Badge(profile)` returns early when nip05 is absent; `NostrNip05Badge(nip05)` renders unconditionally when you've already validated the value.",
          "Swap `MaterialTheme.colorScheme.primary` for your brand verification color on the icon tint.",
        ],
      },
    },
  },
  {
    slug: "user-npub",
    routeId: "user-npub",
    version: "0.1.0",
    description: "Tappable npub chip — shows Rust-truncated npub and copies full bech32 on tap.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-npub",
        dependencies: ["user-core"],
        files: [
          {
            source: "swiftui/user-npub/NostrNpubChip.swift",
            target: "Components/NostrUser/NostrNpubChip.swift",
            role: "source",
            content: nostrNpubChipSwift,
          },
        ],
        screenshots: ["user-npub-swiftui-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Swift (aim.md §6.9). The Rust side owns the truncation length and encoding.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-npub",
        dependencies: ["user-core"],
        files: [
          {
            source: "compose/user-npub/NostrNpubChip.kt",
            target: "Components/NostrUser/NostrNpubChip.kt",
            role: "source",
            content: nostrNpubChipKotlin,
          },
        ],
        screenshots: ["user-npub-compose-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Kotlin.",
          "Uses `ClipboardManager` directly; no permission required on API 32 and below. On API 33+ the system shows a clipboard toast automatically.",
        ],
      },
    },
  },
  {
    slug: "user-card",
    routeId: "user-card",
    version: "0.1.0",
    description: "Compact author header: avatar, display name, and optional NIP-05 badge.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-card",
        dependencies: ["user-core", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works in any navigation stack.",
        files: [
          {
            source: "swiftui/user-card/NostrUserCard.swift",
            target: "Components/NostrUser/NostrUserCard.swift",
            role: "source",
            content: nostrUserCardSwift,
          },
        ],
        screenshots: ["user-card-swiftui-preview.png"],
        customization: [
          "Set `avatarSize` to `32` for dense list rows and `64` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own profile route from there rather than hardcoding any navigation dependency inside this component.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-card",
        dependencies: ["user-core", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works with any Compose navigation setup.",
        files: [
          {
            source: "compose/user-card/NostrUserCard.kt",
            target: "Components/NostrUser/NostrUserCard.kt",
            role: "source",
            content: nostrUserCardKotlin,
          },
        ],
        screenshots: ["user-card-compose-preview.png"],
        customization: [
          "Set `avatarSize` to `32.dp` for dense list rows and `64.dp` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own NavController route from there rather than importing any navigation dependency inside this component.",
        ],
      },
    },
  },
];

export function findComponent(routeId: string): Component | undefined {
  return COMPONENTS.find((c) => c.routeId === routeId);
}

/** CLI install string for a component impl, e.g. `nmp add component swiftui/content-view`. */
export function installCommand(installId: string): string {
  return `nmp add component ${installId}`;
}
