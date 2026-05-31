---
title: "Chirp Kind:0 Lazy Profile Backfill"
slug: chirp-kind0-lazy-backfill
summary: "Chirp lazily backfills kind:0 metadata by harvesting unknown p-tag pubkeys from timeline events (kinds 1 and 6) rather than aggressively subscribing to kind:0 f"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Chirp Kind:0 Lazy Profile Backfill

## Kind:0 Lazy Backfill Strategy

Chirp lazily backfills kind:0 metadata by harvesting unknown p-tag pubkeys from timeline events (kinds 1 and 6) rather than aggressively subscribing to kind:0 for every followee upfront. On launch, the kernel fetches kind:0 for the active account and any seed accounts as a cold-start bootstrap. The home feed subscription only asks for kinds 1 and 6 and does not subscribe to kind:0. retarget_timeline must guard against empty follow_feed_kinds to avoid emitting a malformed kinds:[] author REQ on the wire at sign-in. Profile mention reactivity is driven by the UI layer (the UI claims it needs a pubkey's kind:0 data), not pushed by the kernel. Tapping into ProfileView explicitly fetches kind:0 for that specific pubkey on demand. Kind:0 retrieval batches unknown pubkeys into a single REQ with kinds [0, 3, 10002] and a limit of batch.len() * 3. Because the snapshot only provides hex pubkeys without display_name, picture_url, or nip05, the TUI includes a client-side profile resolver that queries the kernel's profile cache separately. Content-mention profile discovery feeds nostr:npub mentions that appear only in note content into profile discovery, using a D8-clean contains("nostr:") fast-path that reuses the in-core parse_nostr_uri to avoid a nmp-content→nmp-core dependency cycle.

<!-- citations: [^fd809-2] [^4f377-3] [^d98be-2] [^42908-4] [^4edd4-3] -->
## See Also

