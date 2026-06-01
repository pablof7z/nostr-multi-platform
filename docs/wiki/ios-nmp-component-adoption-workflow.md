---
title: iOS NMP Component Adoption Workflow
slug: ios-nmp-component-adoption-workflow
summary: Implementation uses parallel Haiku agents working in isolated git worktrees on an integration branch (`ios/nmp-component-adoption`), with each agent's work revi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:baec5921-6cfd-49df-9ee1-a2b6a81898f8
---

# iOS NMP Component Adoption Workflow

## Integration Branch Workflow

Implementation is fanned out to Haiku agents working in parallel, and all parallel agent work must result in a single PR rather than multiple PRs. Each agent works in an isolated git worktree on the integration branch (`ios/nmp-component-adoption`), with each piece reviewed by a Sonnet agent checking doctrine compliance, design fidelity, test coverage, and merge-order annotations. After all implementation and review phases complete, a single Opus agent performs a holistic cross-review of all work, cross-checks the test plan, confirms merge order, and issues a SHIP / SHIP_WITH_FIXES / HOLD verdict. (Previously: each agent's work was reviewed by a Sonnet agent before merging into the integration branch with no PR, and subsequent agents launched after each merge; the integration branch was submitted as a PR and landed into master only when enough significant work accumulated.)

<!-- citations: [^9a2c7-12] [^baec5-2] -->
## Agent Constraints

Haiku agents must never run `cargo test`; that validation happens during the merge process. Workflow agent prompts must use `git add -- <specific paths>` instead of `git add -A` to prevent accidentally committing untracked repository files. <!-- [^9a2c7-13] -->

## Repository Hygiene

`android/.fastembed_cache/` must not be tracked in git and must be listed in `.gitignore`. <!-- [^9a2c7-14] -->
