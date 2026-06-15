---
type: research-record
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 2003-2012
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Before/after empirical measurement of follows' kind:0 profile resolution: indexer-only 10.2% (108/1054) vs outbox/own-relays 50.0% (528/1054), ~5× improvement; found purplepag.es is AUTH-walled (returns 0 kind:0 anonymously)"
captured_at: 2026-06-15T01:36:09Z
---

Before/after empirical measurement of follows' kind:0 profile resolution: indexer-only 10.2% (108/1054) vs outbox/own-relays 50.0% (528/1054), ~5× improvement; found purplepag.es is AUTH-walled (returns 0 kind:0 anonymously)

---

Assistant: Excellent — the **baseline measurement landed and confirms the thesis emphatically**:

| Metric | Count | % |
|---|---|---|
| Follows (N) | 1054 | 100% |
| **BEFORE** (indexer-only kind:0) | **108** | **10.2%** |
| **AFTER** (outbox / own relays) | **528** | **50.0%** |
| Delta | **+420** | **+39.8 pts** |

**~5× improvement** (10.2% → 50.0%), exactly the jump you predicted. And a killer concrete finding: **purplepag.es is AUTH-walled** — it returns 0 kind:0 to anonymous queries — so today the *entire* 108 comes from primal.net alone. One of Chirp's two indexers is effectively dead for anonymous profile resolution, which is *why* it's so bad. The fix is capped only by how many follows publish a NIP-65 list (608/1054), and resolved 528 of them.
