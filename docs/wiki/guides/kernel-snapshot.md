---
title: Kernel Snapshot
slug: kernel-snapshot
topic: ffi-runtime
summary: NMP is a data framework that delivers raw protocol data; apps own all rendering decisions
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-18
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:17ef19cd-8549-4fa9-b09c-5266aaf480a7
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:019edc00-f3a6-77f3-b21a-d6b45f5f6cab
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:019edc10-1fb3-7752-ab3e-7f5b969da686
  - session:019edc16-8e40-7a92-9ea1-7405af0d34f3
  - session:019edc63-ed50-7dc0-9f1a-38e311efc3b4
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Kernel Snapshot

## Structure & Projections

NMP is a data framework that delivers raw protocol data; apps own all rendering decisions. If an app decides to use shortNpub or Last4CharsOfNpub, that is the app's call. No native business logic is permitted: if an if statement in Swift, Kotlin, or any native language decides what the app should do (not how it should look), that logic belongs in Rust. Native code (Swift, Kotlin, TypeScript, etc.) is allowed to do exactly three things (see aim.md §2 #4 for the full rule): render Rust-produced state snapshots into UI; execute capabilities by calling OS APIs and reporting raw results back to Rust (never deciding policy, retrying, or caching); and hold ephemeral presentation state — purely local throwaway state that no other platform would have to reimplement to behave correctly (spinners keyed to correlation ids, scroll position, focus, input-buffer text, animation state, per-platform icon/color choices). The discriminating test: would a second platform have to reimplement this to stay correct? If yes → Rust (domain logic); if it is only how this platform shows or stages something → shell (presentation). All domain logic — state, business rules, derived data, routing decisions, error recovery, protocol logic — must live in Rust, not in native code. Every external effect must be represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state. New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams, and reducers must remain replayable from message history. KernelSnapshot has 15 fields (down from prior bloat); all app-noun state flows through the extensible projections map rather than typed kernel fields. The `kind` field is Rust-authoritative (a protocol signal), while Swift only decides display behavior, preserving the thin-shell rule with no app-noun fields leaked into the substrate type. FullState/full snapshot is the correctness path; granular ViewBatch or other delta variants may be added only when profiling proves the snapshot path is the bottleneck and the delta is lossless. The snapshot projection seam is asymmetric and output-only: hosts register pull-only closures; inbound state flows via dispatch_action, outbound via projections. The update callback receives snapshots wrapped as {"t":"snapshot","v":{...}}, so projections are accessed at v["v"]["projections"][key]. Debug/history surfaces must use log-safe action tags and correlation IDs; secrets, raw nsecs, plaintext DMs, and bearer tokens must never be recorded. The wallet_status field was removed from KernelSnapshot as a D0 violation; wallet state now surfaces through the host-registered 'wallet' snapshot projection (WalletStatusData lives in projections["wallet"]). WalletStatus was a catastrophic violation: wallet_npub_short was the only identifier on the wire — the raw hex lived only in the private WalletConnection struct and was never serialized. A wallet_pubkey_hex field was added. The ViewModule trait has been deliberately removed; per-protocol views use plain inherent methods via static dispatch. The kernel snapshot is already the UI component contract; no new FFI surface or Rust contract crate is needed for UI components.

<!-- citations: [^47203-10] [^1c093-16] [^17ef1-1] [^45fcf-4] [^64c4f-2] [^86221-3] [^019ed-15] [^019ed-45] [^019ed-52] [^019ed-63] [^019ed-100] -->
## Action Stages Projection

Action stages are projected via projections['action_stages'] keyed by correlation_id with stages requested/awaiting_capability/publishing/accepted/failed. The action_stages projection uses ack-based retention semantics, not per-tick emission, because a one-tick terminal can race Swift's pendingActions drain. Hosts call nmp_app_ack_action_stage(correlation_id) to drop the stage entry. <!-- [^1c093-17] -->

## Doctrine Lint (D14)

Any new Arc<Mutex<Vec<...>>> field on NmpApp, Kernel, or Actor* structs must register a paired SnapshotProjectionSlot namespace. The D14 scope is limited to these core structs only, not all Arc<Mutex<Vec<...>>> instances, which would incorrectly match test fixtures and mock signers. <!-- [^1c093-18] -->

## Projection Design Rules

