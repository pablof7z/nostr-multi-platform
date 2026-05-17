# Pending User Decisions

Decisions I made autonomously while the user was asleep, with my reasoning. If the user disagrees with any, the noted commit can be reverted or amended.

Format: one entry per decision. Surface every entry in every status update until the user explicitly acknowledges or supersedes.

---

## Open (need user review)

_(none — resolved or none open)_

---



---

## Resolved (user acked or superseded)

### PD-001 (resolved 2026-05-18) — Doctrine vocabulary collision

**User picked option (b):** expand `docs/product-spec/overview-and-dx.md` §1.5 to formally absorb the three additional load-bearing rules (errors-never-FFI / capabilities-report / reactivity-≤60Hz) as named doctrines D6, D7, D8.

Product-spec now has D0–D8 with an explicit "two kinds" distinction:
- **D0–D5: policy doctrines** — user-facing semantics (kernel-boundary, best-effort rendering, negentropy-first, outbox-automatic, single-writer-per-fact, snapshots-bounded).
- **D6–D8: substrate invariants** — runtime / FFI / hot-path constraints (errors-never-FFI, capabilities-report, reactivity-contract).

Conflicts still resolve in listed order (D0 wins over D8). README aligned. T19 framework-magic-reconciler in flight will absorb D0–D8 into the framework-magic docs (sending them an updated brief alongside this commit).

