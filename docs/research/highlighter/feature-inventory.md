# Highlighter Feature Inventory — User-Visible Surface for M11.5

> **Status:** Research notes, 2026-05-18. Companion to `app-survey.md` (the source tree map) and `../../design/nip29-crate.md` (the protocol crate this inventory feeds).
> **Purpose:** Exhaustive enumeration of user-visible features the M11.5 rebuild must reach parity with. Each bullet is tagged with one of three scope labels:
>
> - **[N29]** — NIP-29-bearing. Requires `nmp-nip29` to work. M11.5-critical.
> - **[N29-adj]** — NIP-29-adjacent. Interacts with groups but its primary protocol is something else (e.g. NIP-84 highlight shared *into* a group via kind:16). Requires `nmp-nip29` *at the boundary* but the bulk of the feature lives in another crate.
> - **[N29-ind]** — NIP-29-independent. The feature ships on its own protocol stack and would work even if NIP-29 were absent. Listed for completeness because M11.5 must reach UI parity with the whole app.
>
> Bullets in this file are designed to be countable; the final summary line gives the total.

## 1. NIP-29-bearing features (the critical-path surface)

### 1.1 Room discovery + browse

- **[N29]** Room explorer view that lists public rooms hosted on the configured host relay, paginated and sorted by member count (`Communities/RoomExplorerView.swift` + `RoomExplorerStore.swift` → `groups.rs` + relay query for public 39000s).
- **[N29]** "Friends on this room" annotation per explorer row — overlay of which of the user's NIP-02 follows appear in the room's 39002 (`Communities/FriendsOnRoomCard.swift` + `groups.rs` + `follows.rs`).
- **[N29]** "Browse all" deep view — full paginated grid of every visible room (`Communities/RoomBrowseAllView.swift`).
- **[N29]** Room preview sheet — read-only peek at name/about/picture/member count/admins before joining (`Communities/RoomPreviewSheet.swift`).
- **[N29]** Hero card on the communities tab spotlighting a featured room (`Communities/ExplorerHeroView.swift`).
- **[N29]** Square-tile + cover-card rendering of a room with picture + name + member badge (`Communities/RoomSquareTile.swift`, `RoomCoverCard.swift`).
- **[N29]** Joined-rooms list on the user's profile / home — projection of "which rooms am I in" derived from 39001/39002 entries containing the user's pubkey (`groups.rs::query_joined_communities_from_ndb` + `build_joined_communities`).

### 1.2 Joining + membership flow

- **[N29]** "Request to join" action from preview / explorer — publishes kind:9021 with `h=group_id`, then waits for the relay's 39002 update (`groups.rs::publish_join_request`).
- **[N29]** Optimistic UI: "Join requested" toast that promotes to "You're in" when the matching 39002 arrives via the subscription pump.
- **[N29]** Membership-state recognition on every render — derives `is_admin`, `is_member` for the current user from the latest 39001/39002 events (`groups.rs::build_summary`).
- **[N29]** Leave-room flow (publishes kind:9022 — *currently missing from `groups.rs`* but referenced in NIP-29 spec; M11.5 must add).
- **[N29]** Invite-code redemption at sign-up — paste/scan an invite link, redeem against the relay via kind:9021 carrying the `code` tag (referenced in `groups.rs::create_invite_codes` for the mint side; redeem side currently lives in iOS auth flow / referenced by `RoomShareCard.swift`).

### 1.3 Admin / moderation

