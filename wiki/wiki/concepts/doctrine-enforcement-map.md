---
title: "Doctrine Enforcement Map"
summary: "How canonical doctrine, reviewer checklists, doctrine-lint, and local gates relate without becoming duplicate authorities."
tags: [doctrine, lint, testing]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-doctrine-enforcement-sources.md"
---

# Doctrine Enforcement Map

Doctrine in NMP has three layers: the canonical product rule, the reviewer
checklist, and the machine-enforced subset. They are related, but they are not
the same artifact.

## Canonical Rule

`docs/product-spec/doctrine.md` owns the meaning of D0-D10. Start there when
answering what a doctrine means or why a boundary exists.

The current canonical split is:

- D0-D5 and D10: product policy doctrines.
- D6-D9: substrate invariants.

That distinction matters in review. Policy doctrines ask whether an API makes
the wrong behavior possible. Substrate invariants ask whether the runtime,
transport, or replay model can keep the promised behavior true.

## Reviewer Operation

`docs/builder-guide/22-doctrine-checklist.md` is the PR checklist. It turns
canonical doctrine into concrete review questions: no app nouns in
`nmp-core`, no native policy decisions, no unbounded snapshots, no relay URLs
on safe app-facing send/view APIs, and so on.

The checklist should be used as review workflow. It should not be treated as a
second canonical doctrine file.

## Machine Gates

`doctrine-lint` lives under `crates/nmp-testing/bin/doctrine-lint/`. It is a
grep-based guardrail for classes of mistakes the repo has chosen to catch
mechanically. The smoke test is registered as `doctrine_lint_smoke` in
`crates/nmp-testing/Cargo.toml`.

The linter has rules named D0, D6, D7, D8, D9, D10, and D11-D16. Those rule ids
are implementation gates, not a complete restatement of canonical doctrine.
Always read the rule file before relying on a rule number. For example, the
current lint `d9.rs` checks action namespace prefixes, while canonical product
D9 is timestamp ownership.

## Local Validation Rule

Agents are expected to run:

```sh
cargo test -p nmp-testing --test doctrine_lint_smoke
```

That gate is intentionally separate from scoped crate tests. A change can pass
the crate it touched and still violate a cross-cutting doctrine lint.

## See Also

- [[rust-owned-logic-boundary|Rust-Owned Logic Boundary]] ([Rust-Owned Logic Boundary](rust-owned-logic-boundary.md))
- [[temporal-plans-vs-durable-docs|Temporal Plans vs Durable Docs]] ([Temporal Plans vs Durable Docs](temporal-plans-vs-durable-docs.md))
- [[source-authority-map|Source Authority Map]] ([Source Authority Map](../references/source-authority-map.md))

## Sources

- [Doctrine Enforcement Sources](../../raw/repos/2026-05-28-doctrine-enforcement-sources.md)
