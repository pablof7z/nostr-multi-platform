---
type: research-record
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 3206-3233
agent_attribution: a8ef6ceeba20dcbdc
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Re-measurement of follows kind:0 resolution rate: outbox model yields 60.3% (up from 50.0% this morning), indexer-only baseline recovered to 27.9% due to purplepag.es responding, outbox still contributes +341 unique resolves"
captured_at: 2026-06-15T09:48:44Z
---

Re-measurement of follows kind:0 resolution rate: outbox model yields 60.3% (up from 50.0% this morning), indexer-only baseline recovered to 27.9% due to purplepag.es responding, outbox still contributes +341 unique resolves

---

User: [Agent task result: Agent "Re-measure follows kind:0 resolution now" completed]
Measurement complete. Let me note the key methodological difference from this morning to explain the shift.

The AFTER is now **60.3%**, up from this morning's 50.0%. The biggest driver: purplepag.es responded this turn (it was AUTH-walled this morning), lifting the BEFORE/indexer baseline from 108 (10.2%) to 294 (27.9%), and more follows now expose a kind:10002 (718 vs 608). The outbox sweep still adds 341 follows that indexers alone missed.

## Re-run complete — current profile-resolution numbers

Target: `_@f7z.io` / PABLOF7z (`fa984bd7…8f52`). Newest kind:3 dated 2026-06-10 (unchanged since this morning). Measured live just now via the throwaway nostr-sdk harness at `/tmp/nip0-measure/` (rebuilt the kind:3 / kind:10002 fetchers in Rust because `nak`'s multi-author REQ hangs on EOSE; kind:0 sweeps use `fetch_events` with 10–12s timeouts, outbox capped at top-100 write relays, 12-way concurrency).

| Metric | This morning | **Now (current)** |
|---|---|---|
| N — follows (distinct p-tags) | 1054 | **1052** |
| M — follows with a kind:10002 | 608 (57.7%) | **718 (68.2%)** |
| BEFORE — indexer-only kind:0 (primal + purplepag) | 108 (10.2%) | **294 (27.9%)** |
| AFTER — outbox (indexers ∪ each follow's own write relays) | 528 (**50.0%**) | **635 (60.3%)** |
| gained ONLY via outbox (not on indexers) | — | 341 |

### Direct answer
No — follows' kind:0 resolution is **no longer ~50%; it's now 60.3%** under the outbox model.

### Why it moved (materially, +10.3pts)
1. **purplepag.es answered this run.** This morning it was AUTH-walled, so the indexer baseline was a floor of 108 (10.2%). This run it returned events, lifting BEFORE to 294 (27.9%). That single relay-availability change is the dominant driver of the AFTER lift too, since AFTER is the union with that baseline.
2. **More follows now expose NIP-65.** M rose 608 → 718 (57.7% → 68.2% of follows publish a kind:10002), giving the outbox path more write relays to query.
3. **The outbox model still does the heavy lifting:** 341 follows resolve ONLY via their own write relays (invisible to the two indexers), and the outbox sweep alone hit 605 distinct kind:0.

Caveat on comparability: the AFTER gain attributable specifically to the *outbox mechanism* (vs. indexer availability) is best read as AFTER − BEFORE = +341 follows this run vs. +420 this morning — i.e., the outbox contribution is steady; the headline % rose mainly because the indexer baseline recovered (purplepag back online). The N delta (1054→1052) is noise from which relays served the newest kind:3, not a real unfollow — the contact list itself is unchanged (created_at 2026-06-10).

All throwaway code/data stayed in `/tmp/nip0-measure/` (current-run outputs: `follows_current.txt`, `have_10002.txt`, `before_resolved_current.txt`, `outbox_resolved_current.txt`, `after_resolved_current.txt`).
