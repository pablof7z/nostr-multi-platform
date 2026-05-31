---
title: Coding Standards & File Limits
slug: coding-standards
summary: File size limits are ≤300 soft / 500 hard LOC, with no TODO/unimplemented allowed in non-test code.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ad1d532e-a335-44fb-827e-a3f0318a3aae
  - session:9f5b53f7-ae7d-426c-8a51-d7bba9491624
  - session:423f3c56-7275-4e62-998e-e8f37be564da
---

# Coding Standards & File Limits

## File Size Limits

Files must not exceed 300/500 LOC (soft/hard cap), and PRs must not introduce new hard-cap file-size violations. Files must contain no TODO/unimplemented in non-test code, and must never fabricate results—unreachable/absent behavior against the public relay set must be reported loudly as a finding.

<!-- citations: [^ad1d5-8] [^9f5b5-1] [^423f3-4] -->
## See Also

