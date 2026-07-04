---
title: "nmp doctor: Scope, Contract, and Modes"
slug: nmp-doctor
topic: project-status
summary: "`nmp doctor` is an approved diagnostic command with a narrow scope: dependency/source coherence, retired-crate detection, path-dep checks, and informational-onl"
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
---

# nmp doctor: Scope, Contract, and Modes

## Overview

`nmp doctor` is an approved diagnostic command with a narrow scope: dependency/source coherence, retired-crate detection, path-dep checks, and informational-only baseline reporting. The baseline choice is reported but never banned. <!-- [^04745-48314] -->

## Checks

`nmp doctor` runs seven checks:

1. **Source coherence** — verifies that the configured source for each dependency matches what is actually present.
2. **Lockfile agreement** — ensures the lockfile is consistent with declared dependencies.
3. **Retired-crate detection** — flags any crate that appears on the single canonical retired-crate list.
4. **Path-dep validation** — checks that path dependencies resolve and are structurally valid.
5. **Informational-only baseline reporting** — reports the active baseline choice without banning any option.
6. **Companion lockstep** — verifies that companion crates are kept in lockstep with each other.
7. **nmp.toml coherence** — validates that the `nmp.toml` manifest is internally consistent. <!-- [^04745-20125] -->

## Modes

`nmp doctor` exposes three modes:

- **Exit-code mode** — returns a non-zero exit code when issues are detected, suitable for CI and scripting.
- **`--json`** — emits machine-readable JSON output for programmatic consumption.
- **`--strict`** — escalates warnings to errors, causing a non-zero exit on any finding. <!-- [^04745-20125] -->

## Non-Goals

`nmp doctor` explicitly does **not**:

- **Auto-fix** issues — it is diagnostic-only; remediation is left to the user.
- **Policy-gate `master` tracking** — it does not enforce or block any policy around tracking the `master` branch.
- **Perform network access** — all checks run locally against on-disk state. <!-- [^04745-20125] -->
