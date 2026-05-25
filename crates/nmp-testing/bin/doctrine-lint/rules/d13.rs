//! D13 — DM-path raw-key isolation (ADR-0026 enforcement).
//!
//! ADR-0026 establishes a single seam — `RemoteSignerHandle::nip44_encrypt` /
//! `nip44_decrypt` plus the `SignerForSeal` indirection in
//! `nmp_nip59::gift_wrap_with_signer` — for every code path that needs sender-
//! held key material to seal a NIP-59 gift-wrap (NIP-17 DMs, future NIP-57
//! zaps, future raw NIP-44 payloads). The point of the seam is that the same
//! call site works for local keys (`nostr::Keys`) and a remote bunker
//! (`Box<dyn RemoteSignerHandle>`); reaching past it to read raw key material
//! on a DM path defeats the whole purpose — a bunker user's send silently
//! breaks once the local-key branch is selected.
//!
//! ## What this catches
//!
//! ### Part A — raw key reads inside a marked DM / zap / NIP-44 path
//!
//! Inside `crates/nmp-core/src/actor/commands/dm.rs` (the canonical NIP-17
//! send handler), or any other in-scope file opted in via the marker
//! comment `// D13: signer-only seal path`, the rule flags substring matches
//! of any of:
//!
//! - `active_local_keys` — `IdentityRuntime::active_local_keys` hands out a
//!   raw `&Keys`; on the seal path it must instead resolve a
//!   `SignerForSeal`.
//! - `active_nsec_bech32` — the bech32 export of the active secret key.
//! - `.secret_key()` — direct read of a `Keys`'s secret half.
//! - `Keys::parse(` — building a `Keys` from a hex/bech32 nsec inside the
//!   DM path.
//! - `mls_local_nsec` — the Marmot ADR-0025 raw-key escape is not a DM
//!   path concern (D13 Part B forbids it outside the marmot crate; Part A
//!   forbids the symbol from leaking into the DM seal path even by name).
//!
//! ### Part B — `mls_local_nsec` reads outside the marmot crate
//!
//! ADR-0025 names exactly one consumer of the `NmpApp::mls_local_nsec`
//! FFI accessor: the `nmp-marmot` MLS bridge, whose group state cannot be
//! recovered without the user's raw nsec. Every other crate — and the
//! kernel itself — must consume key material through the actor's identity
//! runtime, never through the Marmot ADR-0025 escape.
//!
//! In every file outside `crates/nmp-marmot/`, the literal token
//! `mls_local_nsec` triggers D13. Comment lines, the per-line
//! `// doctrine-allow: D13 — reason` opt-out, and `nmp-testing` (this
//! rule's host) are exempt. The `crates/nmp-ffi/` tree (the C-ABI
//! shell that owns the `NmpApp::mls_local_nsec` accessor),
//! `crates/nmp-core/src/slots.rs` (where the slot alias + constructor
//! moved after the step 11-final FFI extraction), and the
//! `crates/nmp-core/src/actor/` tree are exempt too: those define
//! the slot, wire it into the actor, and expose it across the C
//! ABI — they don't *read* it as raw key material; the lint's intent
//! is "no remote callers may dereference this field."
//!
//! ## Scope
//!
//! - **Part A** — `crates/nmp-core/src/actor/commands/dm.rs` plus any file
//!   that opts in via `// D13: signer-only seal path` (the future zap and
//!   raw NIP-44 paths will do this). `#[cfg(test)]` blocks and the
//!   `--d13-extra-scope` test hook are honored exactly like D9.
//! - **Part B** — every file outside `crates/nmp-marmot/`,
//!   `crates/nmp-testing/`, `crates/nmp-core/src/ffi/`, and
//!   `crates/nmp-core/src/actor/`. Future Marmot-equivalent crates with an
//!   ADR-25-style exception can opt in by being added to the marmot-allow
//!   list here.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D13 — reason` opt-out.
//! - `#[cfg(test)]` blocks (inline and parent-module) — Part A only.

use std::path::Path;

pub const ID: &str = "D13";

/// Substrings whose presence in a Part-A-scoped file fires D13. Kept small
/// and exact so a near-miss identifier (`active_local_keys_for_test`) is not
/// silently included — the seam must be explicit.
const PART_A_BANNED: &[&str] = &[
    "active_local_keys",
    "active_nsec_bech32",
    ".secret_key()",
    "Keys::parse(",
    "mls_local_nsec",
];

/// Files that opt in to Part A by default (no marker comment required).
/// The canonical entry is the NIP-17 send handler; future Theme C+ paths
/// (zap, raw NIP-44) will be added here as they land.
const PART_A_DEFAULT_FILES: &[&str] = &[
    "crates/nmp-core/src/actor/commands/dm.rs",
];

/// The opt-in marker. A file containing this comment anywhere in its
/// contents joins the Part-A scope without having to be enumerated here.
pub const PART_A_MARKER: &str = "// D13: signer-only seal path";

