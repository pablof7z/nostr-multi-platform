---
type: research-record
date: 2026-06-18
session: e7a1d168-3c58-4438-a544-aa645850c388
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e7a1d168-3c58-4438-a544-aa645850c388.jsonl
source_lines: 240-435
agent_attribution: a8e6f566355f0fb98
has_preregistered_criteria: false
has_method: true
has_structured_report: true
characterization: "NMP Component Registry Inventory: cataloged 14 components across platforms, verified all TS-referenced source files exist on disk (6,705 LOC total), found TUI content components exist on disk but not wired into web registry, web platform entirely empty"
captured_at: 2026-06-18T05:39:25Z
---

NMP Component Registry Inventory: cataloged 14 components across platforms, verified all TS-referenced source files exist on disk (6,705 LOC total), found TUI content components exist on disk but not wired into web registry, web platform entirely empty

---

User: [Agent task result: Agent "Audit registry components" completed]
# NMP Component Registry Inventory

## Registry Files Analyzed
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8e6f566355f0fb98/web/registry/src/registry/content.ts`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8e6f566355f0fb98/web/registry/src/registry/user.ts`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8e6f566355f0fb98/web/registry/src/registry/relay.ts`

Source-of-truth filesystem checked at: `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a8e6f566355f0fb98/crates/nmp-cli/registry/`

---

## 1. Complete Component Catalog

### Content Components (`content.ts`) — 8 components

| # | slug | routeId | version | description |
|---|------|---------|---------|-------------|
| 1 | `content-core` | `content-core` | `0.2.0` | Shared renderer configuration + ContentTreeWire wire type for app-owned Nostr content components. |
| 2 | `content-minimal` | `content-minimal` | `0.1.0` | Minimal Nostr content renderer with inline text, mentions, links, and hashtags. |
| 3 | `content-view` | `content-view` | `0.1.1` | Full ContentTreeWire renderer. Stitches text runs, mentions, quote cards, and media grids into one view. |
| 4 | `content-mention-chip` | `content-mention-chip` | `0.1.0` | Avatar + display-name chip used inline anywhere a Nostr profile is referenced. |
| 5 | `content-quote-card` | `content-quote-card` | `0.1.1` | Quoted-note card — author header, content preview, subtle border. Drops into any feed. |
| 6 | `content-media-grid` | `content-media-grid` | `0.1.0` | Adaptive 1–4 image / video grid for inline media attached to a note. |
| 7 | `login-block` | `login-block` | `0.1.0` | Login UI with Amber, Primal, and other local Nostr signer detection, plus a manual key entry fallback. |
| 8 | `relay-list`* | `relay-list` | `0.1.0` | Relay list showing relay URLs with role badges and live connection status dots. |

*Defined in `relay.ts` but logically a content/relay component.

### User Components (`user.ts`) — 5 components

| # | slug | routeId | version | description |
|---|------|---------|---------|-------------|
| 1 | `user-avatar` | `user-avatar` | `0.1.0` | ProfileWire wire type and circular avatar with deterministic identicon fallback. |
| 2 | `user-name` | `user-name` | `0.1.0` | Inline display-name text with fallback to Rust-truncated npub. |
| 3 | `user-nip05` | `user-nip05` | `0.1.0` | NIP-05 verified identity badge — checkmark icon and identifier string. |
| 4 | `user-npub` | `user-npub` | `0.1.0` | Tappable npub chip — shows Rust-truncated npub and copies full bech32 on tap. |
| 5 | `user-card` | `user-card` | `0.1.0` | Compact author header: avatar, display name, and optional NIP-05 badge. |

### Relay Components (`relay.ts`) — 1 component

| # | slug | routeId | version | description |
|---|------|---------|---------|-------------|
| 1 | `relay-list` | `relay-list` | `0.1.0` | Relay list showing relay URLs with role badges and live connection status dots. |

**Total: 14 distinct component slugs.**

---

## 2. Platform Coverage Matrix

| Component | SwiftUI | Compose | TUI | Web |
|-----------|---------|---------|-----|-----|
| `content-core` | stable (0.2.0) | stable (0.1.0) | **missing in TS** | none |
| `content-minimal` | stable (0.1.0) | **none** | **missing in TS*** | none |
| `content-view` | stable (0.1.1) | stable (0.1.0) | **missing in TS** | none |
| `content-mention-chip` | stable (0.1.0) | stable (0.1.0) | **missing in TS** | none |
| `content-quote-card` | stable (0.1.1) | stable (0.1.1) | **missing in TS** | none |
| `content-media-grid` | stable (0.1.0) | stable (0.1.0) | **missing in TS** | none |
| `login-block` | stable (0.1.0) | **none** | **none** | none |
| `relay-list` | stable (0.1.0) | **none** | **none** | none |
| `user-avatar` | stable (0.1.0) | stable (0.1.0) | stable (0.1.0) | none |
| `user-name` | stable (0.1.0) | stable (0.1.0) | stable (0.1.0) | none |
| `user-nip05` | stable (0.1.0) | stable (0.1.0) | stable (0.1.0) | none |
| `user-npub` | stable (0.1.0) | stable (0.1.0) | stable (0.1.0) | none |
| `user-card` | stable (0.1.0) | stable (0.1.0) | stable (0.1.0) | none |

*A TUI source file for `content-minimal` **does exist** on disk (`tui/content-minimal/nostr_minimal_content.rs`, 174 lines), but the web registry TS file does **not** expose a TUI platform entry for it.

---

## 3. Source File Traceback & Line Counts

All source files referenced by the TS registry imports were verified to exist. Below is the complete trace with line counts.

### SwiftUI Source Files

| Registry Source Path | Lines |
|----------------------|-------|
| `swiftui/content-core/NostrContentRenderer.swift` | 190 |
| `swiftui/content-core/ContentTreeWire.swift` | 326 |
| `swiftui/content-minimal/NostrMinimalContentView.swift` | 293 |
| `swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift` | 24 |
| `swiftui/content-view/NostrContentView.swift` | 359 |
| `swiftui/content-view/NostrContentGrouping.swift` | 112 |
| `swiftui/content-view/Examples/NostrContentViewPreview.swift` | 49 |
| `swiftui/content-mention-chip/NostrMentionChip.swift` | 74 |
| `swiftui/content-quote-card/NostrQuoteCard.swift` | 250 |
| `swiftui/content-media-grid/NostrMediaGrid.swift` | 125 |
| `swiftui/login-block/NostrLoginBlock.swift` | 278 |
| `swiftui/relay-list/NostrRelayList.swift` | 249 |
| `swiftui/relay-list/Examples/NostrRelayListPreview.swift` | 44 |
| `swiftui/user-avatar/ProfileWire.swift` | 48 |
| `swiftui/user-avatar/NostrAvatar.swift` | 80 |
| `swiftui/user-name/NostrProfileName.swift` | 31 |
| `swiftui/user-nip05/NostrNip05Badge.swift` | 45 |
| `swiftui/user-npub/NostrNpubChip.swift` | 51 |
| `swiftui/user-card/NostrUserCard.swift` | 45 |
| **SwiftUI Total** | **2,673** |

### Compose Source Files

| Registry Source Path | Lines |
|----------------------|-------|
| `compose/content-core/NostrContentRenderer.kt` | 60 |
| `compose/content-core/ContentTreeWire.kt` | 283 |
| `compose/content-view/NostrContentView.kt` | 617 |
| `compose/content-view/NostrContentGrouping.kt` | 148 |
| `compose/content-mention-chip/NostrMentionChip.kt` | 102 |
| `compose/content-quote-card/NostrQuoteCard.kt` | 391 |
| `compose/content-media-grid/NostrMediaGrid.kt` | 305 |
| `compose/user-avatar/ProfileWire.kt` | 30 |
| `compose/user-avatar/NostrAvatar.kt` | 102 |
| `compose/user-name/NostrProfileName.kt` | 38 |
| `compose/user-nip05/NostrNip05Badge.kt` | 65 |
| `compose/user-npub/NostrNpubChip.kt` | 94 |
| `compose/user-card/NostrUserCard.kt` | 50 |
| **Compose Total** | **2,285** |

### TUI Source Files (Ratatui/Rust)

| Source Path | Lines | Exposed in Web TS Registry? |
|-------------|-------|----------------------------|
| `tui/content-core/content_render_data.rs` | 140 | **No** |
| `tui/content-core/content_tree_wire.rs` | 362 | **No** |
| `tui/content-minimal/nostr_minimal_content.rs` | 174 | **No** |
| `tui/content-mention-chip/nostr_mention_chip.rs` | 81 | **No** |
| `tui/content-media-grid/nostr_media_grid.rs` | 79 | **No** |
| `tui/content-quote-card/nostr_quote_card.rs` | 145 | **No** |
| `tui/content-view/nostr_content_view.rs` | 417 | **No** |
| `tui/user-avatar/profile_wire.rs` | 47 | Yes (`user.ts`) |
| `tui/user-avatar/nostr_avatar.rs` | 86 | Yes (`user.ts`) |
| `tui/user-name/nostr_profile_name.rs` | 41 | Yes (`user.ts`) |
| `tui/user-nip05/nostr_nip05_badge.rs` | 57 | Yes (`user.ts`) |
| `tui/user-npub/nostr_npub_chip.rs` | 50 | Yes (`user.ts`) |
| `tui/user-card/nostr_user_card.rs` | 68 | Yes (`user.ts`) |
| **TUI Total** | **1,747** | |

---

## 4. Missing Source Files Analysis

### Components with platforms listed in TS registry but missing disk files
**None.** Every file imported in `content.ts`, `user.ts`, and `relay.ts` exists on disk and was line-counted above.

### Components with source files on disk but missing from the TS registry
The following TUI content components exist in `crates/nmp-cli/registry/tui/` and are declared in `registry.toml`, but are **not imported or exposed** in the web registry TS files (`content.ts`, `relay.ts`):

- `tui/content-core` — `content_render_data.rs` (140 lines), `content_tree_wire.rs` (362 lines)
- `tui/content-minimal` — `nostr_minimal_content.rs` (174 lines)
- `tui/content-mention-chip` — `nostr_mention_chip.rs` (81 lines)
- `tui/content-media-grid` — `nostr_media_grid.rs` (79 lines)
- `tui/content-quote-card` — `nostr_quote_card.rs` (145 lines)
- `tui/content-view` — `nostr_content_view.rs` (417 lines)

Additionally, there is **no TUI `relay-list`** implementation on disk at all.

---

## 5. inFlight Components & Coverage Gaps

### inFlight Status
The `Component` type in `types.ts` supports an optional `inFlight?: boolean` field. **No component in `content.ts`, `user.ts`, or `relay.ts` is currently flagged as `inFlight: true`.** All listed implementations carry `status: "stable"`.

### Platform Coverage Gaps

#### Web Platform
The `Platform` union includes `"web"`, but **zero components** have a web implementation. No HTML/TSX/Vue/Svelte source files exist in `crates/nmp-cli/registry/`.

#### Compose Gaps (Content)
- **`content-minimal`** has no Compose implementation. Source file does not exist; TS registry correctly omits it.
- **`login-block`** has no Compose implementation.
- **`relay-list`** has no Compose implementation.

#### TUI Gaps (Content)
- **`content-core`** through **`content-view`** have TUI source files on disk but are **not surfaced** in the web registry TS.
- **`login-block`** has no TUI implementation.
- **`relay-list`** has no TUI implementation.

#### User Component Gaps
User components (`user-avatar`, `user-name`, `user-nip05`, `user-npub`, `user-card`) are the **only category with full tri-platform coverage** (SwiftUI + Compose + TUI) in the web registry.

#### Relay Component Gaps
- **`relay-list`** exists only for SwiftUI. No Compose or TUI variants exist on disk or in the registry TS.

---

## Summary

- **14 components** are cataloged in the web registry TS files.
- **All** TS-referenced source files exist and total **6,705 lines** across SwiftUI (2,673), Compose (2,285), and TUI (1,747).
- **No inFlight components** are declared.
- **Web platform is entirely unimplemented** (0 files, 0 components).
- **User components are the most complete** with stable implementations across all three non-web platforms.
- **Content components have hidden TUI implementations** (6 components, ~1,398 lines) that exist on disk and in `registry.toml` but are not imported into the web registry TS files.
- **Compose content-minimal**, **Compose login-block**, **TUI login-block**, and **all relay-list variants except SwiftUI** are genuinely missing.
