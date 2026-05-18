# T154 — Doctrine-Lint D6 Findings Audit (LMDB Code)

**Date:** 2026-05-18
**Task:** T154 pre-fix audit (read-only; fixes blocked pending T141 substrate-types extract on `a9ceb35f`)
**Tool:** `cargo run -p nmp-testing --bin doctrine-lint -- --crate nmp-core --allow-findings`
**Actual count:** 45 (T136b/HB58b reported 44 — one net addition on current master, see delta note below)

---

## Count Delta vs. Prior Report

HB58b logged "44 doctrine-lint findings (all pre-existing LMDB D6, unchanged)."  
The current run yields **45** findings. The delta is +1 in `planner/selection.rs:242` — a `.expect(...)` call on a `max_by().map().expect()` chain that was present in `selection.rs` before T136b but has only now been caught (likely due to a minor re-run path change). This finding is **outside LMDB territory** (see Classification below).

---

## Finding Classification Table

Legend for "Territory":
- **LMDB-test** — inside `src/store/lmdb/` but in a file that is `#[cfg(all(test, ...))]`-declared in `mod.rs`; the D6 check fires because the filename does not match the linter's test-file exemption patterns (`tests.rs`, `test_support.rs`, `*_tests.rs`, `/tests/`).
- **non-LMDB** — outside `src/store/lmdb/`; safe to fix independently before T141 lands.

### Group 1 — `planner/selection.rs` (1 finding, non-LMDB)

| # | File:Line | Violation | Territory | Fix Class |
|---|-----------|-----------|-----------|-----------|
| 1 | `crates/nmp-core/src/planner/selection.rs:242` | `.expect("coverage non-empty checked above")` on `Iterator::max_by(...).map(...).expect(...)` | **non-LMDB production** | Convert to `ok_or(KernelError::Invariant(...))` propagated with `?` |

**Note:** The loop at line 226 breaks if `coverage.is_empty()`, so the `expect` message is correct that non-emptiness is invariant at this call site. The right fix is a `debug_assert!` guard (which D6 does not flag) or a `doctrine-allow: D6 — invariant: coverage non-empty checked at line 226` annotation. No refactor of return type needed since the calling function already returns `Result`.

---

### Group 2 — `store/lmdb/test_fixtures.rs` (9 findings, LMDB-test)

Root cause: `test_fixtures.rs` is declared as `#[cfg(all(test, feature = "lmdb-backend"))] mod test_fixtures;` in `mod.rs`, but the D6 linter's `file_is_test_only()` only exempts `tests.rs`, `test_support.rs`, `*_tests.rs`, and files under `/tests/`. The name `test_fixtures.rs` matches none of those patterns.

| # | File:Line | Violation | Fix Class |
|---|-----------|-----------|-----------|
| 2 | `test_fixtures.rs:16` | `.expect("tempdir")` on `tempdir()` | Whitelist the filename OR rename to `test_fixtures_tests.rs` OR `doctrine-allow` annotation |
| 3 | `test_fixtures.rs:17` | `.expect("open")` on `LmdbEventStore::open(...)` | Same |
| 4 | `test_fixtures.rs:34` | `.expect("sign")` on `EventBuilder::sign_with_keys(...)` | Same |
| 5 | `test_fixtures.rs:35` | `.expect("json")` on `ev.try_as_json()` | Same |
| 6 | `test_fixtures.rs:36` | `.expect("parse")` on `serde_json::from_str(...)` | Same |
| 7 | `test_fixtures.rs:52` | `.expect("sign")` on `EventBuilder::sign_with_keys(...)` | Same |
| 8 | `test_fixtures.rs:53` | `.expect("json")` on `ev.try_as_json()` | Same |
| 9 | `test_fixtures.rs:54` | `.expect("parse")` on `serde_json::from_str(...)` | Same |
| 10 | `test_fixtures.rs:X` | (count mismatch: 9 findings in this file per output) | Same |

**Actual lint output lines 2–10** (per `--allow-findings` run):
```
test_fixtures.rs:16:24  .expect("tempdir")
test_fixtures.rs:17:49  .expect("open")
test_fixtures.rs:34:37  .expect("sign")
test_fixtures.rs:35:32  .expect("json")
test_fixtures.rs:36:32  .expect("parse")
test_fixtures.rs:52:36  .expect("sign")
test_fixtures.rs:53:32  .expect("json")
test_fixtures.rs:54:32  .expect("parse")
```

That is 8 findings in `test_fixtures.rs`, not 9. (The count was off in the table header — corrected below.)

---

### Group 3 — `store/lmdb/tests_kind5.rs` (36 findings, LMDB-test)

Root cause: `tests_kind5.rs` ends in `s.rs` not `_tests.rs`, so the linter's suffix check (`name.ends_with("_tests.rs")`) does not fire. The file is declared `#[cfg(all(test, feature = "lmdb-backend"))] mod tests_kind5;` in `mod.rs`.

All 36 findings are `.unwrap()` on `Result`-returning store operations inside test assertion bodies. These are legitimate test-code panics — the correct fix is the linter exemption, not converting test code to `?`-propagation.