/// True iff the file is in Part A's default scope. The marker-driven scope
/// is checked in [`check_part_a`] (it needs the file body, not just the
/// path).
pub fn file_in_part_a_default(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    PART_A_DEFAULT_FILES
        .iter()
        .any(|suffix| s.ends_with(suffix) || s.contains(&format!("/{suffix}")))
}

/// True iff Part B should scan this file.
///
/// Part B's mandate is "outside the marmot crate, no caller reads the
/// ADR-25 raw-key slot." Carve-outs:
///   - `crates/nmp-marmot/` — the legitimate ADR-25 consumer.
///   - `crates/nmp-testing/` — this rule's own host + fixtures.
///   - `crates/nmp-ffi/` — owns the `NmpApp::mls_local_nsec` C-ABI
///     accessor (extracted from `nmp-core::ffi` in step 11-final).
///   - `crates/nmp-core/src/slots.rs` — the slot alias + constructor
///     moved here when the FFI shell extracted; the file declares the
///     type, doesn't dereference it.
///   - `crates/nmp-core/src/actor/` — wires the slot into the actor.
pub fn file_in_part_b_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // Only scan code under `crates/` and `apps/`.
    let in_workspace = s.contains("/crates/")
        || s.starts_with("crates/")
        || s.contains("/apps/")
        || s.starts_with("apps/");
    if !in_workspace {
        return false;
    }
    let is_marmot = s.contains("/crates/nmp-marmot/") || s.starts_with("crates/nmp-marmot/");
    let is_testing =
        s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/");
    let is_ffi_crate =
        s.contains("/crates/nmp-ffi/") || s.starts_with("crates/nmp-ffi/");
    let is_core_slots = s.contains("/crates/nmp-core/src/slots.rs")
        || s.starts_with("crates/nmp-core/src/slots.rs");
    let is_core_actor = s.contains("/crates/nmp-core/src/actor/")
        || s.starts_with("crates/nmp-core/src/actor/");
    !(is_marmot || is_testing || is_ffi_crate || is_core_slots || is_core_actor)
}

/// Per-line Part-A check. Caller has already established that the file is
/// in Part A scope (either via [`file_in_part_a_default`] or via the
/// marker, or via `--d13-extra-scope`).
///
/// Part A is the "raw key access on a DM seal path" rule — banned tokens
/// must not appear in production code; `#[cfg(test)]` is exempt (the
/// dm.rs tests legitimately call `Keys::generate()` for a recipient
/// pubkey).
pub fn check_part_a(
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut out = Vec::new();
    for banned in PART_A_BANNED {
        if let Some(idx) = line.find(banned) {
            let col = idx + 1; // 1-indexed
            out.push((
                col,
                format!(
                    "raw key access `{}` on a DM seal path violates D13 — \
                     ADR-0026 requires routing seal material through the \
                     `SignerForSeal` seam (see `nmp_nip59::gift_wrap_with_signer`)",
                    banned
                ),
                "resolve a `SignerForSeal` via identity (`IdentityRuntime::active_signer_for_seal`) \
                 and hand it to `gift_wrap_with_signer`; do not read raw `Keys` material from this \
                 path"
                    .to_string(),
            ));
        }
    }
    out
}

