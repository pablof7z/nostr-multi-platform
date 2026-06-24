# Android naddr (kind:30023) claim does not resolve — same-instant repro

## Status: REAL bug, clean repro. The last unresolved cell (Android embed-article). Core deliverable already shipped without it.

## Same-instant evidence (NOT a time-shift artifact)
Fresh cold-start of the Android gallery (emulator-5554) on master `01a6cdfc` (#852 send-gate + #853 nos.lol naddr hint), built with `--features android-ffi` (14 JNI symbols verified). Navigated to ArticleEmbed and, AT THE SAME INSTANT, queried relay.primal.net:
- **primal.net returns the EVENT** for `{authors:[6e468422…], kinds:[30023], #d:[the-internet-left-me]}` (verified live, same instant).
- A prior socket dump (`ss -tnp` on the emulator) showed Android **ESTAB to relay.primal.net** while on this page.
- After 35s the article card STILL shows hex `@6e468422…ee93` + raw timestamp `1774042009`, NO title "What's left of the internet?", NO "Gigi".

So: a connected relay serves the kind:30023, Android is connected to it, yet the claim never resolves. This is NOT relay timing (the trap that mislead earlier hypotheses) — it is reproduced with the relay demonstrably serving the event at the same moment.

## Contrast that localizes it
- **nevent (kind:1 note 276d69d6…) RESOLVES on Android** via the same `model.claimEvent(uri, CONSUMER_ID)` path (EmbedComponentPages.kt:126 vs article :228). Note resolved in <3s against the connected primal.
- **naddr (kind:30023 article) does NOT resolve** on Android, even with primal connected + serving it.
- Both resolve on TUI (TUI smoke 2/2) — because TUI's primal connection + claim path handle the naddr. So the gap is Android-specific OR an addressable-claim path difference that Android happens to exercise.

The difference is event-id (hex64 primary_id) vs addressable coordinate (`kind:pubkey:d_tag` primary_id). `claimed_events` keys naddr entries by the coordinate string (docs/wiki/claimed-events.md:25). The inline PROSE renders fine ("hey, check out my article" / "I hope you enjoy it!") — only the card's typed projection (title/author) is unresolved. So it's a claim/projection resolution failure for addressable events, not a render failure.

## Hypotheses to instrument (do NOT assume — prior agents burned budget guessing)
1. The naddr claim REQ is never compiled/sent (the coordinate→InterestShape path differs from event-id). Check whether a kind:30023 REQ leaves Android on this page (the gallery surfaces kernel logs; or `ss`/relay-side).
2. The kind:30023 event arrives but `lookup_for_primary_id` / the `claimed_events` projection doesn't match it to the `kind:pubkey:d_tag` key (e.g. d_tag/coordinate mismatch, or addressable event stored in `self.store` but the projection's coordinate lookup misses).
3. An Android-specific JNI/decode difference in how the naddr URI is passed to the app-owned event URI adapter (the note nevent works, so URI plumbing is mostly fine — but naddr TLV parsing could differ).

Instrument-first: surface the kernel log + check whether a 30023 REQ is sent and whether the event lands in the store, on Android specifically. Compare to the TUI path which works. The TUI `--smoke` resolves the naddr (2/2) — so reproduce in a kernel/actor test if possible (claim naddr + inject matching kind:30023 via the real ingest path + assert `claimed_events[coord]` present) to see if it's kernel-universal or Android-bridge-specific.

## Decision pending (budget already heavily spent this session)
The core goal (comprehensive verified nmpui screenshots) is DELIVERED: iOS 16/16, TUI 16/16, Android 15/16, verification PDF merged. This is the last cell, on the diagnostic-secondary platform. Whether to spend another kernel investigation now vs. log it as a tracked follow-up is the user's call.
