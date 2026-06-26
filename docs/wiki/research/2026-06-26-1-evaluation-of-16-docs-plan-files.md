---
type: research-record
date: 2026-06-26
session: 5f0cae74-2bae-4eab-b33c-978eeca433c9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5f0cae74-2bae-4eab-b33c-978eeca433c9.jsonl
source_lines: 471-500
agent_attribution: plan-issue-capture@session-5f0cae74
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Evaluation of 16 docs/plan/ files: per-file assessment of shipped/stale/already-tracked status with systematic verdicts and final judgment that all safe to delete with no valid future work lost."
captured_at: 2026-06-26T08:07:51Z
---

Evaluation of 16 docs/plan/ files: per-file assessment of shipped/stale/already-tracked status with systematic verdicts and final judgment that all safe to delete with no valid future work lost.

---

<teammate-message teammate_id="plan-issue-capture" color="blue" summary="Final verdict report; safe to delete docs/plan">
FINAL REPORT — every docs/plan/ file is accounted for. Safe to delete docs/plan/. No still-valid future work will be lost. Two new issues created: #2121 and #2122.

Per-file verdicts:

| File | Verdict |
|---|---|
| m12-wallet.md | SHIPPED + TRACKED — v1 zaps shipped (#978/#1043/#610/#1022 all CLOSED, PR #1076). Post-v1 remainder: Cashu/nutzaps = #1001, wallet surface = #1508. No new issue. |
| m13-wot.md | ALREADY-TRACKED → #999 (post-v1 WoT decision). |
| m14-uniffi.md | STALE/SUPERSEDED — project never adopted UniFFI (zero `uniffi` refs in any Cargo.toml); converged on raw C-ABI + codegen instead (#1901/#1921/#1933/#1726/#1724 all CLOSED), consistent with the "No C-ABI freeze" doctrine + Chirp C-ABI thin shell. Doc is self-caveating. No issue. |
| m15-cross-platform.md | SHIPPED (native) — chirp-desktop exists, Android exists, cross_platform_parity.rs consistency test exists, #1002/#1291 parity CLOSED, MLS validated iOS↔Android. Web part ALREADY-TRACKED → #2045/#1007/#2038. No new issue. |
| m16-cli-starter.md | PARTLY SHIPPED + NEW #2122 — CLI (init/doctor/upgrade/add component) built; missing starter app, full recipe book, docs/nips.md, docs/migration.md, external-dev onboarding proof. |
| m16-component-registry.md | SHIPPED → #980 CLOSED (F-08 + content kits). jsrepo export is explicit nice-to-have; recipe/adoption residue folded into #2122. No standalone issue. |
| m17-release.md | NEW #2121 — release infra exists (release/nmp-release.toml + CI gates) but no public crates.io/npm release, naming unresolved, downstream still pins git revs. |
| post-v1.md | INDEX ONLY — all items tracked (DMs #1259, wallet #1001/#1508, web #2045/#1007, Marmot shipped). |
| marmot-mls.md | STALE — Marmot/MLS shipped + validated 2026-06-13 (per your instruction). |
| decision-log.md | DURABLE-DUPLICATED — ADR-0001–0010 live in docs/decisions/. |
| chirp-showcase.md | DURABLE process doc; no untracked deliverable. |
| principles.md | DURABLE doctrine. |
| subsystem-matrix.md | DURABLE/historical mapping. |
| test-pyramid.md | DURABLE/executed. |
| ci-hygiene.md | MOSTLY EXECUTED — some target-state gates (native iOS/Android/desktop builds + benches as PR-blocking) are aspirational. I deliberately did NOT manufacture an issue; flagging here in case you want CI-gate hardening tracked. |
| parallelization.md | DURABLE orchestration doc. |

Issues created:
- #2121 — Ship the v1 NMP release train: public crates.io + npm availability (priority:p2, category:feature, phase:v1-infra, area:architecture)
- #2122 — Complete the v1 developer-onboarding experience: starter app, recipe book, NIP + migration docs (priority:p2, category:feature, category:docs, phase:v1-dx)

One judgment call to confirm: ci-hygiene.md's aspirational CI gates are the only thing I left as a note rather than an issue. Everything else is shipped/stale/tracked. Cleared to delete.
</teammate-message>