docs/aim.md §2 anti-pattern #1 was corrected from 'Rust pre-formats into strings, native renders them' to: presentation formatting in the backend is banned; Rust sends raw data (pubkeys as hex, timestamps as Unix integers, display names verbatim from kind:0 with no truncation or fallback-npub substitution); presentation layers own all formatting decisions; display helpers are legitimate only in TUI render code, CLI output, and test fixtures — never in projection builders, snapshot types, or FFI serialization paths. SF Symbol names and English presentation formatting must not be baked into Rust projections inside platform-neutral nmp-core; they belong in shell-level code. KeyPackageStatus now emits raw published/age_secs/stale plus a new is_registered: bool, removing bucket_age/render_subtitle/action_label. DiscoveredGroup now emits raw name/group_id/public/open/member_count, removing display_name/initials/subtitle and finalize_display_fields. Projection structs must carry raw pubkeys (hex) and Option<String> for kind:0-derived display names — no short_npub fallbacks, no placeholder URIs, no pre-computed avatar initials/colors. Timestamps, group-name initials, member count labels, and unread badge text are also presentation-level concerns that should not be pre-formatted in Rust projections. The removed flat mirror fields author_display_name and author_picture_url from Nip10ReplyAttribution and npub from AuthorDisplay were redundant with the principle that projections must emit raw protocol data and shells format host-side (e.g. shells bech32-encode via nmp_app_encode_profile). The downstream Chirp parity test (apps/chirp/crates/nmp-app-chirp/tests/typed_feed_parity.rs:59,:62,:63) still constructs the removed AuthorDisplay.npub field and Nip10ReplyAttribution.author_display_name/author_picture_url fields, causing an E0560 compile failure on commit c10831484; it must be updated. New projection fields must have a same-PR Swift consumer (no orphan fields), per review #46's 'wire iOS consumer before any new feature' rule. The Clock injection pattern for projections is 'pass now_secs into projection construction.' WalletScreen.kt must not derive isConnected from walletTone in native code; Rust must project explicit wallet view affordances (e.g. walletIsConnected, showConnectForm, showDisconnect, showBalance) that native renders verbatim. SignInScreen.kt (Android) and AccountsView.swift (iOS) must not branch on signerKind for labels or state filtering; Rust must project signerSectionLabel and per-signer applicability affordances that native renders without semantic branching. ExternalSignerCapabilityBridge.kt transport selection (Intent vs ContentResolver) and concurrent-Intent rejection are acceptable mechanical host execution constrained by Android OS IPC limits, not native policy violations; Rust must own retry/queue/user-facing consequences.

<!-- citations: [^45fcf-5] [^86221-4] [^019ed-16] [^019ed-101] [^11850-71] -->
## Specific Projections

The `projections["mention_profiles"]` derived view (aim.md §4.2) eliminates the same three-Dictionary rebuild that recurs in HomeFeedView, ProfileView, and ThreadScreen (~30 LOC across 3 surfaces). MentionProfilePayload had its pubkey only as a HashMap key (not a struct field), making values unusable without the enclosing map. A pubkey field was added to the struct. HomeFeedView and ThreadScreen should adopt the now-live mention_profiles projection (landed by ProfileView PR) to eliminate their identical Dictionary rebuilds. DmConversation, GroupChatMessage, and MarmotMessageRow carry only raw pubkeys; pre-formatted display fields (peer_short_npub, author_display, sender_short, etc.) are violations of separation of concerns and were removed. MarmotMessageRow was the only struct where raw hex pubkey was completely absent (only had sender_npub in bech32); a sender_pubkey_hex field was added before removing the display-string fields. WalletStatus was a catastrophic violation: wallet_npub_short was the only identifier on the wire — the raw hex lived only in the private WalletConnection struct and was never serialized. A wallet_pubkey_hex field was added. Signer classification via switch kind.lowercased() in AccountsView.swift:100,147 violates aim.md §4.4; the kernel should provide account.is_remote: bool and account.signer_label: String on the AccountSummary snapshot. New Rust projections added by the 10-agent wave: relay_diagnostics, outbox_summary, settings_hub, mention_profiles, nip46_onboarding, plus ProfileAction.dispatch, Nip29GroupChatMessage display fields, MarmotGroupRow display fields, account signer classification, DM is_outgoing, ThreadViewPayload.{previous,next}_count_label.

<!-- citations: [^45fcf-6] [^86221-5] -->
