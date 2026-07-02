//! Negative D13 fixture — must produce zero D13 findings.
//!
//! Every line that would otherwise trip the rule is either:
//! - inside a comment;
//! - covered by the per-line `// doctrine-allow: D13 — reason` opt-out;
//! - or replaced by the legitimate ADR-0072 §D5 signer-port seam.
//!
//! Marker opt-in: the file carries the canonical D13 Part-A marker so
//! it is in scope; the smoke test additionally opts in via
//! `--d13-extra-scope`.

// D13: signer-only seal path

pub fn send_dm_via_signer_port() {
    // Clean: pin the active account via `active_account_pubkey` and sign the
    // seal through the port (`Nip44EncryptForAccount` → `SignEventForAccount`).
    // No raw key reads on the DM path — D13 must stay silent.
    let signer_hex = ctx.active_account_pubkey();
    let _ = ctx.nip44_encrypt_for_account(receiver, rumor, signer_hex, |_| {});
}

pub fn explicit_per_line_optout_is_honored() {
    // The per-line escape hatch suppresses the rule for a single
    // legitimately-raw-key call (e.g. a recovery-path utility). The
    // standard `// doctrine-allow: D13 — reason` shape matches the
    // pattern used by every other rule.
    let _ = identity.active_local_keys(); // doctrine-allow: D13 — recovery path
}
