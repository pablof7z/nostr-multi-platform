---
title: License Allowlist Policy & NCSA Dev-Only Rule
slug: license-allowlist-policy
summary: The NCSA (University of Illinois/NCSA Open Source License) is allowed in the deny.toml license allowlist for transitive dev-only dependencies like libfuzzer-sys
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

# License Allowlist Policy & NCSA Dev-Only Rule

## License Allowlist Policy

The NCSA (University of Illinois/NCSA Open Source License) is allowed in the deny.toml license allowlist for transitive dev-only dependencies like libfuzzer-sys. When no NCSA-licensed crate remains in the dependency tree, the NCSA allowance entry is removed from deny.toml. [^3a906-4]

## See Also

