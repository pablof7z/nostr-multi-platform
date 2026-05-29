---
title: The `main` Branch Is a Divergent Orphan — Use `master`
slug: main-branch-is-orphan-not-default
summary: The local `main` branch is a divergent orphan from May-21 epoch, not a master mirror. The canonical default branch is `master`.
tags:
  - git
  - branches
  - master
  - main
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
---

# The `main` Branch Is a Divergent Orphan — Use `master`

> The local `main` branch is a divergent orphan from May-21 epoch, not a master mirror. The canonical default branch is `master`.

## Overview

The local branch named `main` in this repository is **not** a master mirror or a standard Git default branch. It is a divergent orphan from the May-21 epoch with ~577 unique commits and 389 commits behind current master. It does not exist on `origin`. The canonical default branch is `master`. [^752b5-18]

## Do Not Confuse main With master

- **`master`** — the canonical default branch; all PRs target master; CI runs against master.
- **`main`** — a local-only orphan branch, created during the May-21 divergent-history epoch. It has its own root commit not reachable from master. It carries no live features that aren’t already in master or in named keep-branches.

Do not rebase, cherry-pick from, or push `main` to origin. Do not assume tools or scripts that reference `main` are pointing at the right default branch for this project. [^752b5-19]

## See Also

