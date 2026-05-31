---
title: iOS NMP Component Adoption Workflow
slug: ios-nmp-component-adoption-workflow
summary: Implementation uses parallel Haiku agents working in isolated git worktrees on an integration branch (`ios/nmp-component-adoption`), with each agent's work revi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
---

# iOS NMP Component Adoption Workflow

## Integration Branch Workflow

Implementation uses parallel Haiku agents working in isolated git worktrees on an integration branch (`ios/nmp-component-adoption`), with each agent's work reviewed by a Sonnet agent before merging into the integration branch (no PR), and subsequent agents launched after each merge. When enough significant work accumulates to justify a PR, the integration branch is submitted as a PR and landed into master. [^9a2c7-12]


## Agent Constraints

Haiku agents must never run `cargo test`; that validation happens during the merge process. Workflow agent prompts must use `git add -- <specific paths>` instead of `git add -A` to prevent accidentally committing untracked repository files. [^9a2c7-13]

## Repository Hygiene

`android/.fastembed_cache/` must not be tracked in git and must be listed in `.gitignore`. [^9a2c7-14]
## See Also

