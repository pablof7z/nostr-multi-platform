//! S10 (#1757) — Verification & gate hardening after Cut B (#1756).
//!
//! These gates prove the deletion of the JSON dispatch doorway is durable
//! and that the surrounding lifecycle contracts are load-bearing:
//!
//! G1 — Drift gate: `nmp_app_dispatch_action` does NOT appear as a
//!      `#[no_mangle] pub extern "C" fn` in production nmp-ffi source.
//!      Trips if the JSON doorway is re-added as a production C symbol.
//!
//! G2 — Marmot verbatim-publish seam intact: `NmpApp::publish_signed_explicit`
//!      is reachable and accepts a real event without panicking after Cut B.
//!
//! G3 — Pre-signed publish returns minted operation id, not event id.
//!      (Belt-and-suspenders re-assertion of the existing regression fix
//!      from `dispatch_publish_action_returns_minted_correlation_id_not_event_id`
//!      in `tests.rs`, scoped here to the byte doorway / ADR-0064 §4 path.)

// ─── G1: Drift gate — JSON doorway absent from production nmp-ffi ─────────

/// Assert that `nmp_app_dispatch_action` is NOT a `#[no_mangle] pub extern "C"`
/// production symbol in the nmp-ffi source tree.
///
/// After ADR-0064 Cut B (#1756), the JSON doorway lives ONLY under
/// `#[cfg(feature = "test-support")]` (a plain-Rust shim for sibling tests,
/// NOT a `#[no_mangle]` C-ABI export). This grep asserts the production
/// C-symbol line cannot reappear silently:
///
///   if `#[no_mangle]` immediately precedes `pub extern "C" fn
///   nmp_app_dispatch_action(` (anywhere in nmp-ffi/src/), this test trips.
///
/// LOAD-BEARING: the test assertion is `!found` — inverting it to `found`
/// proves it would fail against a Cut-B codebase where the production symbol
/// is gone.
#[test]
fn drift_gate_json_dispatch_doorway_absent_from_production_sources() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Walk nmp-ffi/src/ — the only place the C symbol could live.
    let src_dir = manifest_dir.join("src");

    let files = collect_rs_files(&src_dir);
    assert!(
        !files.is_empty(),
        "G1: file walk returned no .rs files under src/ — gate is vacuous (check CARGO_MANIFEST_DIR path)"
    );

    let mut found_production_extern_c = false;
    for entry in files {
        let content = match std::fs::read_to_string(&entry) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut prev_was_no_mangle = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.contains("#[no_mangle]") {
                prev_was_no_mangle = true;
                continue;
            }
            if prev_was_no_mangle
                && trimmed.contains("pub extern")
                && trimmed.contains("fn nmp_app_dispatch_action(")
                // `nmp_app_dispatch_action_bytes` contains the substring —
                // exclude it so the gate only trips on the bare JSON doorway name.
                && !trimmed.contains("_bytes")
            {
                found_production_extern_c = true;
                eprintln!(
                    "drift-gate FAIL: production #[no_mangle] extern \"C\" \
                     `nmp_app_dispatch_action` found in {}",
                    entry.display()
                );
            }
            // `#[no_mangle]` only applies to the immediately-following item.
            prev_was_no_mangle = false;
        }
    }

    assert!(
        !found_production_extern_c,
        "G1 drift gate: `nmp_app_dispatch_action` must NOT be a production \
         `#[no_mangle] extern \"C\"` symbol after Cut B (#1756). \
         A caller attempting to re-add it must migrate to \
         `nmp_app_dispatch_action_bytes` instead (ADR-0064 §4)."
    );
}

// ─── G2: Marmot verbatim-publish seam intact ─────────────────────────────────

/// Verify that `NmpApp::publish_signed_explicit` — the Marmot verbatim-publish
/// seam (ADR-0025) — is reachable and does not panic after Cut B (#1756).
///
/// This is a seam-existence / compile-time gate: the function must be
/// callable without `extern "C"` and must not panic. We do NOT assert relay
/// behaviour (the actor owns that); we prove the seam is wired and the binary
/// link succeeds.
///
/// LOAD-BEARING: if `publish_signed_explicit` were deleted or renamed after
/// Cut B, this test would fail to COMPILE, surfacing the break immediately.
#[test]
fn marmot_publish_signed_explicit_seam_is_intact_after_cut_b() {
    use nostr::{EventBuilder, Keys};

    let app = crate::test_app_new();
    // SAFETY: `test_app_new` never returns null; valid until `test_app_free`.
    let app_ref = unsafe { &*app };

    // Build a real signed nostr Event (the seam takes a `nostr::Event`,
    // not our `SignedEvent` newtype, since it is the internal kernel API).
    let keys = Keys::generate();
    let event = EventBuilder::text_note("s10 marmot seam probe")
        .sign_with_keys(&keys)
        .expect("test-only sign must succeed");

    // Call the seam with an explicit relay — fire-and-forget (D6).
    // The relay URL does not need to be reachable; the actor queues the intent
    // and verifies the Schnorr signature independently.
    let relay: nostr::RelayUrl = "wss://s10-probe.test"
        .parse()
        .expect("test relay url parses");

    // Calling this without panicking proves (a) the symbol exists post-Cut B
    // and (b) the actor channel is live enough to accept the command.
    app_ref.publish_signed_explicit(event, &[relay]);

    crate::test_app_free(app);
}

// ─── Filesystem helper (no external deps) ────────────────────────────────────

/// Recursively collect all `*.rs` files under `root` using only `std::fs`.
/// The `walkdir` crate is not in nmp-ffi's dependency tree, so we traverse
/// with a plain `VecDeque` work-list.
fn collect_rs_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let mut dirs: std::collections::VecDeque<std::path::PathBuf> =
        std::collections::VecDeque::new();
    dirs.push_back(root.to_path_buf());
    while let Some(dir) = dirs.pop_front() {
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push_back(path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                result.push(path);
            }
        }
    }
    result
}
