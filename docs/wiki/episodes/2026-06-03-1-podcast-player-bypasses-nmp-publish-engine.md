---
type: episode-card
date: 2026-06-03
session: 13382c6f-c5ac-4856-99fd-3cbfdd0b06a5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/13382c6f-c5ac-4856-99fd-3cbfdd0b06a5.jsonl
salience: architecture
status: active
subjects:
  - nostr-relay-routing-ownership
  - podcast-player-publish-bypass
  - nip-crate-relay-sovereignty
supersedes: []
related_claims: []
source_lines:
  - 33-265
  - 287-356
captured_at: 2026-06-11T22:56:27Z
---

# Episode: Podcast-player bypasses NMP publish engine with raw relay capability

## Prior State

The podcast-player app manually assembled NostrRelayRequest::Publish with relay URLs read from app.configured_relays_handle(), falling back to hardcoded wss://relay.primal.net, and dispatched via a raw capability call — completely outside NMP's actor/publish engine. Four handlers (host_op_publish, social_publish, agent_note, comments) all used this pattern.

## Trigger

User noticed the app explicitly specifying relay URLs and asked 'why is this happening — this is an NMP concern for the most part.' Investigation revealed the podcast-player treats Nostr relay access as a raw capability (like HTTP) rather than as a managed NMP concern, bypassing the publish engine entirely.

## Decision

Confirmed as architectural doctrine: NIP crates own relay routing exclusively. Apps pass only protocol identity (GroupId, recipient_pubkey) — never relay URLs. Two seams already implement this: (1) synchronous pin via ActorCommand::PublishUnsignedEventToRelays for NIP-29, (2) actor-thread ProtocolCommand → PublishTarget::Explicit for NIP-17/Marmot. The podcast-player's NostrRelayCapability pattern must be replaced with dispatch_action("nmp.publish", ...) using PublishTarget::Auto for standard events, with NIP crates handling protocol-specific routing.

## Consequences

- Podcast-player's four handlers (host_op_publish, social_publish, agent_note, comments) that use NostrRelayRequest::Publish directly must be refactored to go through NMP's publish engine
- The hardcoded wss://relay.primal.net fallback must be removed
- All NIP-65 outbox routing, retry logic, offline intent tracking, per-relay outcome projection, and fail-closed guards are currently lost and will be restored once the bypass is eliminated
- The typed action inputs (PostChatMessageInput, SendDmInput) structurally foreclose app involvement — they have no relay field, so the fix is architecturally enforced at the type level
- Three fail-closed guards already prevent misuse: D10 rejects kind:1059+Auto, MissingHostPinForGroupEvent rejects unpinned h-tagged events, required_dm_relays rejects empty kind:10050 lists

## Open Tail

- Actual refactoring of podcast-player handlers to use dispatch_action("nmp.publish", ...) has not been done yet
- The duplicate relay_urls field in host_op_publish.rs (lines 3-4 of original snippet) suggests a partial/buggy edit that may need separate cleanup

## Evidence

- transcript lines 33-265
- transcript lines 287-356