| Range | Count | Pattern | Fix Class |
|-------|-------|---------|-----------|
| lines 28–46 | 9 | `.unwrap()` on `store.insert(...)`, `EventId::from_slice(...)`, `EventBuilder::sign_with_keys(...)`, `try_as_json()`, `from_str(...)` | Linter whitelist / rename |
| lines 59–78 | 7 | `.unwrap()` on same patterns (second test function) | Same |
| lines 103–123 | 5 | `.unwrap()` on `store.insert(...)` + query operations | Same |
| lines 143–164 | 5 | `.unwrap()` on `store.insert(...)` + query operations | Same |
| lines 181–204 | 10 | `.unwrap()` on multi-source insert + `delete_by_filter(...)` | Same |

---

## Summary Table

| File | Count | Territory | All False-Positives? |
|------|-------|-----------|----------------------|
| `planner/selection.rs` | 1 | non-LMDB production | No — genuine D6 finding; short fix |
| `store/lmdb/test_fixtures.rs` | 8 | LMDB-test (linter blind spot) | Yes — test-only code, should be exempt |
| `store/lmdb/tests_kind5.rs` | 36 | LMDB-test (linter blind spot) | Yes — test-only code, should be exempt |
| **Total** | **45** | | |

---

## False-Positive Analysis

44 of 45 findings are **false positives** caused by a gap in the linter's `file_is_test_only()` function. The two affected filenames are:

- `test_fixtures.rs` — does not match `["tests.rs", "test_support.rs"]` nor `*_tests.rs` suffix.
- `tests_kind5.rs` — the `_tests.rs` suffix check requires the exact suffix; `tests_kind5.rs` fails because the word `tests` appears at the _start_ of the filename not the end.

The declarations in `mod.rs` both carry `#[cfg(all(test, feature = "lmdb-backend"))]` gates, which semantically make them test-only. The linter cannot see the parent `mod.rs` attribute, hence the filename-exemption workaround — and the exemption list is incomplete.

---

## Genuine D6 Violation (Safe to Fix Now)

Finding #1: `planner/selection.rs:242` is outside LMDB territory, is production code (not test-only), and is a real D6 violation. It is safe to fix now in a separate small PR without any conflict with T141.

**Proposed fix (two options):**
1. Add `// doctrine-allow: D6 — invariant: Iterator::max() on non-empty coverage, emptiness checked at line 226` — smallest change, no signature impact, appropriate for an invariant that is locally obvious.
2. Replace `.expect(...)` with `ok_or_else(|| KernelError::Invariant("coverage non-empty".into()))?` — converts the caller to propagate `Result`. Check the return type of the enclosing function (`select_relays` or similar) before choosing.

**Recommendation:** Option 1 is faster and maintains the invariant documentation inline. Option 2 is more doctrinally pure.

---

## Remediation Plan for the 44 LMDB-Test False-Positives

**Fix strategy: extend `file_is_test_only()` to cover both filenames.**

In `crates/nmp-testing/bin/doctrine-lint/rules/d6.rs`, change:

```rust
const TEST_FILE_NAMES: &[&str] = &["tests.rs", "test_support.rs"];
```

to:

```rust
const TEST_FILE_NAMES: &[&str] = &["tests.rs", "test_support.rs", "test_fixtures.rs"];
```

And add a prefix-check alongside the suffix-check:

```rust
// `tests_*.rs` is the mirror convention: a test-only file whose cfg(test)
// gate lives in the parent module's `#[cfg(all(test,...))] mod tests_foo;`
// declaration. Match both `*_tests.rs` (suffix) and `tests_*.rs` (prefix).
if name.ends_with("_tests.rs") || name.starts_with("tests_") {
    return true;
}
```

This would exempt `tests_kind5.rs` (starts with `tests_`) and `test_fixtures.rs` (exact-name match) without expanding the exemption surface to arbitrary names.

**Alternative:** Add `// doctrine-allow: D6 — test-only fixture code` to each of the 44 lines. This is mechanical but clutters the code and requires 44 edits.

**Preferred:** Extend the linter — one change cleans all 44 findings.

**Blocking dependency:** This touches `crates/nmp-testing/bin/doctrine-lint/rules/d6.rs`, which has no overlap with T141's substrate-types extract. It can be dispatched as a separate tiny PR **right now**. The LMDB `.rs` files themselves (`test_fixtures.rs`, `tests_kind5.rs`) are not modified — only the linter rule is extended.

---

## Dispatch Plan

| Priority | Task | Files Touched | Blocked by T141? |
|----------|------|---------------|------------------|
| P1 (now) | Fix linter `file_is_test_only()` | `crates/nmp-testing/bin/doctrine-lint/rules/d6.rs` | No |
| P1 (now) | Fix `planner/selection.rs:242` genuine D6 | `crates/nmp-core/src/planner/selection.rs` | No |
| P2 (post-T141) | Verify count drops to 0 after linter fix | — | No, but wait to confirm T141 doesn't add more |

---

## Whitelisting Candidates

After the linter fix lands, if any `tests_*.rs` or `test_fixtures.rs` files introduced by future T-tasks trigger the same false-positive, they are automatically covered by the `starts_with("tests_")` extension. No per-file `doctrine-allow` annotations are needed.

---

*Generated by T154 audit agent. Do not edit — regenerate from source if stale.*
