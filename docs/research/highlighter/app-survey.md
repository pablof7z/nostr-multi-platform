# Highlighter App Survey — Source-of-Truth Reading for M11.5

> **Status:** Research notes, 2026-05-18. Companion to `feature-inventory.md` (what the app does) and `docs/design/nip29-crate.md` (the protocol crate that falls out of this survey).
> **Source tree:** `/Users/pablofernandez/Work/hl/app` (`AGENTS.md` at the root sets the working contract).
> **Reading order:** `app/AGENTS.md` → `app/core/src/lib.rs` (module wiring) → `app/core/src/groups.rs` + `app/core/src/chat.rs` + `app/core/src/highlights.rs` + `app/core/src/subscriptions.rs` (NIP-29 + content surfaces) → `app/ios/Highlighter/Sources/Highlighter/Features/Communities/*` (iOS UI rendering those surfaces).

## 1. Why this survey exists

M11 (podcast app) proves the kernel boundary for a *non-Nostr* product. M11.5 (Highlighter) proves the boundary for a *Nostr-shaped* product that exercises a **second non-trivial protocol** (NIP-29) on top of the existing social stack. The brief from `docs/plan/scope-adjustments-2026-05-18.md` is explicit:

> Builds the second non-social-domain extension app on top of the kernel, after the podcast app proves the boundary. Source: `/Users/pablofernandez/Work/hl/app` (Highlighter — already has a Rust core + native UIs; the rebuild ports it onto NMP's substrate + codegen + view-module/action-module pattern). Adds NIP-29 as a first-class protocol module crate (groups, moderation, joining flow, kind 39000–39003 metadata events) done in NMP-idiomatic shape.

Highlighter differs from the podcast app in one load-bearing way: it **already has a working Rust core**. The M11.5 step-1 is therefore not "Rust port" but "re-architect onto NMP's substrate, deleting Highlighter's hand-rolled actor/subscription pump/cache". Same UI fidelity bar as M11, fundamentally different mid-layer work.

## 2. Tech-stack inventory

From `app/AGENTS.md` and the directory survey:

| Layer | Path | Status today | M11.5 disposition |
|---|---|---|---|
| **Rust core** | `app/core/src/` (~19,900 LOC across 38 files) | Built on `nostr_sdk` + `nostrdb` + `uniffi`; hand-rolled subscription pump (`subscriptions.rs`, 2,155 LOC) and client actor (`client.rs`, 1,956 LOC) | **Re-architect onto NMP substrate.** Each module ports to one of `DomainModule` / `ViewModule` / `ActionModule`. The hand-rolled subscription pump deletes — NMP's subscription compiler (M2) is the single source of truth for relay routing. The hand-rolled client actor deletes — NMP's kernel actor is the single source of truth. |
| **iOS app** | `app/ios/Highlighter/Sources/Highlighter/` (~39,200 LOC across 142 Swift files) | SwiftUI + `@Observable` stores per feature; consumes the Rust core via the `uniffi`-generated Swift module | **Copy verbatim** (M11 copy-step doctrine). Only the data source changes — Rust calls go from the existing `SafeHighlighterCore` shim to NMP's generated `@RoomChat` / `@JoinedGroups` / etc. wrappers. |
| **Android app** | *not present* (`AGENTS.md` references `app/android` but the directory is absent from this checkout) | n/a | M11.5 ships iOS only. Android lives in M15 once cross-platform code-gen is in (`docs/plan.md` §M15). |
| **Desktop app** | *not present* (`AGENTS.md` references `app/desktop`) | n/a | Same — M15. |
| **Shared FFI** | `app/core/src/` is `uniffi::setup_scaffolding!();` (lib.rs:1) | All `uniffi::Record` / `uniffi::Enum` types are generated for Swift+Kotlin | M11.5 uses NMP's raw C FFI initially (per M10.5 exit gate); UniFFI migration is M14. |

**Implication for scope.** The survey area is `app/core/src/` + `app/ios/Highlighter/Sources/Highlighter/`. Other directories named in `AGENTS.md` (Android, Desktop) are absent and out-of-scope for M11.5. The web app referenced in `groups.rs`'s "ports `web/src/lib/ndk/groups.ts`" comment lives in a separate Highlighter monorepo (`/Users/pablofernandez/Work/hl` root has `app/` + sibling `web/`) and is not part of this rebuild.

## 3. Rust core — module-by-module map

LOC counts from `wc -l` on each file as of the checkout snapshot. The table groups modules by what they do, not by alphabetical order, so the NIP-29 surface is visible as a clump.

### 3.1 Protocol-layer modules

| File | LOC | Purpose | NIP / Kinds |
|---|---|---|---|
| `groups.rs` | 820 | NIP-29 group metadata, membership, room creation, invite mint, join request | **NIP-29.** Kinds 9007 (create-group), 9000 (put-user), 9002 (edit-metadata), 9009 (create-invite), 9021 (join-request), 39000 (metadata), 39001 (admins), 39002 (members) |
| `chat.rs` | 292 | NIP-29 chat (`kind:9` scoped by `["h", group_id]` tag) | **NIP-29.** Kind 9. |
| `discussions.rs` | 361 | NIP-29 threaded discussions inside a group (`kind:11` marked `["t","discussion"]`) | **NIP-29.** Kind 11 (note: kind 11 is widely used elsewhere; the `t=discussion` marker is Highlighter convention layered on NIP-29 routing). |
| `highlights.rs` | 1,347 | NIP-84 highlights + cross-group sharing via kind:16 generic repost | NIP-84 (kind 9802) + NIP-18 (kind 16). The repost path is the NIP-29-adjacent bridge: a highlight gets reposted *into* a NIP-29 group via its `["h", group_id]` tag. |
| `articles.rs` | 324 | NIP-23 long-form (kind:30023) reading + reading-feed projection | NIP-23. |
| `reads.rs` | 580 | "Reads" — what the user is reading, projection of articles + podcasts + books + reading lists | NIP-23 + Podcast 2.0 + non-Nostr book lookups. |
| `bookmarks.rs` | 303 | NIP-51 bookmark sets (kind:10003) | NIP-51. |
| `lists.rs` | 407 | NIP-51 generic lists (kind:30000 + 30001 + 30003) including curation sets + web bookmark lists | NIP-51. |
| `curation.rs` | 367 | Curation-set helpers on top of `lists.rs` | NIP-51. |
| `comments.rs` | (small) | NIP-22 comments (kind:1111) | NIP-22. |
| `reactions.rs` | (small) | NIP-25 reactions (kind:7) | NIP-25. |
| `feedback.rs` | 828 | In-app feedback threads (kind:1 + kind:513) scoped to a kind:31933 project address | NIP-72-adjacent (custom kinds for feedback). |
| `follows.rs` | 287 | NIP-02 contact list (kind:3) | NIP-02. |
| `profile.rs` | 293 | NIP-01 user metadata (kind:0) | NIP-01. |
| `pictures.rs` | 233 | NIP-68 picture-first feeds (kind:20) | NIP-68. |
| `relays.rs` | 633 | NIP-65 read/write relay list + NIP-78 app-data for rooms/indexer roles | NIP-65 (kind 10002) + NIP-78 (kind 30078). |
| `nip46.rs` | 576 | NIP-46 bunker signer client | NIP-46. |
| `nostr_entities.rs` | 424 | nip-19 bech32 + nip-21 URI parsing | NIP-19, NIP-21. |
| `blossom.rs` | 421 | Blossom upload/download | Blossom (not a NIP). |
| `web_metadata.rs` | 656 | OG-tag / Open Graph metadata fetch + cache for arbitrary URLs | n/a (HTTP). |
| `isbn_lookup.rs` | 395 | ISBN → book metadata (non-Nostr; uses an external book DB API) | n/a (HTTP). |
| `recent_books.rs` | 511 | Books recently captured in highlights, projected from ISBN-tagged highlights | n/a (derived). |

**NIP-29 footprint:** ~1,500 LOC of the ~19,900-LOC core (≈7.6 %). The bulk of Highlighter is **non-NIP-29**. This matters for scope-honesty: M11.5 introduces `nmp-nip29` as a crate but the *Highlighter rebuild* exercises every protocol crate (`nmp-nip01`, `nmp-nip02`, `nmp-nip23`, `nmp-nip51`, `nmp-nip65`, `nmp-nip78`, `nmp-nip84`, `nmp-blossom`) plus the new `nmp-nip29`. Most of those exist (or will, after M2–M10).

### 3.2 Infrastructure-layer modules

These delete-or-port-to-kernel in M11.5; they are the ones NMP's substrate replaces.

| File | LOC | What it does today | M11.5 disposition |
|---|---|---|---|
| `client.rs` | 1,956 | Top-level `HighlighterCore` actor; owns the `Client` (nostr_sdk), ndb cache, signer, runtime; hosts every public method exposed to FFI | **Delete.** NMP's kernel actor (`crates/nmp-core/src/kernel/`) is the substitute. The methods become `ActionModule::dispatch` impls + `ViewModule::project` reads, generated by `nmp-codegen`. |
| `subscriptions.rs` | 2,155 | Hand-rolled subscription pump: per-view subscription handles, REQ assembly, EOSE handling, delta routing back to FFI callbacks | **Delete.** NMP's subscription compiler (M2 design, `docs/design/subscription-compilation.md`) is the substitute. Each Highlighter view becomes a `ViewModule` whose `dependencies()` produces `LogicalInterest`s the compiler routes. |
| `nostr_runtime.rs` | 1,328 | tokio Runtime + Client lifecycle + relay-status reporting + signer install | **Delete.** Kernel owns the runtime; signer lives in `IdentityModule`; relay status lives in the diagnostics surface (ADR-0007). |
| `outbox.rs` | 374 | Per-author NIP-65 mailbox cache + routing helpers | **Delete.** NMP's M2 outbox planner is the substitute. The Highlighter logic ports as a *consumer* of `MailboxesViewModule`, not as its own implementation. |
| `cache.rs` | (small) | nostrdb wrapper conveniences | **Delete.** NMP's M3 LMDB persistence is the substitute; the `nostrdb` dependency leaves with it. |
| `relay_polish.rs` | 220 | Relay-status presentational helpers (status badges, retry hints) | **Port to UI.** This is presentation, not protocol — moves into the Swift layer or into a `ViewModule` projection. |
| `events.rs` | (small) | `Delta` + `DataChangeType` enums that the FFI callbacks emit | **Delete.** Replaced by NMP's `ProjectionChange` deltas (substrate/view.rs:17). |
| `session.rs` | 230 | Current-user + signer state | **Port to `IdentityModule`** (M6/M8 surface). |
| `errors.rs` | (small) | `CoreError` enum used across the crate | **Adapt.** Each ported module uses NMP's per-module error type pattern. |
| `models.rs` | 494 | Public typed records (`CommunitySummary`, `HighlightRecord`, etc.) | **Port as `DomainRecord` types** owned by their respective extension crates (e.g., `nmp-nip29` owns `Group`, `Membership`; `nmp-nip84` owns `Highlight`). |
| `nip46.rs` | 576 | Bunker signer client | **Delete.** NMP's M6 surface includes bunker; if NDK's helper is richer, we may keep this as reference until M6 lands. |
| `discovery.rs`, `recommendations.rs`, `search.rs` | varies | App-level projections (room explorer, "who you should follow", search index) | **Port as `ViewModule`s** in `highlighter-core` (the app's own extension crate). |

**Bottom line:** ~6,000 LOC of `app/core/src/` (`client.rs` + `subscriptions.rs` + `nostr_runtime.rs` + `outbox.rs` + small infra) **deletes outright** because NMP's substrate provides that machinery once, framework-wide. That deletion is the single biggest validation of the M11.5 doctrine claim.

## 4. iOS surface — feature-folder map

From `app/ios/Highlighter/Sources/Highlighter/Features/`. File counts per folder were measured with `find … | uniq -c`.

| Folder | Files | What it renders | Drives which Rust modules |
|---|---|---|---|
| `Communities/` | 21 | **NIP-29 surface:** Room explorer, room home, chat, discussion list + detail + composer, artifact detail, friends-on-room card, room cover/tile cards, room library cards | `groups.rs`, `chat.rs`, `discussions.rs`, `artifacts.rs`, `comments.rs`, `reactions.rs`, `highlights.rs` (shared via `share_to_community`) |
| `Communities/CreateRoom/` | 3 | **NIP-29 admin surface:** Create-room sheet (name/about/picture/visibility/access), invite picker (mint codes + paste to invite known followers), share card (invite link/qr) | `groups.rs::create_room`, `groups.rs::create_invite_codes`, `groups.rs::add_member` |
| `Capture/` | 16 | Highlight capture: PDF/article/book/podcast capture flows + book picker + share-to-room target picker | `highlights.rs`, `articles.rs`, `isbn_lookup.rs`, `web_metadata.rs`, `blossom.rs`, `groups.rs` (target list) |
| `Highlights/` | 6 | Highlight feed cards + highlight detail + highlight cross-share UI | `highlights.rs`, `comments.rs`, `reactions.rs` |
| `Article/` | 6 | Article reader (NIP-23) + markdown renderer + highlight-inline overlay | `articles.rs`, `highlights.rs`, `web_metadata.rs` |
| `Book/` | 2 | Book detail view + book reading state | `recent_books.rs`, `isbn_lookup.rs` |
| `Podcast/` | 8 (+ `Rows/` 5) | Podcast listening + episode rows + player store | non-Nostr (Podcast 2.0 + RSS) |
| `Reads/` | 3 | Unified "what I'm reading" feed (articles + podcasts + books) | `reads.rs` |
| `Bookmarks/` | 4 | NIP-51 bookmarks UI | `bookmarks.rs`, `lists.rs` |
| `Profile/` | 6 | User profile + their highlights/articles/communities | `profile.rs`, `highlights.rs`, `follows.rs` |
| `Search/` | 4 | Search across highlights/articles/users/rooms | `search.rs` |
| `Comments/` | 9 | NIP-22 comment trees + composer | `comments.rs`, `reactions.rs` |
| `Feedback/` | 6 | In-app feedback threads (the dogfood loop) | `feedback.rs` |
| `Settings/` | 3 + `Network/` 6 | App settings + relay management UI | `relays.rs`, `session.rs` |
| `Auth/` | 6 | Onboarding: paste nsec / create new / bunker:// paste / nostrconnect:// scan | `nip46.rs`, `session.rs` |
| `Share/` | 3 | Share-extension target (the iOS share sheet's "Save to Highlighter") | `highlights.rs`, `web_metadata.rs` |
| `Web/` | 2 | In-app web reader for non-NIP-23 articles | `web_metadata.rs` |
| `WhatsNew/` | 1 | Bundled changelog sheet | non-Nostr |

**NIP-29 share of UI surface:** 24 of 142 Swift files (~17 %). Per-LOC the share is similar — `Communities/` is the third-largest feature folder by file count, behind `Capture/` and tied with `Highlights+Comments+Article` once those are summed.

## 5. Cross-cutting infrastructure in iOS

The Swift layer also has:

- `Core/Generated/` — UniFFI-generated Swift bindings (regenerate on Rust API change)
- `Core/Design/` — design tokens, typography, color palette (port verbatim per M11 copy-step)
- `Core/RichText/` — markdown + nostr-uri renderer (consumes `nostr_entities.rs`)
- `Session/` — session-management glue (`SafeHighlighterCore`, the actor-bound wrapper around the Rust core for SwiftUI concurrency)
- `Navigation/` — root tab + deep-link routing
- `Resources/` — assets + `whats-new.json` + onboarding copy

`Session/SafeHighlighterCore` is the bridge between async SwiftUI and the Rust core. In the NMP rebuild it becomes the bridge to NMP's generated `@DomainName` / `@ViewName` wrappers — the file *survives* the rebuild, only its implementation changes.

## 6. Disposition summary for M11.5

| Layer | Action |
|---|---|
| Rust core, ~13,100 LOC of protocol + view logic | **Port** as `ViewModule` / `ActionModule` impls in extension crates (`nmp-nip29`, `nmp-nip84`, `nmp-nip23`, `nmp-nip51`, `nmp-nip78`, plus `highlighter-core` for app-specific projections) |
| Rust core, ~6,000 LOC of infra (`client`, `subscriptions`, `nostr_runtime`, `outbox`) | **Delete** — NMP substrate replaces this once, framework-wide |
| iOS app, all 142 Swift files | **Copy verbatim** (M11 step-0) → **rewire** data sources to generated NMP wrappers (M11 step-4) |
| Android + Desktop apps | **Out of scope** for M11.5; covered by M15 cross-platform |

The next file (`feature-inventory.md`) enumerates the user-visible features each layer ships, separated by whether they are **NIP-29-bearing** (require the new `nmp-nip29` crate), **NIP-29-adjacent** (interact with groups but their primary protocol is something else), or **NIP-29-independent** (the rest of the app). That split is the scope contract for the M11.5 doctrine claim.

## 7. References (verbatim file paths for the rebuild agent)

- `app/AGENTS.md` — read first; defines tech-stack + build commands
- `app/core/src/lib.rs` — module wiring
- `app/core/src/groups.rs` — full NIP-29 surface, well-commented
- `app/core/src/chat.rs` — kind:9 group chat, well-tested
- `app/core/src/discussions.rs` — kind:11 threaded group discussions
- `app/core/src/highlights.rs` — NIP-84 + kind:16 share-to-group bridge
- `app/core/src/subscriptions.rs` — to-delete; reference for "what subscription shapes does Highlighter need"
- `app/core/src/relays.rs` — NIP-65 + NIP-78 routing with hardcoded `HIGHLIGHTER_RELAY` (the host-relay-pin we generalize in `nmp-nip29`)
- `app/ios/Highlighter/Sources/Highlighter/Features/Communities/RoomStore.swift` — reference reactive store pattern (the shape that NMP's `@JoinedGroups` wrapper must match)
- `app/ios/Highlighter/Sources/Highlighter/Features/Communities/ChatView.swift` — reference for the chat UI's data dependencies
- `app/ios/Highlighter/Sources/Highlighter/Features/Communities/CreateRoom/CreateRoomSheet.swift` — reference for create-room flow
- `app/ios/Highlighter/Sources/Highlighter/Features/Communities/CreateRoom/RoomInviteView.swift` — reference for invite-mint + invitee-picker flow
