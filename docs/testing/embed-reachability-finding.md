# Embed "loading forever" — RESOLVED (two fixes)

All three nmp-gallery event embeds (note, article, highlight) now resolve end-to-end on iOS and TUI, verified by reading actual rendered text (not redacted screenshots).

## Two root causes, two fixes

### Fix 1 — kernel claim-race (#843, MERGED to master `653d96d2`)
A claim's REQ fans out to multiple relays sharing one `sub_id`. `complete_unknown_oneshot` released the claim on the FIRST relay's EOSE-no-match, removing the row from `event_claims` BEFORE a slower sibling relay delivered the matching EVENT. Since the `claimed_events` projection walks `event_claims.keys()`, the vanished row meant the event never surfaced — so kind:1 notes stayed "loading" even though the EVENT arrived. Fix moved claim teardown to the single controller-owned `terminate_claim` site, gated on `ClaimTermination::Exhausted | Budget`, so a claim survives until ALL relays respond. RED-verified regression test in `claim_expansion_ingest_tests.rs`. Files: `discovery.rs`, `claim_expansion_helpers.rs`.

### Fix 2 — nevent relay hints (#844, OPEN/mergeable)
The showcase note + highlight nevents embedded `wss://relay.primal.net` as their relay hint, which doesn't serve those events (websocat: EOSE empty). For event-id (nevent) claims the kernel correctly follows the embedded hint first — outbox isn't reliable for a bare event id (per user: "for event ids you can't do outbox unless the bech32 includes the pubkey — that's ok — if the nevent has a relay hint we should follow that first anyway"). Re-encoded both nevents with `wss://nos.lol` (serves all three events) via `nak`, verified by `nak decode`. File: `apps/nmp-gallery/showcase-references.json`.

## Verified result (iOS sim, master+#843+#844, accessibility-tree text)
| embed | content | author (display name, not hex) | inline surrounding text |
|---|---|---|---|
| article (30023) | "What's left of the internet?" | Gigi | "check out my article … I hope you enjoy it!" |
| note (1) | "grok cli is INSANELY bad, jesus" | PABLOF7z | "this is a great point … what do you think?" |
| highlight (9802) | "Vibe-coding is what brought me back to programming" | PABLOF7z | "found this interesting …" |

TUI smoke (`./target/release/nmp-gallery-tui --smoke`): **✅ ALL 2 embed targets resolved** (was 1/2).

## NOT regressions
#828/#825/#834/#836/#841 were ALL exonerated. The race (#843) was pre-existing; the relay-hint mismatch (#844) was stale showcase data. Four agents earlier hallucinated code-regression theories because they lacked the deterministic `--smoke` repro; the fix came from instrument-first debugging against that oracle.

## Cold-start timing note (not a bug)
iOS cold kernel needs ~20-30s to connect → claim → fetch from nos.lol before an embed resolves. The "loading" placeholder during that window is correct. Screenshot capture for the verification PDF must wait that long per embed.

## Secondary cleanup (independent, low priority)
`apps/nmp-gallery/tui/src/main.rs:344-346` prints a false "seeded relays are purplepag.es, nos.lol, relay.damus.io, relay.nostr.band" message — the actual seed is `showcase::references().relays` (purplepag.es + relay.primal.net). Fix the message to read the real list.
