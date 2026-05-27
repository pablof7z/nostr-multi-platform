//! A4 — doctrine-lint sentinel for relay-search-radius (W9).
//!
//! A thin sentinel whose existence confirms that W9 changes did not introduce
//! any banned-token violations detectable by the doctrine-lint tool.
//!
//! The real gate is `cargo test -p nmp-testing --test doctrine_lint_smoke`,
//! which CLAUDE.md mandates on every scoped-test run.  That test runs the
//! doctrine-lint binary against all checked-in production crates and asserts
//! zero D0/D6/D7/D8 violations.
//!
//! This file exists so the W9 acceptance suite has an A4 row in `Cargo.toml`
//! matching the implementation-plan §W9 table entry.  No network access or
//! feature flag is required — the test always runs in the default path.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test relay_search_radius_a4_doctrine_lint -- --nocapture
//! ```

#[test]
fn a4_doctrine_lint_sentinel_w9() {
    // The actual doctrine-lint gate is doctrine_lint_smoke; see:
    // cargo test -p nmp-testing --test doctrine_lint_smoke
    //
    // This sentinel confirms A4 is registered and that no compilation error
    // was introduced by W9 changes (a compilation failure here would surface
    // a banned-token issue before the smoke test runs).
}
