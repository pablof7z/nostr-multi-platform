---
type: research-record
date: 2026-07-04
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
source_lines: 6657-6666
agent_attribution: ios-tester-b7-groups
has_preregistered_criteria: true
has_method: false
has_structured_report: true
characterization: "iOS sweep Batch 7 Groups (S45-53): NIP-29 and Marmot/MLS group testing — found Marmot completely dead on iOS, 3 PASS / 4 FAIL / 2 BLOCKED, 3 bugs filed"
captured_at: 2026-07-04T18:49:50Z
---

iOS sweep Batch 7 Groups (S45-53): NIP-29 and Marmot/MLS group testing — found Marmot completely dead on iOS, 3 PASS / 4 FAIL / 2 BLOCKED, 3 bugs filed

---

All 9 scenarios (S45–S53) are complete, with evidence committed and pushed to branch `ios-sweep-b7`, and one comment posted per scenario on chirp#60.

## Results

| Scenario | Verdict |
|---|---|
| S45 Groups tab load | PASS |
| S46 Discover groups | PASS |
| S47 Join a public group | **FAIL** — bug #74 |
| S48 Public group chat send 
