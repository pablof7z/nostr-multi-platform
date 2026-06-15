---
type: research-record
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
source_lines: 2003-2012
agent_attribution: a1a08e24546128fbd
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Baseline measurement of kind:0 profile resolution: indexer-only resolves 10.2% (108/1054) vs outbox model resolves 50.0% (528/1054), ~5× improvement; discovered purplepag.es is auth-walled returning 0 results anonymously"
captured_at: 2026-06-14T23:14:28Z
---

Baseline measurement of kind:0 profile resolution: indexer-only resolves 10.2% (108/1054) vs outbox model resolves 50.0% (528/1054), ~5× improvement; discovered purplepag.es is auth-walled returning 0 results anonymously

---

Assistant: Excellent — the **baseline measurement landed and confirms the thesis emphatically**:

| Metric | Count | % |
|---|---|---|
| Follows (N) | 1054 | 100% |
| **BEFORE** (indexer-only kind:0) | **108** | **10.2%** |
| **AFTER** (outbox / own relays) | **528** | **50.0%** |
| Delta | **+420** | **+39.8 pts** |

**~5× improvement** (10.2% → 50.0%), exactly the jump you predicted. And a killer concrete finding: **purplepag.es is AUTH-walled** — it returns 0 kind:0 to anonymous queries — so today the *entire* 108 comes from primal.net alone. One of Chirp's two indexers is effectively dead for anonymous profile resolution, which is *why* it's so bad. The fix is capped only by how many follows publish a NIP-65 list (608/1054), and resolved 528 of them.
