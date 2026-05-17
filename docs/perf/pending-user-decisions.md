# Pending User Decisions

Decisions I made autonomously while the user was asleep, with my reasoning. If the user disagrees with any, the noted commit can be reverted or amended.

Format: one entry per decision. Surface every entry in every status update until the user explicitly acknowledges or supersedes.

---

## Open (need user review)

### PD-001 — Doctrine vocabulary collision: D0–D5 vs RMP-bible invariants vs reactivity ADRs

**Decision (autonomous):** The canonical D0–D5 is what's in `docs/product-spec/overview-and-dx.md` §1.5 (kernel-boundary, best-effort, negentropy-first, outbox-automatic, single-writer-per-fact, snapshots-bounded). I corrected the README to match (commit pending in next push).

**Why:** Codex review of merge `51120cb` flagged that I had been using a different D2/D3/D5 mapping throughout this session — conflating the product-spec D0–D5 with RMP-bible invariants (errors-never-cross-FFI is invariant #2; capabilities-report-don't-decide is RMP cardinal rule #6; ≤60 Hz reactivity is from ADR-0001..0004). They're all real binding rules, but they're three different rule sources. I had collapsed them into a single "D0–D5" rubric, which was wrong.

**What's in master that might still have the wrong mapping:**
- `docs/design/framework-magic.md` + sub-files — were written using my collapsed mapping. The framework-magic-designer (T17) inherited it from earlier session messages. A follow-up commit will reconcile.
- Various codex review prompts I wrote earlier passed the wrong D0–D5 to codex. The flagged findings in those reviews are still valid (codex review the doctrine word-by-word), but the framing was off.
- `docs/perf/m10.5/debt-inventory.md` — uses the canonical D0–D5 correctly (codex flagged + fixed earlier).

**Open question for user:** Do you want me to (a) keep the canonical product-spec D0–D5 as the single rubric and downgrade the RMP-bible invariants + reactivity-ADR rules to "supporting rules" (cited by number where relevant); or (b) expand the canonical D0–D5 to incorporate the three additional load-bearing rules (would require editing `docs/product-spec/overview-and-dx.md` §1.5 — that's the authoritative source so changing it is a meaningful commit).

**Current default while you decide:** option (a). README + framework-magic docs will be aligned to product-spec D0–D5; supporting rules cited by their source (RMP-bible invariant N, ADR-NNNN, etc.).

---



---

## Resolved (user acked or superseded)

_(none yet)_
