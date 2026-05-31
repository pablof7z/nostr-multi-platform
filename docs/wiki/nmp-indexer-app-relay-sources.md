---
title: NMP Indexer & App Relay Sources
slug: nmp-indexer-app-relay-sources
summary: "Indexer relays are always-additive for kind:0, kind:3, and kind:10000–19999 for both read and write, independent of NIP-65 mailbox state"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-25
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:41858cd2-3a5d-4ad1-bdd0-4cbe1df2dd9d
  - session:bbafe8a2-8814-4625-83b6-6af3d4ec0412
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:50510273-d1c9-424a-b877-179d52fba557
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
---

# NMP Indexer & App Relay Sources

## Indexer Relays

No relay is hardcoded for discovery; the app provides an indexer relay instead. The `ActorCommand::Start` command and `nmp_app_start` FFI accept an `indexer_relays: Vec<String>` parameter (passed as CSV) supplied by the app. The `BOOTSTRAP_DISCOVERY_RELAYS` constant is removed, and diagnostic uses of `CONTENT_RELAY_URL`/`INDEXER_RELAY_URL` are replaced with projection-sourced URLs. The hardcoded `purplepag.es` URL in `subs/mod.rs:211` is seeded from the Start command instead of being constant; Chirp and NmpPodcast both pass `"wss://purplepag.es"` as their indexer relay. The kernel's `bootstrap_urls_for_role(role)` reads from the app-provided `relay_edit_rows` config and returns URLs matching the requested role, returning empty if nothing is configured. `bootstrap_discovery_relays()` returns the union of indexer and content URLs from the app-provided `relay_edit_rows`. `spawn_missing_relays` reads relay configuration from the kernel config instead of hardcoded constants, spawning only Indexer workers from the app-provided list with no Content bootstrap. All production relay resolution call sites (outbox fallback routing, profile requests, startup requests, status projections, and subscription lifecycle) read relays from kernel config instead of hardcoded constants. The `all_relays_connected` startup gate relies on Indexer-only connectivity, allowing Content relays to join on-demand rather than blocking the initial REQ burst. Indexer relays are strictly discovery-direction relays for kind:0, kind:3, and kind:10002, and never receive content (e.g. kind:1) REQs. Published events of discovery kinds (kind:0, kind:3, and kind:1xxxx) are additionally routed to the author's configured indexer relays. When an event qualifying for indexer storage (kind:0, kind:3, or kind:1xxxx) is retrieved from a non-indexer relay, it must be republished (without resigning) to connected indexer relays. Before republishing, `store.provenance_for(event_id)` must be checked; if any provenance entry is already from an indexer relay, the republish must be skipped. The indexer republish pipeline must be optional and default to enabled; it must live in nmp-core (the NMP layer), not in the app template layer. The indexer is never a content relay unless an author independently declares it in their own kind:10002. If an author has no NIP-65 mailbox and no app relay, that author is not queried for content. NMP reads kind:10086 (NIP-51 Indexer relays) for the spec-compatible event shape but applies the relays to a broader kind range (kind:0, kind:3, kind:10002) than the NIP-86 spec scopes them to. When a REQ targets a pubkey with no cached and not-yet-attempted kind:10002, the core auto-emits a kinds:[10002] discovery REQ to the indexer set (implicit mailbox probing). ingest_relay_list fires CompileTrigger::Nip65Arrived on every kind:10002 that replaces a mailbox, so a successful discovery triggers a recompile that routes the author via their declared NIP-65 relays. An author who never published kind:10002 is probed exactly once per session (the empty EOSE still marks them in a session-sticky probed_mailboxes set); clear_probed_mailboxes() is the refresh escape hatch. Relay state (`indexer_relays`, `local_write_relays`, `RelayEditRow`) is actor-owned via `IdentityState::set_relay_edit_rows` (`kernel/identity_state.rs:198-231`); however, they remain raw shared primitives rather than typed projections, which is the outstanding design debt. Nip65OutboxResolver holds an Arc<Mutex<Vec<String>>> for indexer relays that the kernel keeps current via set_relay_edit_rows on relay config changes; the publish path at `publish/nip65/mod.rs:64,68` bypasses the actor to read `indexer_relays` and `local_write_relays` directly through this Mutex rather than via an actor message.

<!-- citations: [^41858-7] [^bbafe-1] [^fc128-3] [^fd809-4] [^50510-3] [^1c093-17] [^86221-6] -->
## App Relays

App relays are operator-configured at kernel construction and user-mutable via a settings surface showing relays with their roles (e.g., Indexer, App). MarmotBridge reads relay configuration from the app's relay config instead of using a hardcoded array of relays. On the Swift side, KernelModel seeds `wss://r.f7z.io` (role: both) and `wss://purplepag.es` (role: indexer) before `kernel.start()` if `relayEditRows` is empty. App relays serve as a fallback when a user has no NIP-65 relay list and are additive alongside NIP-65 relays even when an author has NIP-65 relays (e.g. a publish goes to author writes ∪ app relays). For a REQ with no authors and no #p tags (e.g. hashtag firehose), the relay set is the logged-in user's NIP-65 read relays union app relays. For a plain publish, the relay set is the author's NIP-65 write relays union app relays. When a read targets an author with no NIP-65, the system reads from app relays if any exist, otherwise fails so the app can handle it. When a write targets an author with no NIP-65, the system writes to app relays if any exist, otherwise fails so the app can handle it. If app_relays is empty and an author has no other relays, that author is silently unroutable (no fallback). Authors with no NIP-65 mailbox and no app relay are collected into an unroutable_authors set on CompiledPlan, surfaced as a kernel-observable diagnostic.

<!-- citations: [^41858-8] [^bbafe-2] [^fd809-5] -->
## NIP-17 DM Relays

NIP-17 gift-wrapped DMs require a set recipient inbox relay or no DMs are sent for that pubkey (fail-closed). PrivateToRecipients events never include the indexer relay set.

<!-- citations: [^bbafe-3] [^50510-4] -->
## See Also

