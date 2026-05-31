---
title: Profile Claim Batched Fetch Path
slug: profile-claim-batch-path
summary: P-tags must be routed through the batched profile claim path rather than the capped discovery path, allowing them to be fetched in one batched REQ per relay wit
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-28
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
  - session:47882225-939f-4978-bf5a-8feb9e5ef029
---

# Profile Claim Batched Fetch Path

## Batched Profile Claim Path

P-tags must be routed through the batched profile claim path rather than the capped discovery path, allowing them to be fetched in one batched REQ per relay without a concurrency cap. Whenever any pubkey is rendered on screen, an attempt must be made to fetch kind:0 from indexer relays and, once kind:10002 is known, from the pubkey's own write relays. The presentation layer (Swift, Kotlin, TUI, etc.) must drive profile fetching by calling claim_profile when it needs to render a pubkey, not the tokenizer or kernel parsing content. Data fetching uses a frontend-driven claim pattern where the renderer calls bridge.claim_profile(pubkey, consumer_id) on every Poll tick, relying on the kernel to deduplicate claims per (pubkey, consumer_id). EventClaimSink provides a claim_profile(pubkey, consumer_id) method with a default no-op implementation for backward compatibility. Profile claims must be sent by UI rows on `.onAppear` for all pubkeys they need to render (authors, mentions, p-tags, embedded card authors) and released on `.onDisappear` by calling release_profile to clean up the profile subscription. claim_profile fires every 250ms poll tick so kind:0 fetch sticks once a relay connects. The discovery module must lift the `MAX_DISCOVERY_CONCURRENCY = 2` cap for profile fetches while keeping the cap for event fetches. When kind:10002 arrives and a pubkey's actual write relays become known, kind:0 must be re-fetched from those relays (refresh_profile_after_mailbox). When an indexer relay closes, requested-but-unfulfilled profile pubkeys must be moved back to pending so they are re-batched on the next reconnect. LiveKernelSink implements claim_profile as the EventClaimSink trait method, with the inherent method deleted to prevent silent shadowing; a compile guard test enforces this.

<!-- citations: [^7b4ae-5] [^19e07-15] [^86221-10] [^d98be-6] [^6e8af-6] [^47882-4] -->
## Cold Start Rehydration

On cold start, the kernel must rehydrate `kernel.profiles` from LMDB so that previously resolved profiles are immediately available without re-fetching from the wire. [^7b4ae-6]

## Mention and Sender Resolution

NIP-17 DM senders and NIP-29 group chat senders must be rendered using the `mentionProfiles` lookup instead of showing raw hex. The kernel must tokenize note content to discover `nostr:npub1…` mentions that lack a corresponding `p` tag so their profiles can be claimed. Kind:7 reaction authors must trigger a profile claim instead of falling through the default ingest arm with no claim. When rendering a Mention(uri) token, NostrContentView calls sink.claim_profile(&uri.primary_id, consumer) (deduped via claimed_this_frame) and resolves the profile name from live_profiles, falling back to the user's shortened npub (e.g., @npub1xxxx…xxxx) while in-flight, never a synthetic string like "Resolved Profile". profile_value() emits a null display_name rather than falling back to the hex pubkey, preventing the unresolved state from being masked.

<!-- citations: [^7b4ae-7] [^d98be-7] -->
## Profile Struct Extensions

The `Profile` struct must be extended to include `lud16`, `banner`, and `website` fields to prevent future kind:0 surface gaps. The kernel snapshot includes a claimed_profiles projection as a BTreeMap<pubkey_hex, MentionProfilePayload> built from existing profile_claims keys once kind:0 data arrives. EmbedHostState reads claimed_profiles from the snapshot and exposes a profiles() method returning BTreeMap<String, ContentProfileRenderData>.

<!-- citations: [^7b4ae-8] [^d98be-8] -->
## See Also

