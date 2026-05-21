//! Zero-diff CI gate proving `apps/fixture/nmp-app-fixture` is a **generated**
//! consumer of `nmp-codegen`.
//!
//! Opus review #49 flagged this crate as having zero generated consumers: both
//! `apps/*/ffi.rs` were hand-written, so the 1.5kLOC of generator code was
//! shipped-but-inert. The fix is to make the fixture's source tree a function
//! of the generator's output, then guard the equivalence with this test.
//!
//! ## What this test enforces
//!
//! Given `apps/fixture/nmp.toml`, regenerating every file in
//! `apps/fixture/nmp-app-fixture/` (`Cargo.toml`, `src/{action,capability,
//! domain,envelope,ffi,lib,update,view_spec}.rs`) produces **byte-identical**
//! contents to what is committed. Any drift — generator change, hand edit to
//! the generated tree — fails CI on this single assertion.
//!
//! ## Why the live tree, not a temp dir
//!
//! `tests/determinism.rs` already proves the generator is itself deterministic
//! against a synthetic temp-dir manifest. That covers the *generator*; it does
//! not cover the *consumer*. This test closes the loop: the fixture's
//! committed source IS the generator's output. A hand edit to either side
//! breaks the gate, which is exactly the property we want.
//!
//! ## Why this satisfies review #49
//!
//! With this test in place, `nmp-codegen` has at least one live consumer whose
//! correctness is structurally tied to the generator. The generator can no
//! longer drift away from any user; if it does, this fails.

use std::path::PathBuf;

/// The committed `nmp-app-fixture` source tree MUST be byte-identical to what
/// `nmp_codegen::generate_modules` emits for `apps/fixture/nmp.toml`.
///
/// If this fails, regenerate with:
/// ```text
/// cargo run -p nmp-codegen -- gen modules \
///     --manifest apps/fixture/nmp.toml \
///     --out apps/fixture/nmp-app-fixture
/// ```
/// and commit the result.
#[test]
fn committed_fixture_matches_generator_output() {
    let (manifest, out) = fixture_paths();

    let check = nmp_codegen::check_modules(&manifest, &out)
        .expect("check_modules must succeed when the manifest + output paths are valid");

    assert!(
        check,
        "apps/fixture/nmp-app-fixture has drifted from the codegen output.\n\
         Regenerate with:\n\
             cargo run -p nmp-codegen -- gen modules \\\n\
                 --manifest apps/fixture/nmp.toml \\\n\
                 --out apps/fixture/nmp-app-fixture\n\
         and commit the result."
    );
}

/// Resolve the workspace-relative paths from this crate's `CARGO_MANIFEST_DIR`
/// so the test runs regardless of the directory it is invoked from.
///
/// Layout: `crates/nmp-codegen/` → workspace root is two levels up; the
/// fixture lives at `apps/fixture/`.
fn fixture_paths() -> (PathBuf, PathBuf) {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must have a grandparent (the workspace root)")
        .to_path_buf();
    let manifest = workspace_root.join("apps/fixture/nmp.toml");
    let out = workspace_root.join("apps/fixture/nmp-app-fixture");
    (manifest, out)
}
