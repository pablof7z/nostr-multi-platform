//! Positive D13 fixture — must trigger at least one D13 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.
//!
//! Marker opt-in: the smoke test uses `--d13-extra-scope` to reach this
//! file regardless of path. To also exercise the marker-driven opt-in
//! mechanism, the file includes the canonical D13 Part-A marker comment
//! below — the rule should still fire either way.

// D13: signer-only seal path

pub fn send_dm_with_local_key_gate() {
    // Part A: raw key access on a DM seal path — banned by D13. Any of
    // these substrings on a non-comment, non-test line fires the rule.
    let _ = identity.active_local_keys();
    let _ = keys.secret_key();
    let _ = Keys::parse("nsec1...");
}

pub fn read_mls_local_nsec_from_a_dm_path() {
    // Part A also bans `mls_local_nsec` reads on the DM seal path —
    // the ADR-0025 raw-key escape is not a DM concern.
    let _ = app.mls_local_nsec();
}
