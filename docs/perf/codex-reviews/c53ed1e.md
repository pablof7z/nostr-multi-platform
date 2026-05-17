# Codex Review — c53ed1e

**Commit:** `c53ed1e docs(framework-magic): adopt D0-D8 canonical doctrine set (PD-001 resolution)`
**Review session:** `019e3846-4d6c-7200-ad6e-44492593f068`
**Model:** gpt-5.5
**Date:** 2026-05-18
**Tokens used:** 79,913

## Scope

T19 reconciliation pass: framework-magic docs aligned with canonical D0-D8 doctrine set. Reviewed:
- 5 TBD-from-research markers replaced with concrete file:line citations
- NDK kind:3 overclaim corrected (NDK has no unified kind:3→open-subscription rewire in core)
- C1 marked PARTIAL (kind:3 and kind:10002 tie-break pending M3)
- C3 and C4 marked fully PENDING M3
- C13 marked PARTIAL (author_picture_url still Option<String>)
- D6/D7/D8 doctrine numbers adopted throughout

## Verdict

**Issues found (5); 4 require fixes:**

1. **README.md:23, :95, :108** — stale D0-D5 wording; T19 still described as "in flight." Update to D0-D8 and mark reconciliation complete. *(Fixed in follow-up commit 76769d9)*

2. **docs/design/framework-magic/signers.md:62** — wrong D-number mapping. "No plaintext nsec crosses FFI" is not D6 (D6 is errors-never-FFI-as-exceptions). Keep as key-security invariant; tie signer failures to D6, not the nsec boundary. *(Fixed in follow-up commit 175632b)*

3. **docs/design/framework-magic.md:30 and :42** — index table overclaims C1 as `[DONE] kernel` and C13 as `[DONE] in placeholder shape`. Both sub-files already say PARTIAL. Table should match. *(Fixed in follow-up commit 175632b)*

4. **docs/design/framework-magic/replaceable.md:12** — bad citation range `ingest.rs:187-185`; actual `ingest_profile` path is `166-184`. *(Fixed in follow-up commit 175632b)*

5. **docs/design/framework-magic/replaceable.md:66** — says "the in-memory kernel can run the NIP-40 timer" but line 80 correctly says the current kernel does not parse or schedule NIP-40 expiration. Remove the timer claim; C4 is fully pending M3. *(Fixed in follow-up commit 175632b)*

**File size note:** `docs/product-spec/overview-and-dx.md` is 353 LOC, over the 300-line soft limit. The codex-reviews/*.md files are generated transcripts and treated as exempt.

## Overall assessment

PASS after fixes. No doctrine violations introduced; the reconciliation correctly reflects the current kernel state (partial C1, no C3/C4/C13 completion). All 5 findings addressed in follow-up commits `76769d9` (README) and `175632b` (signers, table, replaceable).
