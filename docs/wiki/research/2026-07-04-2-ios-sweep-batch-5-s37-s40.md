---
type: research-record
date: 2026-07-04
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
source_lines: 6284-6293
agent_attribution: ios-tester-b5
has_preregistered_criteria: true
has_method: false
has_structured_report: true
characterization: "iOS sweep Batch 5 (S37-S40, S62-S64): profile/accounts testing — found systemic write-publish failure where UI lies about success; mostly PASS with FAIL dispatch verdicts"
captured_at: 2026-07-04T18:49:50Z
---

iOS sweep Batch 5 (S37-S40, S62-S64): profile/accounts testing — found systemic write-publish failure where UI lies about success; mostly PASS with FAIL dispatch verdicts

---

All work for this batch is complete. Here's the summary of what I found and did.

## Results table (S37–S40, S62–S64)

| Scenario | Verdict | Key finding |
|---|---|---|
| S37 (own profile) | **PASS** | Name/bio/avatar/banner/post-count/notes list/Edit button all correct |
| S38 (other profile + follow/unfollow) | **PASS render / FAIL dispatch** | Bob's profile renders correctly, but unfollow flips the UI button while the relay's kind:3 event stays byte-identical — the publish never happens, and the Outbox falsely reports "All published" |
| S39 (resolution correctness) | **PASS mostly / FAIL finding** | Names resolve everywhere except a permanent raw-hex citation line under kind:30023/30311 posts, even though the s

