import type { Component } from "./types";

// User profile — SwiftUI
import profileWireSwift from "../vendor/swiftui/user-avatar/ProfileWire.swift?raw";
import nostrProfileHostSwift from "../vendor/swiftui/user-avatar/NostrProfileHost.swift?raw";
import nostrAvatarSwift from "../vendor/swiftui/user-avatar/NostrAvatar.swift?raw";
import nostrProfileNameSwift from "../vendor/swiftui/user-name/NostrProfileName.swift?raw";
import nostrNip05BadgeSwift from "../vendor/swiftui/user-nip05/NostrNip05Badge.swift?raw";
import nostrNpubChipSwift from "../vendor/swiftui/user-npub/NostrNpubChip.swift?raw";
import nostrUserCardSwift from "../vendor/swiftui/user-card/NostrUserCard.swift?raw";

// User profile — Compose
import profileWireKotlin from "../vendor/compose/user-avatar/ProfileWire.kt?raw";
import nostrProfileHostKotlin from "../vendor/compose/user-avatar/NostrProfileHost.kt?raw";
import nostrAvatarKotlin from "../vendor/compose/user-avatar/NostrAvatar.kt?raw";
import nostrProfileNameKotlin from "../vendor/compose/user-name/NostrProfileName.kt?raw";
import nostrNip05BadgeKotlin from "../vendor/compose/user-nip05/NostrNip05Badge.kt?raw";
import nostrNpubChipKotlin from "../vendor/compose/user-npub/NostrNpubChip.kt?raw";
import nostrUserCardKotlin from "../vendor/compose/user-card/NostrUserCard.kt?raw";

// User profile — Ratatui
import profileWireRust from "../vendor/tui/user-core/profile_wire.rs?raw";
import nostrAvatarRust from "../vendor/tui/user-avatar/nostr_avatar.rs?raw";
import nostrProfileNameRust from "../vendor/tui/user-name/nostr_profile_name.rs?raw";
import nostrNip05BadgeRust from "../vendor/tui/user-nip05/nostr_nip05_badge.rs?raw";
import nostrNpubChipRust from "../vendor/tui/user-npub/nostr_npub_chip.rs?raw";
import nostrUserCardRust from "../vendor/tui/user-card/nostr_user_card.rs?raw";