/// Per-line Part-B check — flags any read of `mls_local_nsec` outside
/// the marmot crate. Caller has already established that the file is in
/// Part B scope via [`file_in_part_b_scope`].
///
/// Comments and the standard per-line opt-out (handled by the driver) are
/// exempt. Test-cfg is NOT exempt — a test that reads `mls_local_nsec`
/// outside the marmot crate is still leaking the ADR-25 escape.
pub fn check_part_b(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let needle = "mls_local_nsec";
    let Some(idx) = line.find(needle) else {
        return Vec::new();
    };
    let col = idx + 1;
    vec![(
        col,
        format!(
            "read of `{}` outside `crates/nmp-marmot/` violates D13 — the \
             ADR-0025 raw-nsec escape has exactly one allowed consumer (the \
             Marmot MLS bridge); every other caller must route through \
             `IdentityRuntime` instead",
            needle
        ),
        "route through `IdentityRuntime` (or the NIP-44 signer seam) — only \
         the `nmp-marmot` crate may read `mls_local_nsec` directly"
            .to_string(),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── Part A path-scope ────────────────────────────────────────────────

    #[test]
    fn dm_rs_is_in_part_a_default_scope() {
        assert!(file_in_part_a_default(&PathBuf::from(
            "crates/nmp-core/src/actor/commands/dm.rs"
        )));
        assert!(file_in_part_a_default(&PathBuf::from(
            "/abs/path/crates/nmp-core/src/actor/commands/dm.rs"
        )));
    }

    #[test]
    fn unrelated_files_are_not_part_a_default() {
        assert!(!file_in_part_a_default(&PathBuf::from(
            "crates/nmp-core/src/actor/commands/publish.rs"
        )));
        assert!(!file_in_part_a_default(&PathBuf::from(
            "crates/nmp-nip17/src/lib.rs"
        )));
    }

    // ── Part A check ─────────────────────────────────────────────────────

    #[test]
    fn part_a_flags_active_local_keys() {
        let hits = check_part_a(
            "    let Some(keys) = identity.active_local_keys() else {",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "expected one D13 finding for active_local_keys");
        assert!(hits[0].1.contains("D13"));
        assert!(hits[0].1.contains("active_local_keys"));
    }

    #[test]
    fn part_a_flags_secret_key() {
        let hits = check_part_a("    let sk = keys.secret_key();", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains(".secret_key()"));
    }

    #[test]
    fn part_a_flags_keys_parse() {
        let hits = check_part_a(
            "    let keys = Keys::parse(\"nsec1...\").expect(\"valid\");",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("Keys::parse"));
    }

    #[test]
    fn part_a_flags_mls_local_nsec() {
        let hits = check_part_a("    let nsec = app.mls_local_nsec();", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("mls_local_nsec"));
    }

    #[test]
    fn part_a_ignores_test_cfg() {
        // Inside `#[cfg(test)]`, the DM tests legitimately call `Keys::generate()`
        // (recipient pubkey) and `Keys::parse(TEST_NSEC)` (sign-in for a fake
        // user). Those must not fire D13.
        let hits = check_part_a(
            "    let recipient_keys = Keys::parse(\"nsec1...\");",
            false,
            true,
        );
        assert!(hits.is_empty(), "test-cfg lines are exempt from Part A");
    }

    #[test]
    fn part_a_ignores_comments() {
        let hits = check_part_a(
            "    // identity.active_local_keys() returns &Keys",
            true,
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn part_a_clean_signer_call_does_not_fire() {
        let hits = check_part_a(
            "    let signer = identity.active_signer_for_seal()?;",
            false,
            false,
        );
        assert!(hits.is_empty(), "the legitimate seam call must not trip D13");
    }

    #[test]
    fn part_a_reports_column_at_token_start() {
        // The 1-indexed column must point at the first character of the
        // offending substring.
        let line = "    let k = identity.active_local_keys().unwrap();";
        let hits = check_part_a(line, false, false);
        assert_eq!(hits.len(), 1);
        let expected_col = line.find("active_local_keys").unwrap() + 1;
        assert_eq!(hits[0].0, expected_col);
    }

    // ── Part B path-scope ────────────────────────────────────────────────

    #[test]
    fn part_b_scope_excludes_marmot() {
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-marmot/src/lib.rs"
        )));
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "/abs/path/crates/nmp-marmot/src/group/encrypted.rs"
        )));
    }

    #[test]
    fn part_b_scope_excludes_nmp_testing() {
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d13/pos.rs"
        )));
    }

    #[test]
    fn part_b_scope_excludes_nmp_ffi_slots_and_actor() {
        // Step 11 final extraction moved the C-ABI shell from
        // `crates/nmp-core/src/ffi/` to the standalone `crates/nmp-ffi/`
        // crate, and the slot alias + constructor (formerly inside
        // `ffi/mod.rs`) to `crates/nmp-core/src/slots.rs`. The actor
        // wiring stayed in `crates/nmp-core/src/actor/`. All three pass
        // `mls_local_nsec` around by name; they're not the "remote
        // caller dereferencing the field" target of Part B.
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-ffi/src/lib.rs"
        )));
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-core/src/slots.rs"
        )));
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-core/src/actor/dispatch.rs"
        )));
    }

    #[test]
    fn part_b_scope_includes_other_crates() {
        // Any other crate or app code reading `mls_local_nsec` is the
        // exact leak Part B forbids.
        assert!(file_in_part_b_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/marmot/ffi.rs"
        )));
        assert!(file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-nip17/src/lib.rs"
        )));
        assert!(file_in_part_b_scope(&PathBuf::from(
            "crates/nmp-core/src/lib.rs"
        )));
    }

    #[test]
    fn part_b_scope_excludes_files_outside_workspace() {
        assert!(!file_in_part_b_scope(&PathBuf::from(
            "/tmp/some/random/file.rs"
        )));
    }

    // ── Part B check ─────────────────────────────────────────────────────

    #[test]
    fn part_b_flags_mls_local_nsec_read() {
        let hits = check_part_b("    let nsec = app.mls_local_nsec();", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("D13"));
        assert!(hits[0].1.contains("mls_local_nsec"));
    }

    #[test]
    fn part_b_ignores_comments() {
        let hits = check_part_b("    // mls_local_nsec is the ADR-25 escape", true);
        assert!(hits.is_empty());
    }

    #[test]
    fn part_b_reports_column_at_token_start() {
        let line = "    let nsec = app.mls_local_nsec().expect(\"set\");";
        let hits = check_part_b(line, false);
        assert_eq!(hits.len(), 1);
        let expected_col = line.find("mls_local_nsec").unwrap() + 1;
        assert_eq!(hits[0].0, expected_col);
    }

    #[test]
    fn part_b_clean_line_does_not_fire() {
        let hits = check_part_b("    let signer = identity.active_signer_kind();", false);
        assert!(hits.is_empty());
    }
}
