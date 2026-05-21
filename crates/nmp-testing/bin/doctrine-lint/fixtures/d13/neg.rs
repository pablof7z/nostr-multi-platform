//! Negative D13 fixture — must produce zero D13 findings.
//!
//! Every line that would otherwise trip the rule is either:
//! - inside a comment;
//! - covered by the per-line `// doctrine-allow: D13 — reason` opt-out;
//! - or replaced by the legitimate `SignerForSeal` seam call.
//!
//! Marker opt-in: the file carries the canonical D13 Part-A marker so
//! it is in scope; the smoke test additionally opts in via
//! `--d13-extra-scope`.

// D13: signer-only seal path

pub fn send_dm_via_signer_seam() {
    // Clean: resolve a `SignerForSeal` via the actor's identity runtime
    // and hand it to `gift_wrap_with_signer`. No raw key reads on the
    // DM path — D13 must stay silent.
    let signer = identity.active_signer_for_seal();
    let _ = nmp_nip59::gift_wrap_with_signer(&signer, &receiver, rumor, 0);
}

pub fn explicit_per_line_optout_is_honored() {
    // The per-line escape hatch suppresses the rule for a single
    // legitimately-raw-key call (e.g. a recovery-path utility). The
    // standard `// doctrine-allow: D13 — reason` shape matches the
    // pattern used by every other rule.
    let _ = identity.active_local_keys(); // doctrine-allow: D13 — recovery path
}