export const userComponents: Component[] = [
  {
    slug: "user-core",
    routeId: "user-core",
    version: "0.1.0",
    description: "Shared Ratatui ProfileWire mirror for Rust-owned user profile projections.",
    platforms: {
      tui: {
        status: "stable",
        installId: "tui/user-core",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`ProfileWire` is the Rust-side projection mirror used by the TUI user widgets. It carries display-ready profile fields from the kernel; host apps should not derive profile names or npub truncation in terminal UI code.",
        files: [
          { source: "tui/user-core/profile_wire.rs", target: "src/components/nostr_user/profile_wire.rs", role: "source", content: profileWireRust },
        ],
        screenshots: [],
        customization: [
          "Keep this type aligned with the kernel projection and use it as the input to the display widgets.",
        ],
      },
    },
  },
  {
    slug: "user-avatar",
    routeId: "user-avatar",
    version: "0.1.0",
    description: "Reference-first avatar that claims and observes its profile projection.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-avatar",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrAvatar(pubkey:)` claims/releases its own profile interest through `NostrProfileHost`, reads the current Rust-owned profile projection, and falls back to a deterministic identicon until the picture URL arrives. Install once; every other user component depends on the shared `ProfileWire`.",
        files: [
          { source: "swiftui/user-avatar/ProfileWire.swift", target: "Components/NostrUser/ProfileWire.swift", role: "source", content: profileWireSwift },
          { source: "swiftui/user-avatar/NostrProfileHost.swift", target: "Components/NostrUser/NostrProfileHost.swift", role: "source", content: nostrProfileHostSwift },
          { source: "swiftui/user-avatar/NostrAvatar.swift", target: "Components/NostrUser/NostrAvatar.swift", role: "source", content: nostrAvatarSwift },
        ],
        screenshots: ["user-avatar-ios-gallery-preview.png"],
        customization: [
          "Edit the `palette` array in `NostrIdenticon` to match your app's brand colors. The color is deterministic from the pubkey so the same user always gets the same color.",
          "Replace `AsyncImage` with your own image cache (Kingfisher, Nuke) by swapping the URL loading block — the identicon fallback is self-contained.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-avatar",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrAvatar(pubkey = ...)` claims/releases its own profile interest through `LocalNostrProfileHost`, reads the current Rust-owned profile projection, and falls back to a deterministic identicon until the picture URL arrives. Install once; every other Compose user component depends on the shared `ProfileWire`.",
        files: [
          { source: "compose/user-avatar/ProfileWire.kt", target: "Components/NostrUser/ProfileWire.kt", role: "source", content: profileWireKotlin },
          { source: "compose/user-avatar/NostrProfileHost.kt", target: "Components/NostrUser/NostrProfileHost.kt", role: "source", content: nostrProfileHostKotlin },
          { source: "compose/user-avatar/NostrAvatar.kt", target: "Components/NostrUser/NostrAvatar.kt", role: "source", content: nostrAvatarKotlin },
        ],
        screenshots: ["user-avatar-kotlin-preview.png"],
        customization: [
          "Edit `IDENTICON_PALETTE` in `NostrAvatar.kt` to match your brand colors.",
          "Replace `SubcomposeAsyncImage` with Glide or a custom Painter — the identicon fallback composables don't depend on Coil.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-avatar",
        version: "0.1.1",
        dependencies: ["user-core"],
        longDescription:
          "`ProfileWire` mirrors the Rust-owned profile projection. `NostrAvatar` accepts an optional `ratatui-image` protocol supplied by the host app and falls back to initials with a deterministic pubkey-derived accent.",
        files: [
          { source: "tui/user-core/profile_wire.rs", target: "src/components/nostr_user/profile_wire.rs", role: "source", content: profileWireRust },
          { source: "tui/user-avatar/nostr_avatar.rs", target: "src/components/nostr_user/nostr_avatar.rs", role: "source", content: nostrAvatarRust },
        ],
        screenshots: ["tui-user-avatar-preview.png"],
        customization: [
          "Edit `PALETTE` in `nostr_avatar.rs` to match your terminal theme.",
          "The widget is render-only; host apps own image fetching, terminal protocol selection, and navigation.",
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
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-name/NostrProfileName.swift", target: "Components/NostrUser/NostrProfileName.swift", role: "source", content: nostrProfileNameSwift },
        ],
        screenshots: ["user-name-ios-gallery-preview.png"],
        customization: [
          "Pass any `Font` and `Color` — the component has no hardcoded styling. Use `.headline` for headers and `.subheadline` with a muted color for secondary rows.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-name",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-name/NostrProfileName.kt", target: "Components/NostrUser/NostrProfileName.kt", role: "source", content: nostrProfileNameKotlin },
        ],
        screenshots: ["user-name-kotlin-preview.png"],
        customization: [
          "Pass any `TextStyle` and `Color` — no hardcoded styling. Use `MaterialTheme.typography.titleMedium` for headers and `bodySmall` for secondary rows.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-name",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-name/nostr_profile_name.rs", target: "src/components/nostr_user/nostr_profile_name.rs", role: "source", content: nostrProfileNameRust },
        ],
        screenshots: ["tui-user-name-preview.png"],
        customization: [
          "Pass a Ratatui `Style` with `.style(...)`; the fallback label still comes from `ProfileWire::display()`.",
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
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-nip05/NostrNip05Badge.swift", target: "Components/NostrUser/NostrNip05Badge.swift", role: "source", content: nostrNip05BadgeSwift },
        ],
        screenshots: ["user-nip05-ios-gallery-preview.png"],
        customization: [
          "The failable `init?(profile:)` lets you gate the badge in one line: `if let badge = NostrNip05Badge(profile: profile) { badge }`.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
          "Swap `Color.accentColor` for your brand verification color on the checkmark icon.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-nip05",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-nip05/NostrNip05Badge.kt", target: "Components/NostrUser/NostrNip05Badge.kt", role: "source", content: nostrNip05BadgeKotlin },
        ],
        screenshots: ["user-nip05-kotlin-preview.png"],
        customization: [
          "`NostrNip05Badge(profile)` returns early when nip05 is absent; `NostrNip05Badge(nip05)` renders unconditionally when you've already validated the value.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
          "Swap `MaterialTheme.colorScheme.primary` for your brand verification color on the icon tint.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-nip05",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-nip05/nostr_nip05_badge.rs", target: "src/components/nostr_user/nostr_nip05_badge.rs", role: "source", content: nostrNip05BadgeRust },
        ],
        screenshots: ["tui-user-nip05-preview.png"],
        customization: [
          "`NostrNip05Badge::from_profile` returns `None` when the projection has no identifier, so callers can skip the row cleanly.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
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
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-npub/NostrNpubChip.swift", target: "Components/NostrUser/NostrNpubChip.swift", role: "source", content: nostrNpubChipSwift },
        ],
        screenshots: ["user-npub-ios-gallery-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Swift (aim.md §6.9).",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-npub",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-npub/NostrNpubChip.kt", target: "Components/NostrUser/NostrNpubChip.kt", role: "source", content: nostrNpubChipKotlin },
        ],
        screenshots: ["user-npub-kotlin-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Kotlin.",
          "Uses `ClipboardManager` directly; no permission required on API 32 and below.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-npub",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-npub/nostr_npub_chip.rs", target: "src/components/nostr_user/nostr_npub_chip.rs", role: "source", content: nostrNpubChipRust },
        ],
        screenshots: ["tui-user-npub-preview.png"],
        customization: [
          "Clipboard writes are host capabilities; bind your copy key to `profile.npub` outside the widget.",
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
        version: "0.1.0",
        dependencies: ["user-avatar", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works in any navigation stack.",
        files: [
          { source: "swiftui/user-card/NostrUserCard.swift", target: "Components/NostrUser/NostrUserCard.swift", role: "source", content: nostrUserCardSwift },
        ],
        screenshots: ["user-card-ios-gallery-preview.png"],
        customization: [
          "Set `avatarSize` to `32` for dense list rows and `64` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own profile route from there rather than hardcoding any navigation dependency inside this component.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-card",
        version: "0.1.0",
        dependencies: ["user-avatar", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works with any Compose navigation setup.",
        files: [
          { source: "compose/user-card/NostrUserCard.kt", target: "Components/NostrUser/NostrUserCard.kt", role: "source", content: nostrUserCardKotlin },
        ],
        screenshots: ["user-card-kotlin-preview.png"],
        customization: [
          "Set `avatarSize` to `32.dp` for dense list rows and `64.dp` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own NavController route from there.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-card",
        version: "0.1.1",
        dependencies: ["user-core", "user-avatar", "user-name", "user-nip05"],
        longDescription:
          "A compact terminal author header composed from `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge`. Host apps pass an optional avatar image protocol and handle selection/profile navigation in their input loop.",
        files: [
          { source: "tui/user-card/nostr_user_card.rs", target: "src/components/nostr_user/nostr_user_card.rs", role: "source", content: nostrUserCardRust },
        ],
        screenshots: ["tui-user-card-preview.png"],
        customization: [
          "Adjust the outer `Block` style in `nostr_user_card.rs` for dense feeds, modal headers, or focused rows.",
        ],
      },
    },
  },
];
