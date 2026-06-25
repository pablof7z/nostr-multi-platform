---
title: Native iOS UI Conventions
slug: native-ios-ui-conventions
topic: ui-components
summary: Chirp must be footgun-free such that anyone could build an app like Chirp in one shot with any capable LLM
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-06-19
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:5d893073-9635-450b-b8e9-50648bc1a4e7
  - session:cb3376a7-cea1-49ac-b6dd-9251fa1af14a
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:835e3f03-658e-4150-a31a-cd4986ab5308
---

# Native iOS UI Conventions

## Native iOS UI Conventions

Chirp must be footgun-free such that anyone could build an app like Chirp in one shot with any capable LLM. The entire Chirp iOS app must look like a normal native iOS app, using only typical native controls with semantic names and no hardcoded colors or custom styles. All feature views must use native List/Form with Section headers and NavigationLink with Label instead of custom styled scrollviews. No Swift files in the Chirp iOS app may contain Color.black, Color.blue, Color.indigo, Color.purple, .listRowBackground(Color.clear), or .scrollContentBackground(.hidden). SwiftUI foregroundStyle and tint must use Color.accentColor rather than the invalid .accent ShapeStyle member.

Obviously broken core functionality (profiles not loading, embedded events showing placeholder text, images not tappable, duplicate relays) must be prioritized over minor UI polish like animations and haptics. <!-- [^19e07-6] -->

<!-- citations: [^5d893-1] [^c4b2e-4] -->
## Pasteboard Access

Chirp onboarding and wallet-connect views must not read UIPasteboard.general.string inside SwiftUI body expressions; users paste via the standard iOS text field long-press menu instead. <!-- [^5d893-2] -->

## Design Tokens

ChirpTheme.swift tokens map to native iOS semantic values: ChirpColor uses Color.accentColor, Color.primary, Color.secondary, Color.tertiaryLabel, Color.separator, UIColor.systemBackground, UIColor.secondarySystemBackground; ChirpFont uses standard Font modifiers (e.g., Font.largeTitle.weight(.bold)). <!-- [^5d893-3] -->

## Component Neutralization

GlassCard, ChirpPrimaryButton, ChirpSectionHeader, and ChirpAvatar are neutralized to plain native wrappers: GlassCard is just padding, ChirpPrimaryButton is a plain Button, ChirpSectionHeader is Text(title).font(.caption). ChirpAvatar is fully retired in favor of the NostrAvatar registry component (swiftui/user-avatar). NostrAvatar falls back to a brand gradient and initials when no picture URL is available. NostrAvatar in feeds uses .cacheOk liveness mode (cache-first with OneShot fill). DM conversation rows, group chat message rows, and Marmot message rows all use NostrAvatar with a deterministic color derived from the peer's pubkey prefix instead of a flat gray circle. All note row components (NoteRowView, ProfileNoteRow, ThreadNoteRow, ModularBlockView) must be stripped of custom colors and backgrounds. The focused note in ThreadNoteRow uses a 46pt NostrAvatar; the regular reply row uses a 38pt NostrAvatar. The thread connecting lines in ModularBlockView use a 44pt NostrAvatar with a vertical line overlay between replies.

<!-- citations: [^5d893-4] [^19e07-5] [^835e3-1] -->
## View-Specific Rules

