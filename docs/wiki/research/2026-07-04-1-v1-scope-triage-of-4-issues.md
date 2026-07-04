---
type: research-record
date: 2026-07-04
session: d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5.jsonl
source_lines: 284-311
agent_attribution: review-v1-scope
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "V1-scope triage of 4 issues: grep-verified crate/code presence on master against docs/nips.md criteria, verdict NONE are v1 blockers (DEFER-POST-V1/CLOSE)"
captured_at: 2026-07-04T12:36:51Z
---

V1-scope triage of 4 issues: grep-verified crate/code presence on master against docs/nips.md criteria, verdict NONE are v1 blockers (DEFER-POST-V1/CLOSE)

---

User: Another Claude session sent a message:
<teammate-message teammate_id="review-v1-scope" color="purple" summary="v1-blocker triage of 4 issues complete">
Triage complete. Anchored to two authoritative surfaces: #2690 exit criteria (v1 = name/version/publish/prove) and `docs/nips.md` (the "v1 pre-release truth source", reviewed 2026-07-01). Verdict up front: NONE of the four is a genuine v1 code blocker.

---
**#2864 — NIP-60/61 wallet epic → DEFER-POST-V1 (add phase:post-v1)**
- Verdict: Feature app on top of the framework, not v1 framework surface.
- Evidence: `docs/nips.md` row 54 classifies "NIP-60 Cashu wallet and NIP-61 nutzaps" as **post-v1**, explicitly ("none for v1"; "Re-enter scope only through an explicit post-v1 wallet milestone"). NIP-47 NWC (row 48) and NIP-57 zaps (row 52) are likewise post-v1, removed from the release train + default composition by #2318. aim.md §1 lists wallet as "eventually," not v1. The heavy recent merges (#3002/#3007) are that post-v1 milestone executing early — good, but they do not pull it into v1 scope. The epic body itself confirms `nmp-core` learns no wallet nouns (invariant), so publishing 1.0 crates.io surface is unaffected by wallet state.
- Action: Label phase:post-v1. Do not block v1.

**#2858 — X-Ray diagnostic surface epic → DEFER-POST-V1 (add phase:post-v1)**
- Verdict: Dev-build-only diagnostic tooling; by its own invariants it is NOT part of the shipped public/app surface.
- Evidence: Issue's own invariants: "Diagnostic surface is dev-build-only… `nmp-devtools` is a separate crate behind an off-by-default cargo feature, never linked by app code; CI asserts each platform's release-artifact dependency graph contains no `nmp-devtools`." It cannot be a 1.0 public-crate blocker because it is mechanically excluded from release artifacts. Flagship consumer is chirp#30 (external app). Most phases (B/C/D/E) are still open with large items (headless host runner, MCP server, Chirp pane) — clearly a post-v1 build-out.
- Action: Label phase:post-v1. Do not block v1.

**#2927 — NIP-AD → CLOSE (already delivered on master)**
- Verdict: Core deliverables DONE and desktop-proven; the issue is stale (still carries status:needs-adr / category:decision though the design+impl landed).
- Evidence (verified on master, `grep --include=*.rs crates`): the full crate `crates/nmp-nip-ad` exists; `WireNode::AdCandidateUrl`, `nmp_content::AdUrlResolver`, `AdUrlState`, and `nmp_nip_ad::open_ad_collection` are all present and wired in `crates/nmp-native-runtime/src/ad.rs` (fail-open D1/D6 via `AdUrlState::ResolutionFailed`, `AdResolutionPolicy` seam, `resolve_ad_url_blocking`). Companion crates `nmp-wellknown-http` also present. Memory records desktop-iced live proof (#3000) and that FFI-shell/Chirp follow-ups (#2979/#2981) are already split out and deferred.
- Action: CLOSE #2927 as delivered (or mark Done + strip needs-adr), leaving #2979/#2981 as the tracked post-v1 tails. Either way it is NOT a v1 blocker.

**#2974 — Marmot MLS keyring CredentialStore never wired → DEFER-POST-V1 (keep open, add phase:post-v1)**
- Verdict: Genuine correctness defect, but in a feature that is NOT part of the v1 public surface.
- Evidence: Confirmed the bug — `grep -rn "set_default_store" crates` returns ZERO matches, so `MdkSqliteStorage::new`'s required global keyring store is never initialized; `is_registered` is permanently false for every identity on every platform, exactly as the issue traces. BUT: Marmot/MLS/NIP-EE has no v1 support row in `docs/nips.md` at all; the only reference (row 53, NIP-59) lists `nmp-marmot` as a "partial" protocol primitive, and NIP-17 DMs (row 37) are "partial" with explicit "v1 product docs must not claim complete DM UX." Encrypted MLS groups are not a v1 claim, so shipping 1.0 with this unfixed does not ship a *claimed* v1 feature broken. The fix also needs a net-new Android Keystore-backed `keyring_core::CredentialStore` (none exists in the ecosystem) — real post-v1 work.
- Action: Keep OPEN (it is a real bug for anyone using Marmot), label phase:post-v1. Fix it in the post-v1 DM/groups milestone, not before publishing 1.0. NEEDS-OWNER only if the owner wants Marmot groups pulled *into* v1 — nips.md says no.

---
**BOTTOM LINE:** There is NO genuine framework code work in these four blocking v1. #2927 is already done (close it). #2864, #2858, #2974 are all post-v1 (feature app / dev tool / non-v1-claimed MLS feature) and should be labeled phase:post-v1 so they stop looking like un-phased p2 blockers against #2690's "zero open p2 besides #1626" bar. Consistent with the owner's steer: an issue existing ≠ it must ship in v1. v1 remains what #2690 says it is — the owner-gated publish act (name → rc rehearsal → 1.0.0 tag → crates.io/npm → external-consumption proof), gated on the public-name decision and #1626, not on any of these four.
</teammate-message>
