---
title: Workspace Test Policy
slug: workspace-test-policy
topic: ci-gates
summary: Running `cargo test --workspace` must not be run
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:04745411-a0c1-4523-ac83-71dc983f410b
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
---

# Workspace Test Policy

## Workspace Test Execution Policy

Running `cargo test --workspace` must not be run. Wallet-related tests are scoped to `cargo test -p nmp-wallet -p nmp-nip60` plus `cargo test -p nmp-testing --test doctrine_lint_smoke`.

<!-- citations: [^04745-624e3] [^91a86-405ed] [^91a86-da265] -->

## Wallet Action Constant Uniqueness

A test (`no_action_namespace_is_duplicated_as_a_compatibility_alias`) asserts no two wallet action constants share a string value. <!-- [^91a86-53528] -->
