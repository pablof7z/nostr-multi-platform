---
title: NIP-65 Outbox Expansion Gap for Cold-Start nevent Claims
slug: nevent-cold-start-outbox-expansion-gap
summary: When an nevent claim comes from a cold start, Phase 2 of claim-expansion never seeds candidates from the author's NIP-65 outbox write relays, causing claims to exhaust even though the event is reachable.
tags:
  - nmp-gallery
  - kernel
  - claim-expansion
  - outbox
  - nevent
  - nip-65
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# NIP-65 Outbox Expansion Gap for Cold-Start nevent Claims

> When an nevent claim comes from a cold start, Phase 2 of claim-expansion never seeds candidates from the author's NIP-65 outbox write relays, causing claims to exhaust even though the event is reachable.

Root Cause

Phase 2 candidate queue is built only from URI-provided relay hints, on the assumption that 'Phase 1 already covered the author's outbox.' This assumption holds for author-addressed claims (naddr, kind:0, profile — the planner knows the author up front and routes Phase 1 to their NIP-65 write relays). But for an nevent (event-id) claim, Phase 1 only knows the event id plus any embedded relay hint. When the hinted relay EOSE's empty, Phase 2 has nothing left and the claim goes directly to terminal_exhausted without ever attempting to discover the author's NIP-65 write relays. [^6a951-88]

Verified Example

pablof7z's kind:1 note (nevent1…) embeds the relay hint relay.primal.net, which does not carry the event. purplepag.es (the seeded indexer) serves pablof7z's kind:10002 listing write relays 140.f7z.io, pyramid.fiatjaf.com, and r.f7z.io — all of which actually have the note. But Phase 2 never fetches the kind:10002 and never seeds those write relays. The claim logs confirmed: Phase 1 hit primal.net, got empty EOSE, then 'phase1 → terminal_exhausted' with no kind:10002 probe or Phase 2 expansion. [^6a951-89]

Design Decision: nevent Cannot Reliably Use Outbox

For an nevent, outbox expansion is not a reliable path because the bech32 may not include an author pubkey. The correct designed behavior for nevent claims is to follow the relay hint first. If the relay hint points to a relay that has the event, the claim succeeds. If not, the claim exhausts — and the fix is at the data layer: use nevents with relay hints that point to relays known to carry the events. [^6a951-90]

Relay-Hint Fix for Showcase References

The showcase nevents in showcase-references.json originally embedded relay.primal.net as their relay hint, but primal.net lacks the showcase note and highlight events. The events exist on nos.lol. The fix is to re-encode the nevents using nak with nos.lol as the relay hint. This is a data fix, not a kernel change. The re-encoded nevents were verified via nak decode to ensure correct event IDs and author. [^6a951-91]

Verification Evidence

After the relay-hint fix, the kernel claim log showed the note resolving via nos.lol: 'ReqEmit relay=wss://nos.lol has_hint:true' → 'EventRx event_id=276d69d6… from nos.lol' → 'terminal_hit'. The kernel-level claim path works correctly when the relay hint points to a relay that has the event. [^6a951-92]

Related: claimed_events Projection Gap for kind:1

Even after the relay-hint fix ensures the kernel receives the EVENT, a separate kind:1 projection gap was discovered: the note arrives at the kernel level (EventRx + terminal_hit) but does not appear in the claimed_events projection. The event goes through ingest_timeline_event which gates on should_store_event. While should_store_event line 236 explicitly handles claimed events via claim_expansion_match_author, the note's appearance in the projection was not confirmed. The article (kind:30023, addressable) lands via get_param_replaceable correctly; the note (kind:1) takes a different store/lookup path that may not populate the projection. This gap is under active investigation. [^6a951-93]


## Relay-Hint Fix for Showcase References

The naddr article relay hint must use a wss:// relay URL, not an HTTP blog URL, so the kernel can actually dial it for addressable claims. [^6a951-124]
## See Also
- [[showcase-relay-data-reachability|Showcase Relay Reachability — Data Lives on nos.lol, Not Default Seeds]] — related guide
- [[claimed-events|claimed_events Snapshot Projection]] — related guide

