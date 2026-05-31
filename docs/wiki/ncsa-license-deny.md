---
title: NCSA License Restriction in cargo deny
slug: ncsa-license-deny
summary: NCSA (University of Illinois/NCSA Open Source License) is not allowed in cargo deny
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
---

# NCSA License Restriction in cargo deny

## NCSA License Denial

NCSA (University of Illinois/NCSA Open Source License) is not allowed in cargo deny. When no NCSA-licensed crates remain in the dependency tree, the vestigial NCSA allowlist entry in deny.toml is removed. [^3a906-4]

## See Also