- **[N29]** Create-room sheet: name + about + picture (uploaded via Blossom) + visibility (public/private) + access (open/closed) (`Communities/CreateRoom/CreateRoomSheet.swift` → `groups.rs::create_room` which fires kind:9007 then kind:9002 back-to-back).
- **[N29]** Edit-metadata action — change name/about/picture/visibility/access on an existing room (kind:9002; admin-only). UI surface exists in settings drawer per `groups.rs::create_room`'s metadata helper but the standalone edit flow is partial.
- **[N29]** Add-member action — admin invites a known pubkey directly via kind:9000 put-user (`groups.rs::add_member`).
- **[N29]** Remove-member action — admin kicks a member via kind:9001 remove-user (*currently missing from `groups.rs`*; M11.5 must add).
- **[N29]** Mint invite codes — admin pre-generates N single-use codes via kind:9009 (`groups.rs::create_invite_codes`), automatically fans out across multiple events when `count > MAX_CODES_PER_INVITE_EVENT = 10`.
- **[N29]** Promote / demote admin — currently emulated via `put-user` with a `role` tag (`KIND_PUT_USER` adds to 39001 if role tag present); explicit promote/demote UI absent today.
- **[N29]** Delete-event moderation — admin removes an offending message via kind:9005 (*not yet wired in Highlighter*; M11.5 should plan for it since NIP-29 supports it and it's a frequent request).

### 1.4 Invite distribution

- **[N29]** Invitee picker after room creation: pull the user's NIP-02 follow list, render a multi-select with names+pictures, plus a paste-an-npub fallback (`Communities/CreateRoom/RoomInviteView.swift`).
- **[N29]** Bulk add — selected invitees are added via `add_member` (kind:9000) per pubkey; the relay materialises one 39002 update per put-user.
- **[N29]** Shareable invite link / QR — wraps an invite code in a deep-link URI rendered as a share card (`Communities/CreateRoom/RoomShareCard.swift`).
- **[N29]** Reusable code paste at any point — auth screen accepts an invite code as the *first* onboarding action (`Features/Auth/` flow).

### 1.5 In-room conversation

- **[N29]** Group chat — flat conversational kind:9 messages scoped via `["h", group_id]` (`Communities/ChatView.swift` + `ChatStore.swift` → `chat.rs::query_chat_messages` + `publish_chat_message`).
- **[N29]** Reply marker — chat messages can carry `["e", <target>, "", "reply"]` and render inline-threaded; chat.rs surfaces this via `reply_to_event_id` (`chat.rs::reply_target`).
- **[N29]** Author header collapsing — consecutive messages from the same author render without repeating the header (`ChatView.swift::shouldShowHeader`).
- **[N29]** Ascending-time order — chat view appends at the bottom; `chat.rs::query_chat_messages` returns ascending-by-`created_at`.
- **[N29]** Reactions on chat messages — via NIP-25 kind:7 with the chat message in the `["e", …]` target (`reactions.rs` consumed from chat view).
- **[N29]** Discussion threads — kind:11 long-form discussions scoped to the room, with title + body + image attachments (`Communities/DiscussionListView.swift`, `DiscussionDetailView.swift`, `DiscussionComposerView.swift` → `discussions.rs`).
- **[N29]** Discussion list ordering — discussions sort by latest-reply timestamp (computed from kind:1111 NIP-22 comments that target the kind:11).
- **[N29]** Discussion replies — NIP-22 (kind:1111) tree under each kind:11 root (`comments.rs`).

### 1.6 In-room shared artifacts (the unique Highlighter primitive)

- **[N29]** Artifact detail — view of a shared artifact (article / book / podcast / web bookmark) inside the room context, with member highlights overlaid (`Communities/ArtifactDetailView.swift` + `RoomLibraryArticleCardView`, `RoomLibraryBookCardView`, `RoomLibraryPodcastCardView`).
- **[N29]** Room library lanes — the room home view's "what people are reading here" lanes per artifact type (`Communities/RoomLanesView.swift`, `RoomHomeView.swift`).
- **[N29]** Artifact registry per room — derived from kind:16 reposts of NIP-84 highlights tagged with the room's `h`; the `artifacts.rs` projection groups highlights by their reference (`a` tag for articles, `i` tag for ISBNs, `r` tag for podcast URLs).

## 2. NIP-29-adjacent features

### 2.1 Share to room

- **[N29-adj]** "Share highlight to community" action from a highlight detail view — publishes a kind:16 generic repost with `["h", target_group_id]` (`highlights.rs::share_to_community`). The highlight itself stays on the author's write relays; only the share event routes to the room's host relay.
- **[N29-adj]** "Publish and share" combined action — captures a new highlight (kind:9802) on the user's write relays *and* fans out a kind:16 share into one room in a single user gesture (`highlights.rs::publish_and_share`). Two separate `send_event` calls under the hood with different routing targets — load-bearing example of the dual-routing problem `nmp-nip29` must solve cleanly.
- **[N29-adj]** Target-room picker UI — list of rooms the user is in, surfaced from the highlight composer (`Capture/` flow → `groups.rs::query_joined_communities_from_ndb`).

### 2.2 Cross-protocol reactions / comments inside rooms

- **[N29-adj]** Reactions on group artifacts — Highlighter today emits NIP-25 kind:7 *without* an `h` tag (verified per `reactions.rs::publish_reaction` — only `e`/`p`/`k` tags). They're public reactions to the underlying artifact, owned by `nmp-nip25`, routed per the author's NIP-65 write relays. M11.5 preserves this for parity. The `nmp-nip29::GroupReaction` / `ReactInGroup` shapes defined in `nip29-crate.md` are the *future-proof path* — once Highlighter's reaction composer (post-M11.5) starts attaching `h`, the unifying ownership rule moves the reaction into `nmp-nip29` automatically.
- **[N29-adj]** Comments on group artifacts — same parity story as reactions: Highlighter's `comments.rs::publish_comment` emits NIP-22 kind:1111 with `E`/`e` scope tags only, no `h`. Today's comments live in `nmp-nip22`, routed per NIP-65. `nmp-nip29::GroupComment` / `CommentInGroup` are the future-proof shapes for when (and only when) Highlighter starts attaching `h` to in-room comments.
- **[N29-adj]** Group-scoped comment threads on artifacts — discussion view embeds the (currently non-h-tagged) NIP-22 comments scoped to the artifact's `A:`/`E:`/`I:` reference (`Comments/` consumed inside `RoomStore.swift::commentsByReference`). The `DiscussionsWithReplyCounts` view in `highlighter-core` (per `nip29-crate.md` §6) composes `nmp-nip29::GroupDiscussions` + `nmp-nip22::Comment` to surface reply counts and latest-reply ordering.

## 3. NIP-29-independent features (full app parity surface)

Listed at one bullet per coherent feature so the count reflects the real scope of the rebuild. The crate that owns each in NMP is in parens.

### 3.1 Highlight capture (NIP-84, kind 9802)

- **[N29-ind]** Capture from article URL — fetch web metadata, render preview, select text to highlight (`Capture/CapturePageView.swift` + `web_metadata.rs` + `highlights.rs`) (→ `nmp-nip84`).
- **[N29-ind]** Capture from PDF — pick a PDF from Files, extract text, highlight (`Capture/` PDF flow).
- **[N29-ind]** Capture from book — search by ISBN, render book card, highlight a passage (`Capture/BookPicker.swift` + `isbn_lookup.rs`).
- **[N29-ind]** Capture from podcast episode — paste an episode URL, attach a timestamp + transcript snippet (`Capture/` podcast flow).
- **[N29-ind]** Share-extension capture — iOS share sheet entry point captures a URL into the same flow without launching the main app (`ShareExtension/`).
- **[N29-ind]** Blossom upload — highlight context images stored on a Blossom server, URL embedded in the kind:9802 (`blossom.rs`).

### 3.2 Reading + feeds

- **[N29-ind]** Article reader — NIP-23 long-form with markdown rendering (`Features/Article/ArticleReaderView.swift` + `articles.rs` + `MarkdownRenderer.swift`) (→ `nmp-nip23`).
- **[N29-ind]** Web reader — non-NIP-23 articles via web-metadata fetch + simplified-reader-mode rendering (`Web/WebReaderView.swift`).
- **[N29-ind]** Unified reads feed — interleaves articles + podcasts + books "currently reading" (`Reads/` + `reads.rs`).
- **[N29-ind]** Highlight feed — global / following / per-author highlight stream (`Highlights/HighlightFeedCardView.swift` + `highlights.rs`).
- **[N29-ind]** Highlight detail — single highlight, full context, reactions, comments (`Highlights/HighlightDetailView.swift`).
- **[N29-ind]** Hydrated highlight rendering — `HydratedHighlight` joins a highlight to its source artifact + author profile (`models.rs::HydratedHighlight`).

### 3.3 Bookmarks + lists

- **[N29-ind]** NIP-51 bookmark sets (kind:10003) — manage saved articles + URLs (`Bookmarks/` + `bookmarks.rs`) (→ `nmp-nip51`).
- **[N29-ind]** Curation sets (kind:30004) — user-curated lists of NIP-23 articles (`curation.rs` + `lists.rs`).
- **[N29-ind]** Web bookmark sets — non-NIP-23 URLs in a NIP-51 list (`lists.rs::KIND_WEB_BOOKMARK`).

### 3.4 Profile + social

- **[N29-ind]** User profile (NIP-01 kind:0) — name, about, picture, lud16, banner (`Profile/ProfileView.swift` + `profile.rs`) (→ `nmp-nip01`).
- **[N29-ind]** Follow list (NIP-02 kind:3) (`follows.rs`) (→ `nmp-nip02`).
- **[N29-ind]** Profile screen tabs — highlights, articles, joined rooms, reads, books (`Profile/` view modules).

### 3.5 Podcasts (Podcast 2.0 + RSS)

- **[N29-ind]** Podcast subscription + feed fetch (`Podcast/` + RSS module — not in core today; referenced through artifacts).
- **[N29-ind]** Episode rows + listening view + player (`Podcast/PodcastListeningView.swift`, `PodcastPlayerStore.swift`, `Rows/`).

### 3.6 Onboarding + signing

- **[N29-ind]** Paste-nsec onboarding — local key into Keychain (`Auth/` + `session.rs`) (→ NMP M6 surface).
- **[N29-ind]** Create-new-nsec onboarding — generate, encrypt (NIP-49), store (`Auth/` + scope-adjustments.md M6 fold).
- **[N29-ind]** Bunker:// paste — paste a `bunker://` URL, rendezvous (`Auth/` + `nip46.rs`) (→ NMP M6 bunker surface).
- **[N29-ind]** nostrconnect:// emit — app generates a `nostrconnect://` URI for the user to scan from their bunker (`nip46.rs::DEFAULT_NOSTR_CONNECT_PERMS`).
- **[N29-ind]** Permission scope display — show which kinds the bunker will be asked to sign (per `DEFAULT_NOSTR_CONNECT_PERMS`).

### 3.7 Settings + relay management

- **[N29-ind]** Relay list view + per-relay role toggles (read/write/rooms/indexer) (`Settings/Network/` + `relays.rs`) (→ `nmp-nip65` + `nmp-nip78`).
- **[N29-ind]** Per-relay status + health — connected/disconnected/auth-paused/error (`relay_polish.rs` + ADR-0007 diagnostics).
- **[N29-ind]** Settings catalog — themes, notifications, default highlight visibility (`Settings/SettingsView.swift`).

### 3.8 In-app feedback (the dogfood loop)

- **[N29-ind]** Feedback thread list + detail + composer — kind:1 + kind:513 threads scoped to a kind:31933 project address; routes to `wss://feedback-relay.highlighter.com` (`Feedback/` + `feedback.rs`). Highlighter-specific surface; not user-relevant for most M11.5 demos, but listed because it's part of UI parity.

### 3.9 Search

- **[N29-ind]** Cross-entity search — highlights + articles + users + rooms in a single result list (`Search/SearchView.swift` + `search.rs`).

### 3.10 What's new + meta

- **[N29-ind]** What's new sheet — one-per-cold-launch changelog from bundled JSON (`WhatsNew/` + `Resources/whats-new.json`).
- **[N29-ind]** Deep linking + URL handling — opens nostr URIs, invite links, share-target URLs (`Navigation/`).

## 4. Summary

**Bullet count by scope label** (count one feature = one bullet):

- **[N29] NIP-29-bearing:** 34 bullets across §§ 1.1–1.6
- **[N29-adj] NIP-29-adjacent:** 6 bullets across §§ 2.1–2.2
- **[N29-ind] NIP-29-independent:** 32 bullets across §§ 3.1–3.10

**Total feature bullets: 72.**

The split (34 N29 / 6 adjacent / 32 independent) confirms the survey's claim that NIP-29 is one slice of Highlighter, not the whole app — but it's the *new* slice that justifies the M11.5 milestone existing at all. The 34 N29-bearing features are the surface area `nmp-nip29` must support; the 6 adjacent features are the dual-routing test cases that prove the host-relay-pin design integrates cleanly with NMP's existing outbox planner.

The 32 independent features confirm M11.5 needs every protocol crate planned through M10 (nip01/02/22/23/25/51/65/68/78/84, plus blossom, plus the bunker signer surface from M6) to be in place. M11.5 cannot start before M10 ends; the brief already reflects this (M11.5 follows M11 in the ladder).
