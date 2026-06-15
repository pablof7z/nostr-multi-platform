---
type: research-record
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 3272-3299
agent_attribution: a8ef6ceeba20dcbdc
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "App relay measurement: adding nos.lol lifts resolution from 60.3% to 88.8%, with 204 of 300 net-new profiles having no NIP-65 (structurally unreachable by outbox alone)"
captured_at: 2026-06-15T10:26:41Z
---

App relay measurement: adding nos.lol lifts resolution from 60.3% to 88.8%, with 204 of 300 net-new profiles having no NIP-65 (structurally unreachable by outbox alone)

---

User: [Agent task result: Agent "Re-measure follows kind:0 resolution now" completed]
All sane: every resolved author is a real follow (0 spurious), nos.lol is the dominant driver (299 of the 300 net-new), and it reaches 217 of the 334 outbox-unreachable no-NIP-65 follows. Measurement complete.

## Result — does adding an APP relay resolve more kind:0? Yes, substantially.

Reused `/tmp/nip0-measure/` (`follows_current.txt` N=1052, `have_10002.txt` M=718, `after_resolved_current.txt`=635). Swept `{kinds:[0], authors:[all 1052 follows]}` against each relay separately (batch 50, 12s EOSE, paced) via the existing `nip0measure` SDK binary. Every resolved pubkey verified to be a real follow (0 spurious).

| Metric | Count | % of N (1052) |
|---|---|---|
| **AFTER** (indexers ∪ each follow's own write relays) — baseline | 635 | 60.3% |
| App relay: damus.io kind:0 distinct | 202 | 19.2% |
| App relay: nos.lol kind:0 distinct | 931 | 88.5% |
| **App union (damus.io ∪ nos.lol)** | 932 | 88.5% |
| **App-relay NET-NEW** (resolved, not already in AFTER) | **+300** | +28.5pts |
| — of net-new, follows with **NO kind:10002** (outbox-unreachable, app-only) | **204** | — |
| **NEW TOTAL (AFTER ∪ app relays)** | **935** | **88.8%** |

### The key number
Of the 300 follows that an app relay adds on top of the full outbox model, **204 publish no kind:10002 at all** — they are structurally unreachable by the outbox/NIP-65 path (no write relays to query). A broad app relay is the *only* way to get their kind:0. Context: 334 of the 1052 follows (31.7%) have no NIP-65; the app relay resolves 217 of those 334 outbox-unreachable profiles (nos.lol alone: 217).

### Typical app relays vs aggregator (reported separately, as requested)
- **damus.io + nos.lol (typical app relays):** drive the entire +300. nos.lol is the workhorse (299 of the 300 net-new, 931 total); damus.io adds only 7 net-new beyond AFTER but corroborates.
- **relay.nostr.band (aggregator):** not measurable via open REQ in this harness — it returned 0 kind:0 across all batches, and a focused 5-author / 15s probe also returned 0 while those same authors resolve on damus. It appears to require NIP-42 AUTH (or otherwise reject) bulk multi-author kind:0 REQs, so I excluded it rather than score it as 0 real coverage. Its true contribution is unknown from an unauthenticated client.

### Bottom line
Adding a single broad app relay (nos.lol) jumps profile resolution from **60.3% → 88.8%**, and — critically — most of that gain (204 follows) is the no-NIP-65 cohort that the outbox model can never reach. An app that configures a popular general relay alongside outbox/indexers closes nearly all of the remaining kind:0 gap.

Throwaway artifacts (current run): `app_damus.txt`, `app_noslol.txt`, `app_union.txt`, `app_netnew.txt`, `netnew_no_nip65.txt`, `no_nip65.txt`, `after_plus_app.txt` — all in `/tmp/nip0-measure/`.
