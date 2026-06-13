---
type: research-record
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
source_lines: 8117-8158
agent_attribution: aa54266b1636c10ef
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Opus agent evaluation of all 11 needs-decision issues against documented product direction, classifying each as A (determined-by-direction) or B (needs-owner) with verified code facts and unblocked actions"
captured_at: 2026-06-13T20:03:56Z
---

Opus agent evaluation of all 11 needs-decision issues against documented product direction, classifying each as A (determined-by-direction) or B (needs-owner) with verified code facts and unblocked actions

---

User: [Agent task result: Agent "Resolve all needs-decision issues vs product direction" completed]
All 11 decisions posted to GitHub. (The voice server at localhost:8888 returned 404 both times — it isn't running this session, so the spoken notification couldn't be delivered.)

📋 SUMMARY: All 11 `status:needs-decision` issues resolved in one pass — 10 determined by documented direction, 1 genuinely needs the owner.

🔍 ANALYSIS: The owner's bet held. After grounding in aim.md (D0, thin-shell, "all relay management in core"), plan.md (v1 = iOS+Android+desktop; web/wasm post-v1), doctrine.md D0, AGENTS.md zero-tolerance-on-hacks, and verifying the actual crate dependency graph, 10 of 11 were dictated by existing direction. The only true (B) is #1281's backfill semantics — a product contract choice not derivable from any durable doc.

⚡ ACTIONS: Read all 11 issues + comments, read the five durable docs, and verified two load-bearing facts in code: (1) `nmp-nip01 → nmp-core` already exists (Cargo.toml:11), so #920's literal fix would create a cycle; (2) `nmp-nip01` already owns the typed timeline row family, confirming the envelope-cut shape. Posted one titled comment per issue with citations + unblocked action.

✅ RESULTS: Decision table —

| Issue | A/B | One-line decision | Unblocked action |
|---|---|---|---|
| #1291 | A | GO — full iOS+Android+desktop parity via thin-shell wiring only (already owner-ratified) | File per-platform wiring tasks AFTER #1283/#980 land |
| #1283 | A | GO — resolve embeds in nmp-ffi via nmp-content, ship typed on sidecar key, decode in shells | Ready: ~3 PRs (M+S+S)+verify |
| #1281 | **B** | Exempt since=None from T129 watermark rewrite? | **Owner picks (a) exempt / (b) keep**; I recommend (a) |
| #1250 | A | PARK nmp-nip60 + nmp-blossom behind off-by-default feature/exclusion | Ready: one gating PR |
| #1202 | A | Kill silent wasm publish-fail NOW (honest-disable); real composition root → #1007 | Ready: small honesty PR + drift guard |
| #1090 | A | GO floor-coherent eviction (Stage 2) + re-enable ceiling (Stage 3) | Ready: Stages 2–3 dispatchable now |
| #1008 | A | Already decided post-v1; label stale | Drop needs-decision label |
| #999 | A | Defer to post-v1 (M13); no decision exists | Drop needs-decision label |
| #980 | A | Proceed — in-scope v1-dx, label stale; keep status:blocked | Dispatch F-CR-02 once #1283 in flight |
| #967 | A | Defer post-v1; shape pre-determined (unify MailboxCache, hard-rename) | Two post-v1 PRs, by-the-book |
| #920 | A | GO via snapshot-envelope cut (NOT naive move — cycle); post-v1, needs staged plan | Sequence as crate-boundary migration step |

📊 STATUS: 11/11 comments posted. One open sub-decision on #1090 (delete dead watermark machinery — recommend delete) folded into its A verdict.

📁 CAPTURE: Two code-verified facts worth keeping: #920's naive fix is cycle-blocked (`nmp-nip01 → nmp-core` exists; right shape is the envelope cut, and `nmp-nip01` already owns `ModularTimelineSnapshot`/`TimelineEventCard`). #1283 and #920 share the same architectural pattern: resolve protocol-specific branching one layer ABOVE the kernel, ship typed, kernel stays D0-clean.

➡️ NEXT: Ready-to-implement now (dispatch engineers): **#1283, #1250, #1202, #1090 (Stages 2–3)**. Label-only cleanup (drop stale needs-decision): **#1008, #999, #980, #967, #920** — with #920 needing a `status:staged` plan when scheduled, and #980 keeping `status:blocked`. The single item that still needs YOU: **#1281** — pick (a) exempt since=None [my rec] or (b) keep T129 as designed. One small clarifying sub-call on #1090: confirm DELETE the dead persisted-watermark machinery.

📖 STORY EXPLANATION:
1. The owner bet that most "needs-decision" issues were already implied by documented direction — and that bet was correct: 10 of 11 resolved without escalation.
2. I grounded every verdict in the immutable north star (aim.md), the release plan (v1 = native only, web post-v1), D0 doctrine, and the zero-tolerance-on-debt rule — citing file:line or principle for each.
3. Two issues (#1283, #1090) already had detailed plans; my job there was the GO/NO-GO, and D0 + zero-debt + single-mechanism cache-serve strongly dictated GO on both.
4. #920 looked like a one-liner ("move TimelineItem to nmp-nip01") but the live dependency graph proved that creates a cycle — the architecturally-right fix is the snapshot-envelope cut the layering already declares.
5. Four issues (#1008, #999, #980, #967) were effectively already decided or mislabeled — `status:needs-decision` was stale; direction or prior owner decisions fully settled them.
6. #1250 and #1202 were both resolved by the zero-debt rule: park dead islands explicitly, and never let a preview silently always-fail.
7. Only #1281 resisted derivation — whether an "all-time" interest should backfill below the watermark is a genuine product-contract choice, so I posed one crisp question with a recommendation and left it for the owner.
8. Net result: a clean implementation queue for engineers, a short label-cleanup list, and exactly one decision that genuinely needs the owner's voice.

🎯 COMPLETED: Eleven decisions posted — ten direction-determined, one needs owner on backfill semantics.
