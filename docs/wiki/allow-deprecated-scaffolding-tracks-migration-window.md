---
title: #[allow(deprecated)] with ADR Comments Is a Tracked Migration Obligation
slug: allow-deprecated-scaffolding-tracks-migration-window
summary: When #[allow(deprecated)] appears alongside an ADR or architectural comment, it marks a live migration obligation — the deprecated symbol must not be deleted until every such callsite is migrated.
tags:
  - deprecation
  - migration
  - adr
  - discovery
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:322d163a-59eb-4c02-8604-009b4ae4d9b0
---

# #[allow(deprecated)] with ADR Comments Is a Tracked Migration Obligation

> In this codebase, `#[allow(deprecated)]` annotations that appear alongside an ADR reference or explanatory architectural comment are not noise to suppress — they are explicit, tracked migration obligations. The deprecated symbol remains load-bearing until every such callsite has been migrated per the referenced ADR.

## Details

- **Pattern to recognize**: A callsite annotated with both `#[allow(deprecated)]` and a comment like `// ADR-XXXX: migrating to ...` or `// tracked by ADR-XXXX` signals that the deprecation is intentional and the migration is in-progress or scheduled.
- **Example**: `nmp_app_chirp_snapshot` was marked `#[deprecated]`, but all callers used `#[allow(deprecated)]` with comments referencing ADR-0037. This means the symbol is still actively used and the migration window is open.
- **Do not delete**: A deprecated symbol with outstanding `#[allow(deprecated)]` callsites **cannot** be safely removed. Doing so will break compilation at those sites.
- **Migration completion check**: Before removing a deprecated symbol, grep the entire workspace for `#[allow(deprecated)]` in proximity to the symbol name. All such sites must be migrated first.
- **Do not silently suppress**: If you encounter `#[allow(deprecated)]` without an ADR or comment, treat it as a code smell and add a tracking comment rather than leaving it unexplained.
- **ADR lookup**: The referenced ADR document describes the target architecture and the migration path. Consult it before attempting any refactor of the deprecated symbol or its replacements.

## See Also
- [[half-landed-migration-is-not-done|A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations]] — related guide
