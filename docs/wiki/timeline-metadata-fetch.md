---
title: Timeline Metadata Fetch
slug: timeline-metadata-fetch
topic: marmot
summary: "The home feed subscription starts when the shell sends `ActorCommand::OpenContactListSubscription { kinds: BTreeSet<u32> }` (e.g"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-26
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
  - session:c4b2e655-ca6b-42d2-9383-89bf52215d0a
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:64f3e239-c4c1-4c32-82de-458516b28418
---

# Timeline Metadata Fetch

## Follow Feed Subscription

The home feed subscription starts when the shell sends `ActorCommand::OpenContactListSubscription { kinds: BTreeSet<u32> }` (e.g. Chirp passes `{1, 6}`), causing the kernel to resolve the active account's follows from its own cached kind:3 and register per-author interests. (Previously: the shell sent `ActorCommand::OpenTimeline`, a unit variant with no payload.) The C ABI symbol `nmp_app_open_timeline` remains byte-for-byte unchanged; Swift, Kotlin, and TUI call sites are untouched — Chirp's `{1, 6}` is declared inside that function.

Hardcoding `kinds: {1, 6}` inside the kernel's `follow_feed_interest()` is a D0 violation — social-app knowledge baked into the substrate that prevents other apps from subscribing to different kinds over a user's contact list. V-45 is reworded to: 'No substrate seam for NIP-02 contact-list author expansion — kernel hardcodes kinds {1,6}' with action to add `LogicalInterestSource::ContactListAuthors { viewer, kinds }` and replace `OpenTimeline` with `OpenContactListSubscription { kinds }`.

The new abstraction is `LogicalInterestSource::ContactListAuthors { viewer: Pubkey, kinds: BTreeSet<u32> }` — a source type that expands into existing `LogicalInterest`s, not a new variant on the `LogicalInterest` struct. Auto-including the viewer in the author set is app policy, not kernel contract; the kernel must not auto-include the viewer, and apps wanting the viewer's own events should register a separate `Direct` interest. Lifecycle for `ContactListAuthors` is fixed as `Tailing` and scope as `Global`; per-author limit (200) stays kernel-internal until a real use case demands a field.

On `ContactListAuthors` registration and on every kind:3 ingest where `event.pubkey == viewer`, the kernel resolves the viewer's follow set from `seed_contacts`; a missing kind:3 resolves to the empty set (CLEAR semantics, not no-op). Each per-author interest from `ContactListAuthors` expansion routes through the existing NIP-65 outbox planner (Case A), requiring no new compiler work. Per-author InterestIds derive from `(tag, viewer, kinds_hash, author_pubkey)` so that two apps registering different `kinds` over the same viewer don't collide or clobber each other's interests.

NMP supports an 'authors:[current-users-follows]' query abstraction so that apps can request a follow-based feed without manually handling kind:3, kind:10002, relay connections, or pubkey filter construction. Following accounts automatically sets up timeline and outbox REQ subscriptions to retrieve followed authors' posts without any Swift-side logic.

TimelineItem carries a `kind` field (u32 in Rust, UInt32 in Swift) populated from `event.kind`, allowing Swift to branch on `item.kind == 6` for reposts instead of relying on fragile JSON heuristics. The Swift `TimelineItem` Decodable struct uses `.convertFromSnakeCase`, so no CodingKeys override is needed for the new `kind` field.

The kernel stores `follow_feed_kinds: BTreeSet<u32>` which starts empty; when empty, the subscription is off — interests withdrawn and author cache cleared. The host re-declares kinds on timeline re-entry; on `ActorCommand::Reset`, the field resets to empty.

`ingest_contacts()` is called on both Inserted and Replaced outcomes, so a Tailing kind:3 replacement from another client correctly triggers `sync_follow_feed_interests()` and `FollowListChanged`.

<!-- citations: [^fd809-4] [^17ef1-4] [^5d180-1] [^c4b2e-9] [^6e4c3-3] [^64f3e-6] -->
## Kind:0 Metadata Fetching

The presentation layer (Swift, Android, TUI, etc.) drives profile fetching by calling claim_profile for each pubkey it renders, rather than the kernel parsing content to trigger fetches. Whenever any pubkey appears on screen (as note author, content mention, or any render context), the system attempts to fetch kind:0 metadata from indexer relays and, once kind:10002 is known, from the pubkey's own write relays. When kind:10002 (mailbox) data arrives for a pubkey, the system re-fetches kind:0 from that pubkey's own write relays, since the earlier kind:0 from the indexer may be stale.

The collect_unknown_refs and request_profile_for_rendered_note functions are wired exclusively to ingest_timeline_event (kind:1/6). NIP-17 DM peers, NIP-29 group chat senders, Marmot MLS messages, and NIP-02 follow list members bypass them entirely. Opening a profile (ProfileView) explicitly fetches kind:0 for that specific pubkey on demand. On cold-start, startup_requests fetches the active user's own kind:3, profile, and relay list instead of emitting seed-bootstrap, seed-contacts, seed-profiles, or seed-relays for a hardcoded trio. The timeline excludes hardcoded seed pubkeys (fiatjaf, jb55, pablof7z) from timeline_authors after the active account's kind:3 arrives, but the active user's own pubkey is included so they see their own posts. should_open_timeline gates on the active account's contacts being available, not on three seed contact lists. The status line reports 'Timeline' instead of 'SeedTimeline(fiatjaf,jb55,pablof7z)'. sign_in_nsec, create_account, and switch_active reconcile the M2 follow-feed and emit bootstrap REQs for the new active account so the follow feed works when login occurs after cold-start. create_account prepopulates seed_contacts with DEFAULT_FOLLOWS before calling reconcile_follow_feed_after_identity_change to close the race condition where the published kind:3 event is not locally ingested. A published post from the user's account must appear on the timeline without manual refresh.

Two coexisting subscription systems exist in the codebase: InterestRegistry and the M1 hand-rolled `req()`.

<!-- citations: [^95d02-16] [^fd809-5] [^5d180-2] [^c4b2e-10] [^17ef1-3] [^86221-9] -->
## Kind:6 Repost Rendering

Kind:6 reposts in the Chirp timeline render the inner note's text content with a "Repost" badge, rather than displaying raw JSON. The previous `effectiveContent` heuristic in NoteRowView detected reposts by checking for a `sig` field in the embedded JSON, which failed when relays or clients stripped the `sig` field from reposts. A potential follow-up is to have the kernel emit a `contentTree` for the inner event of a kind:6 repost so the body renders with entity decoration rather than plain text. <!-- [^17ef1-5] -->
