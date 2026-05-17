# M13 — Web-of-Trust

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — wallet/WoT + cross-platform + release.

**Demo product:** Twitter slice gets a "score-filtered timeline" toggle. With it on, low-WoT-score authors are de-prioritized; toggling off restores chronological order.

**Scope.** Per spec §7.7:

**Subsystem deliverables.**

- `nmp-wot` protocol module:
  - Action: `LoadFollowGraph { root: PubKey, depth: u8 }` — populates an in-memory follow graph.
  - Projection cache: `wot_score: HashMap<PubKey, f32>`.
  - View module: `WotRank` exposes per-pubkey score + reasoning.
  - Filter view module wrapper: composes with Timeline to produce a score-filtered variant.
- Pluggable scoring trait (default: depth-weighted in-degree).

**Exit gate.**

- Load follow graph rooted at the active account to depth 2; computes scores for 10k+ pubkeys in ≤ 5 s on iPhone 12.
- Score-filtered timeline visibly reorders / hides low-score authors; toggle off restores chronological.
- New kind:3 arrival incrementally updates scores without full recompute.

**Runnable artifact.** iOS Twitter slice with WoT toggle. Report in `docs/perf/m13/wot.md`.
