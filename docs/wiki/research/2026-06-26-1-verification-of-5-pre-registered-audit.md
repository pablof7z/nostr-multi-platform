---
type: research-record
date: 2026-06-26
session: c04a2540-aae1-49c3-adb3-a646dbb5f21f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c04a2540-aae1-49c3-adb3-a646dbb5f21f.jsonl
source_lines: 150-167
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Verification of 5 pre-registered audit findings against current repository state; verdicts: CONFIRMED/MOSTLY-CONFIRMED/STALE with file-level evidence"
captured_at: 2026-06-26T09:12:27Z
---

Verification of 5 pre-registered audit findings against current repository state; verdicts: CONFIRMED/MOSTLY-CONFIRMED/STALE with file-level evidence

---

Assistant: I've verified all the doc-level findings directly. Three code-grounded checks (ADR-0029 channel/metric, ADR-0032 miscitation, ADR-0057/0025/0016 code) are still running in the background — I'll fold them in when they report. Here's what's confirmed so far against current `master`:

## Doc-level findings — verdicts

**✅ Finding #1 (two authority models) — CONFIRMED, genuine.**
- `docs/decisions/README.md:4` — "point-in-time decision, **not a living document**… superseded or amended rather than rewritten."
- `AGENTS.md:89` — "Correct docs in place; **do not layer superseding corrections**… The current rule must be readable in the document that owns it."
- `0036-…md:149` — "edit this document in place instead of adding a later correction document."
- These genuinely conflict. The audit's read is fair.

**✅ Finding #2 (index ↔ file mismatches) — MOSTLY CONFIRMED, one part stale.**
- **ADR-0016 heading**: file body literally reads `# ADR-0041: …` and "takes the next free number, 0041" (`0016-f-ttl-ffi-surface.md:1,7`). The README index *row* is correct (`0016 … (renumbered from 0041)`), but the **file's own heading still says 0041** — an agent opening the file is misled. CONFIRMED.
- **Status mismatches** all CONFIRMED: README says `Proposed` for 0043/0051/0052/0057, but the files say `Implemented` / `Accepted / Implemented` / `Implemented` / `Implemented` respectively.
- **`docs/plan/decision-log.md` stops at 0010** — STALE: that file and the whole `docs/plan/` tree were already retired (commit `095b40a1d`). Finding no longer applies.
- One nuance: ADR-0029 is *not* a mismatch — both README ("Accepted; bound never built") and the file ("Superseded / Not implemented") agree it was never built.

**⚠️ Important context: PR #2130 ("docs: retire stale non-wiki guidance") is OPEN and already rewriting many of these ADRs in place** — including 0016, 0025, 0029, 0039, 0042 and ~30 ADR/builder-guide files, plus `aim.md`. It explicitly edits ADRs in place rather than layering corrections (the `AGENTS.md` model). So part of the audit's cleanup is already in flight — but note it does **not** regenerate the README index from front matter, so the status/heading mismatches likely persist after it merges unless we add the mechanical lint the audit recommends.

