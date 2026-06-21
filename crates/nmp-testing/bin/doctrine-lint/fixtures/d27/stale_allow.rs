// D27 stale-allow fixture (#1712) — a `// doctrine-allow: D27` marker on a line
// that carries NO D27 violation. The hardening must flag this as a stale allow
// so a dead marker (left behind after a relocation PR removed the underlying
// projection-label / display-helper) cannot silently rot.
//
// Placed under fixtures/ so the walker never scans it during a real sweep;
// opted into D27 scope via --d27-extra-scope in the smoke test.

/// Typed projection row. The `pubkey` field is a raw protocol value — perfectly
/// D27-clean — yet still carries a leftover allow. That marker is STALE and must
/// be flagged.
#[derive(Debug)]
pub struct StaleAllowRow {
    pub pubkey: String, // doctrine-allow: D27 — leftover from a removed _label field
}

/// A genuine banned call that is LEGITIMATELY allowed must NOT be reported as
/// stale: the marker silences a real finding, so the line is compliant and the
/// stale path must never trigger here.
fn build(pk: &str) -> String {
    to_npub(pk) // doctrine-allow: D27 — exercises the legit-suppression path
}

// A comment-only line that merely QUOTES the marker text is documentation, not
// an escape, and must NOT be flagged stale: write `// doctrine-allow: D27 —
// reason` on the offending line itself. (Guards the comment-only false positive.)
