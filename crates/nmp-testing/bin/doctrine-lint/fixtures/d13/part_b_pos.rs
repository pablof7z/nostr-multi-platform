//! D13 Part-B positive fixture — must trigger ≥1 D13 finding.
//!
//! Part B (`marmot_local_nsec` outside `crates/nmp-marmot/`) is path-derived
//! and doesn't honor the extra-scope hook the same way Part A does — Part B
//! fires whenever the file is outside the marmot/testing/ffi/actor carve-out.
//! The smoke test stages this fixture under
//! `target/doctrine_lint_d13_part_b_pos/` and runs the lint there directly;
//! `nmp-testing/` is exempt by design, so the test relies on the staged path
//! falling outside that exemption when scanned.

pub fn read_marmot_local_nsec_from_some_app_crate() {
    // A non-marmot crate reaching into the ADR-25 raw-key slot is the
    // exact leak Part B forbids.
    let _ = app.marmot_local_nsec();
}
