# M16 — CLI + starter app + recipe book

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — wallet/WoT + cross-platform + release.

**Demo product:** A developer with no prior framework knowledge runs `nmp init my-app`, follows recipes, ships a working hashtag-feed app on all four platforms in ≤ 2 hours.

**Scope.**

**Subsystem deliverables.**

- `nmp init`, `nmp add module`, `nmp gen modules`, `nmp doctor`, `nmp upgrade` commands.
- A minimal **starter app** (distinct from the proof/Twitter app) implementing only: login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
- Recipe book in `docs/recipes/`: one recipe per common app shape (timeline-only viewer, kind-filtered explorer, long-form reader, etc.).
- NIP support matrix in `docs/nips.md`.
- Migration guide in `docs/migration.md`.

**Exit gate.**

- §3 success criteria of the spec reproducible from published docs alone, no insider knowledge.
- One external developer (or an LLM agent with no prior context) succeeds at building a small custom app from the starter + recipes in ≤ 2 hours.

**Runnable artifact.** Public `nmp init` flow. Report in `docs/perf/m16/dx.md`.
