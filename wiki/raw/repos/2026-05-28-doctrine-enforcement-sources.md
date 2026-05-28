---
title: "Doctrine Enforcement Sources 2026-05-28"
summary: "Source notes for canonical doctrine, PR checklist, doctrine-lint coverage, and local validation gates."
tags: [repo, doctrine, lint, testing]
source_type: repo-snapshot
repo: /Users/pablofernandez/Work/nostr-multi-platform
commit: 50ecae23b3587affa1ae167baa067a1e07b9a677
ingested: 2026-05-28
updated: 2026-05-28
---

# Doctrine Enforcement Sources 2026-05-28

## Primary Source Files

- `docs/product-spec/doctrine.md`
- `docs/builder-guide/22-doctrine-checklist.md`
- `crates/nmp-testing/Cargo.toml`
- `crates/nmp-testing/bin/doctrine-lint/main.rs`
- `crates/nmp-testing/bin/doctrine-lint/rules/`
- `.github/workflows/doctrine-lint.yml`
- `AGENTS.md`

## Canonical Doctrine

`docs/product-spec/doctrine.md` owns the product doctrine semantics. It defines
D0-D10 and says D0-D5 plus D10 are policy doctrines, while D6-D9 are substrate
invariants. This file wins over checklist shorthand and lint implementation
details when explaining what a doctrine means.

## Reviewer Checklist

`docs/builder-guide/22-doctrine-checklist.md` is the reviewer-facing checklist.
It maps each doctrine to concrete PR checks and points to machine-enforced
subsets. It is operational guidance, not a second doctrine definition.

## Doctrine Lint

`doctrine-lint` is a grep-based static analyzer under `nmp-testing`. The binary
currently advertises rules D0, D6, D7, D8, D9, D10, D11, D12, D13, D14, D15,
and D16. The `doctrine_lint_smoke` test is registered in
`crates/nmp-testing/Cargo.toml` and points at
`bin/doctrine-lint/tests.rs`.

The lint rule numbers are implementation gates. Do not infer canonical
doctrine semantics from the rule number alone; read the rule file and the
canonical doctrine doc. For example, the current lint rule `d9.rs` enforces
`nmp.` namespace prefixes for action constants, while canonical product D9 in
`docs/product-spec/doctrine.md` is about kernel-owned timestamp policy.

## Always-On Local Gate

The repo contributor guide requires agents to run:

`cargo test -p nmp-testing --test doctrine_lint_smoke`

This catches doctrine-lint smoke coverage that scoped crate tests do not run.
The same guide also calls for workspace compile checks when public symbols,
module paths, dependency paths, or workspace members change.

## Authority Notes

Use this source set to distinguish three layers:

- canonical product rule: `docs/product-spec/doctrine.md`;
- reviewer operation: `docs/builder-guide/22-doctrine-checklist.md`;
- machine-enforced subset: `crates/nmp-testing/bin/doctrine-lint/`.
