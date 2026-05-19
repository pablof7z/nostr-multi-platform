# Principles of execution

> Part of the [Build & Validation Plan](../plan.md).

1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the [M11](m11-podcast.md) podcast-app proof.
6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.
