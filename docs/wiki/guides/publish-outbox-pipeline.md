---
title: Publish Outbox Pipeline
slug: publish-outbox-pipeline
topic: crate-architecture
summary: publish_outbox_status() must check for any Ok relay state before checking for Pending state, so that a partially-succeeded publish (some relays Ok, some Pending
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-19
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
  - session:7174d4d4-371b-4b8e-87a6-91024c2b4c2a
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:019edbff-8164-7a20-abc2-c977bc495d49
  - session:019edc10-1fb3-7752-ab3e-7f5b969da686
  - session:019edc18-81c6-72c3-91c4-47fbff9f8f43
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# Publish Outbox Pipeline

## Status Priority

publish_outbox_status() must check for any Ok relay state before checking for Pending state, so that a partially-succeeded publish (some relays Ok, some Pending) shows as 'queued' rather than 'pending'. (Previously: The user initially believed reactions staying Pending was caused by a 'running=false' race condition, but investigation revealed the publish_outbox_status priority bug and p-tag fanout behavior.) <!-- [^fbebb-1] -->

## Relay Disconnection Handling

When a relay disconnects (PoolEvent::Failed or PoolEvent::Closed), mark_relay_unavailable() reverts any InFlight state back to Pending, causing a publish to appear stuck even if another relay already accepted the event. <!-- [^fbebb-2] -->

## P-Tag Fanout

The Nip65OutboxResolver adds each p-tagged recipient's kind:10002 read relays to the publish target set (p-tag fanout), limited to events with fewer than 15 distinct p-tagged recipients. This fan-out condition must exclude discovery-kind events (such as kind:3), because p-tags in kind:3 events represent follows, not message recipients. Step 4 (recipient inbox fan-out) must be guarded with `!is_discovery_kind(kind)` so that kind:3 events do not publish to p-tagged follows' inbox relays.

<!-- citations: [^fbebb-3] [^e6b44-3] [^e6b44-7] -->
## Outbox Resolver Trait

The OutboxResolver trait must return Vec<ResolvedRelay> (containing url and reason) instead of BTreeSet<RelayUrl> so that relay selection rationale is preserved through the publish pipeline. When the same relay URL is selected via multiple code paths (e.g., both author write relay and indexer relay), the first reason wins and subsequent duplicates are ignored during deduplication. RelaySelectionReason is a structured enum (not human-readable strings) threaded through the internal pipeline; display formatting is isolated to format_relay_reason() in kernel/publish_outbox.rs at the wire boundary.

<!-- citations: [^fbebb-4] [^7174d-5] -->
## Relay Reason Labels

Nip65OutboxResolver annotates all 5 code paths with structured RelaySelectionReason enum variants, and the engine deduplicates by canonical URL, merging distinct reasons with '; '. The five code paths produce these enum variants: NIP-65 write relay for author kind:10002 write relays, App relay (local config) for local_write_relays fallback, Discovery indexer (kind {n}) for indexer relays, Inbox relay for {short_npub} for p-tag recipient read relays, and Explicit relay for PublishTarget::Explicit.

<!-- citations: [^fbebb-5] [^7174d-3] -->
## InFlight State

The InFlight struct must carry a relay_reasons map (BTreeMap<RelayUrl, String>) populated once at publish time and never mutated by retry logic. relay_reasons is write-once at publish time and survives availability cycles, retry transitions, and process restarts, persisted on PublishRecord.

<!-- citations: [^fbebb-6] [^7174d-4] -->
## PublishOutboxRelay Schema

PublishOutboxRelay must include a relay_reason string field with serde(default, skip_serializing_if = 'String::is_empty') for backwards compatibility, so older payloads and resumed-from-store publishes remain forward-compatible.

<!-- citations: [^fbebb-7] [^7174d-6] -->
## Implementation Order & PR Split

The per-relay publish rationale feature is implemented across backend (Steps 1–4), TUI (Steps 5–6), and iOS (Step 7), developed in parallel agents within a single git worktree on branch feat/per-relay-publish-rationale. The implementation must be executed in this order: (1) ResolvedRelay struct & trait change & test stubs, (2) Nip65OutboxResolver annotation, (3) InFlight.relay_reasons & start_publish_inner split, (4) PublishOutboxRelay.relay_reason & projection update, (5) chirp-tui snapshot parsing, (6) chirp-tui UI detail pane. Steps 1–4 (all backend, zero shell changes) form a single PR, steps 5–6 are a chirp-tui PR on top, and iOS picks up the new field in a separate one-liner PR.

P1 publish_outbox and relay_diagnostics wire-shape changes must wait until PR #1525 (snapshot-projector JSON escape-hatch removal) merges, then rebase, because they collide heavily on types.rs/generated-bindings.

<!-- citations: [^11850-17] [^fbebb-8] [^7174d-2] [^11850-42] [^11850-101] [^11850-142] [^11850-166] [^11850-195] [^11850-218] [^11850-244] -->
## Outbox UI

The outbox UI surfaces per-relay reasons for why each relay was targeted when publishing events (e.g. 'NIP-65 write relay', 'Inbox relay for npub1abc…').

publish_outbox must emit raw kind/content/status/attempt/counts; iOS SF Symbol names, English title/preview/status_label/attempt_label/summary are removed from nmp-core and owned by shells (iOS NotificationsView+OutboxRow and TUI shell).

<!-- citations: [^7174d-1] [^11850-141] [^11850-217] -->
## Terminal Outcome & History

TerminalOutcome carries relay_reasons through to RelayAckOutcome so that publish_queue history rows retain the per-relay rationale after eviction from publish_outbox. <!-- [^7174d-7] -->

## iOS OutboxRelayRow Rendering

iOS OutboxRelayRow renders relay.relayReason as a .caption2 / .secondary line below the URL row, guarded with if !relay.relayReason.isEmpty. <!-- [^7174d-8] -->

## iOS PublishOutboxRelay Decoding

iOS PublishOutboxRelay uses a custom init(from:) with decodeIfPresent for relayReason, defaulting to empty string, to handle kernels that omit the field. <!-- [^7174d-9] -->

## PublishQueueEntry Title

PublishQueueEntry.title is a pre-formatted string field populated by the kernel at push_publish_entry sites; chirp-tui reads it verbatim and the bespoke publish_kind_label() function was deleted. <!-- [^7174d-10] -->

## LOC Ceiling Compliance

All changed files are under the 500-LOC V-12 ceiling: engine.rs at 452, feature_snapshot.rs at 496, settings.rs at 234, publish_engine.rs at 460, publish_outbox.rs at 403. <!-- [^7174d-11] -->

## publish_engine_terminals Module Declaration

publish_engine_terminals.rs is declared via #[path] inside publish_engine.rs rather than as a module in kernel/mod.rs, to avoid touching the pre-existing 1903-LOC kernel/mod.rs violation. <!-- [^7174d-12] -->

## Silent Fallback Relays

FALLBACK_CONTENT_RELAY and FALLBACK_INDEXER_RELAY activate silently when relay rows are empty, causing users to publish to unconsented relays. <!-- [^cd2b6-11] -->

## Orphaned NIP-47 Payments

NIP-47 pending_payments with no timeout sweep can become orphaned, causing payments to appear stuck forever. <!-- [^cd2b6-12] -->

## Explicit Targets & RoutingContext

The publish path has two parallel explicit-relay mechanisms: the live PublishTarget::Explicit (PublishPlan → ActorCommand::PublishUnsignedEventToRelays → publish_signed_to → PublishTarget::Explicit) and the dead RoutingContext::explicit_targets (built but never populated in mailboxes.rs). NIP-29 PublishPlan correctly routes via the live PublishTarget::Explicit seam and should not be rerouted onto the dead RoutingContext::explicit_targets. The dead RoutingContext::explicit_targets seam vs the live PublishTarget::Explicit must be triaged as a follow-up: either delete the dead seam or migrate PublishTarget::Explicit to route through route_publish. The generic OutboxRouter must not carry a per-NIP kind→EventClass classification table; the explicit-targets publish path routes through RoutedRelaySet::from_explicit and attributes uniformly as ClassRouted{Other("explicit"), Explicit} regardless of event kind. The routing relay-set decision (URL selection) for the explicit-targets path uses ctx.explicit_targets verbatim minus blocked relays; the only behavioral change is the RoutingSource class attribution label, not the relay set itself. Issue #1538 was filed as a follow-up to track the unification (presenting Option A: delete the dead seam, and Option B: migrate PublishTarget::Explicit through route_publish), rather than fixing it in this campaign because no publish-path lane exists. (Previously: P7 findings #3/#4 required filing a follow-up issue for an architecture call on unifying the publish-side explicit-relay mechanisms; TODO(#1493) existed to thread an explicit_class: Option<EventClass> through RoutingContext.)

<!-- citations: [^019ed-2] [^019ed-64] [^11850-16] [^11850-40] [^11850-81] [^11850-102] [^11850-140] [^11850-164] [^11850-193] [^11850-216] [^11850-243] -->
## Rate-Limited Ack Classification

The NIP-20/NIP-01 rate-limited relay-ack code must be classified as Transient (retried with backoff), not Permanent (give-up-immediately); PoW correctly stays Permanent because the engine cannot add PoW without re-signing into a different event id. Rate-limited was in PERMANENT_CODES in publish/state.rs; PR #1539 reclassifies it as Transient and corrects the publish-ack classifier to agree with closed_reason.rs which already maps rate-limited to ERR_TRANSIENT. The kernel wire helper doc in publish_engine_wire.rs must be reworded to policy-neutral wording, since the previous doc grouping rate-limited under 'permanent classes' was a doc-lie that contradicted the new classification.

<!-- citations: [^019ed-54] [^11850-41] [^11850-82] [^11850-103] [^11850-139] [^11850-165] [^11850-194] [^11850-215] [^11850-242] -->
## Generic OutboxRouter Cleanup

The nmp-core test-only router must be updated to remove its own classify_kind/explicit_set_for_kind per-NIP table to match the production GenericOutboxRouter change, or the D0 cleanup is incomplete. Module-level lane-5 docs in router.rs and test module header docs in tests_lanes.rs must be updated to remove stale claims that the router classifies evt.kind into Search/Draft/Wiki/Other. <!-- [^019ed-65] -->

## Kind 443 Dual-Publish Deadline

The kind:443 dual-publish deadline has expired (was through 2026-05-31; as of 2026-06-18 it is past), requiring action. <!-- [^11850-18] -->

## Relay Bypass for DM Inbox

Nip17DmRelay (RoutingSource::Nip17DmRelay) must be included in relay_bypasses_selection so DM-inbox relays are never pruned by the NIP-65 outbox optimizer. A gift-wrap DM inbox relay carries an empty-author wildcard sub-shape giving zero coverage score, and under a large follow set the DM inbox relay is silently pruned, causing the user to stop receiving DMs. PR #1532 (P7) fixes this with 3 regression tests.

<!-- citations: [^11850-39] [^11850-80] [^11850-163] [^11850-214] [^11850-241] -->

## Local Publish Consistency

A locally-published event must flow through the same EventIngestDispatcher →
projection fan-out pipeline as relay-received events, ensuring read-your-writes
consistency from the moment a signed event hits the local store. <!-- [^e6b44-4] -->