SettingsHubView must use native Form with Section headers, NavigationLink with Label, plain relay rows, and inline relay editing.
AccountsView must use native List with ContentUnavailableView, plain account rows with ChirpAvatar and checkmark for active state, and AddAccountSheet using Picker(.segmented). The bottom TabView contains 5 tabs (reduced from 6), which eliminates the duplicate back button on the Accounts settings screen.
OnboardingView must use Color(.systemBackground) with no gradient background or decorative orbs.
HomeFeedView must not use .listStyle(.plain), .scrollContentBackground(.hidden), .background(ChirpColor.bg), or custom toolbar button styling.
ProfileView must not have a banner gradient; follow/unfollow buttons are placed in .toolbar with no custom divider backgrounds.
ComposeView must use plain VStack instead of GlassCard, and TextEditor must not have .scrollContentBackground(.hidden) or .background(Color.clear).
SearchView must use plain TextField and Button with Label, with no capsule buttons, GlassCard, or custom input backgrounds.
NotificationsView must use plain Image, Text, and LazyVGrid, with no custom glow rings, capsule badges, or GlassCard.
DiagnosticsView must use native List with Section headers, with no DiagChip capsule badges or custom RoundedRectangle backgrounds on sections.
RelaySettingsView must use native Form with Section headers, standard NavigationStack sheet, plain Toggle, standard Button, and semantic colors — no Color.purple, Color.blue, GlassCard, ChirpPrimaryButton, capsule badges, .listStyle(.plain), or custom row backgrounds with strokes.
NoteContentView must use Color(.secondarySystemBackground) for video placeholder backgrounds and Color.accentColor for URLs and mentions — no Color.black.opacity(0.72), Color.blue, or Color.indigo.
MarmotGroupsView, MarmotGroupChatView, and MarmotInviteSheet must not use .scrollContentBackground(.hidden) on TextEditors.
RelayDetailView and WireSubscriptionDetailView must use semantic colors and must not use GlassCard.
WalletView must use native List/Form layout and must not ship a hardcoded 21,000 msat default zap amount to production.
No iOS ArticleDetailView or ArticleListView Swift file exists in Chirp; the audit confused Rust ViewModule types in crates/nmp-nip23/ with iOS Swift views.

<!-- citations: [^5d893-5] [^cb337-2] [^1c093-27] [^cd2b6-8] -->
## Component Extraction Policy

NMP UI components must not ship as a library until a second NMP iOS app exists and independently re-implements ≥3 of the same primitives. Every UI component shipped in v1 must have a live Chirp caller in the same PR (no speculative or orphan components). NMPAvatar is the correct first extraction target when app #2 exists, lifting AccountAvatar.swift:8-36 into bindings/swiftui/NMPSwiftUI/. NoteRenderer (NMPNoteContent) is the wrong first-PR candidate for UI extraction; Avatar is the right first component (36 LOC lift, trivial). The apps/chirp/ios/Chirp/Components/ directory should be hardened as the future staging directory for UI components under two rules: snapshot-binding only (no platform policy, no callbacks except literal user gestures → dispatch_action), and every file has ≥1 live screen consuming it (no orphans). ADR-0027 should codify that the eventual UI component extraction shape is sealed, versioned NMPSwiftUI / NMPCompose packages, not a shadcn-style copy-paste registry (which contradicts aim.md §1's 'impossible to build broken'). <!-- [^45fcf-9] -->

## Substrate-Shaped Logic vs. UI Library

Chirp exists to surface substrate gaps; any hack or complex logic in Chirp Swift is the opposite of the project's purpose — the kernel and nmp crates must make things easier for all apps. Chirp's 9.4K Swift LOC overrun is substrate-shaped (missing kernel projections), not UI-library-shaped; extracting shared components shaves only ~1.3K from Components/ while the ~5K in Features/ requires kernel fixes to collapse. Features/ contains ~1,500–2,000 LOC of substrate-shaped logic that should move to Rust; the rest is legitimate SwiftUI declarative layout. The Marmot Swift surface (MarmotGroupsView + MarmotGroupChatView + MarmotInviteSheet = 709 LOC) repeats the canonical bad example named in apps/chirp/AGENTS.md — the same pattern the Rust-side migration already performed. <!-- [^45fcf-10] -->

## Dead Code & LOC Targets

Dead code to delete: SettingsHubView.activeAccountSubtitle (5 LOC unused), roadmapItem (~20 LOC unused), SearchView coming-soon section (~25 LOC inert). The SettingsHub+Search combined PR (#190) achieved −31% LOC (331→229), exceeding the 30% target; all other PRs fell short because substrate-shaped logic was only 30–60 LOC in 200–400 LOC files, with the rest being legitimate SwiftUI layout. <!-- [^45fcf-11] -->
