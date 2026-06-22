//! Typed-only byte-doorway gate for the **FULL production Chirp composition**
//! (ADR-0064 / #1756).
//!
//! # Why this gate exists (the false signal it fixes)
//!
//! The byte doorway (`nmp_app_dispatch_action_bytes` →
//! `ActionRegistry::start_bytes`) is TYPED-ONLY: every action module reachable
//! through it MUST decode a typed FlatBuffers payload (override
//! `ActionModule::decode_payload` to return `Some`). A JSON-only /
//! no-`decode_payload` module is rejected `NotTypedCapable` and so is
//! unreachable through the byte doorway.
//!
//! The sibling gate in `nmp-defaults`
//! (`tests/typed_only_action_doorway_gate.rs`) only walks
//! `nmp_defaults::register_defaults` — the canonical NMP default set. That is a
//! STRICT SUBSET of what the production Chirp app wires: Chirp also registers
//! NIP-29 group actions, the cross-protocol visible-note-relations action, the
//! NIP-47 wallet stack, and (under the `marmot` feature) the Marmot MLS seam.
//! Asserting "all typed" over `register_defaults` alone therefore reported a
//! FALSE GREEN while the full Chirp composition still carried JSON-only modules.
//!
//! This gate spins up a REAL [`NmpApp`] and runs the actual production
//! composition root — `nmp_app_chirp_register` (the exact C-ABI entry point the
//! iOS shell links against) plus the Marmot `ActionModule` registration the
//! `marmot`-featured iOS build performs at sign-in — then asserts the untyped
//! set equals a frozen RATCHET allowlist of genuinely-remaining JSON-only
//! producers.
//!
//! # Migration ratchet (ADR-0064 is per-crate, in-flight)
//!
//! ADR-0064 migrates each action crate to a typed FlatBuffers payload
//! INDIVIDUALLY; the JSON doorway (`nmp_app_dispatch_action`) still exists for
//! not-yet-migrated modules and is deleted only at Cut B. So a handful of
//! production modules legitimately remain JSON-only today. Rather than assert
//! ZERO untyped modules (which would falsely fail on those in-flight modules),
//! this gate pins the untyped set to a frozen ALLOWLIST. The allowlist is a
//! RATCHET:
//!
//! * a NEW untyped module (not on the list) → FAILS the gate (regression: no
//!   one may add a JSON-only module to the production composition without an
//!   explicit, reviewed allowlist entry);
//! * migrating a listed module to typed without removing it from the list →
//!   FAILS the gate (forcing the allowlist to SHRINK toward empty as ADR-0064
//!   completes; at Cut B the JSON doorway and this allowlist both reach zero —
//!   that is Cut B *across the full composition*, not merely the default set).

use nmp_ffi::{nmp_app_free, nmp_app_new, NmpApp};

use super::super::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus};

/// Production Chirp modules NOT yet migrated to a typed FlatBuffers payload —
/// they ride the JSON doorway (`nmp_app_dispatch_action`) only and are rejected
/// `NotTypedCapable` by the byte doorway. This is the ADR-0064 migration backlog
/// for the FULL Chirp composition. It MUST only shrink: each removal is a crate
/// (or single module) that finished its typed migration.
///
/// Each entry documents the owning crate + the reason it is still JSON-only.
/// Cut B for the full composition = this list reaches empty.
///
/// NOTE on `marmot`: `nmp.marmot` is produced by the `nmp-marmot` crate and is
/// only registered when the `marmot` cargo feature is on (the iOS shell builds
/// with `--features marmot`; the default test/CI build does NOT). The allowlist
/// below is feature-gated to match the composition actually built, so the gate
/// stays exact in both configurations.
const MIGRATION_PENDING_UNTYPED: &[&str] = &[
    // Owner: nmp-relations (`VisibleNoteRelationsModule`). The cross-protocol
    // visible-note-relations claim/release action is still a serde-tagged enum
    // with no `decode_payload` override (see
    // `crates/nmp-relations/src/visible_relations.rs`). Typed migration pending.
    "nmp.nip01.visible_note_relations",
    // Owner: nmp-nip29 (`DiscoverGroupsAction`). The group-discovery action is
    // the lone NIP-29 module still without a `decode_payload` override — its
    // siblings (post/react/share/repost/create/join/leave/put_user/invite) are
    // already typed (#1837/#1838). Typed migration pending
    // (`crates/nmp-nip29/src/action/discover.rs`).
    "nmp.nip29.discover",
];

/// `nmp.marmot` is JSON-only (`MarmotActionModule` overrides no
/// `decode_payload`; see `crates/nmp-marmot/src/projection/action.rs`). Owner:
/// nmp-marmot (the MLS-over-Nostr seam). Only present under `--features marmot`,
/// so it is a SEPARATE feature-gated allowlist entry.
#[cfg(feature = "marmot")]
const MARMOT_PENDING_UNTYPED: &str = "nmp.marmot";

/// Build the EXACT production Chirp composition on a fresh real [`NmpApp`]:
/// `nmp_app_chirp_register` (= `register_defaults` + NIP-29 actions +
/// visible-note-relations + NIP-47 wallet + zaps/group/op-feed projections),
/// and, under `--features marmot`, the `MarmotActionModule` the iOS shell
/// registers at sign-in. Returns the live `(app, handle)`; the caller must
/// `nmp_app_chirp_unregister(handle)` then `nmp_app_free(app)`.
fn build_full_chirp_composition() -> (*mut NmpApp, *mut ChirpHandle) {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`; a null
    // viewer_pubkey is the explicitly-permitted "no viewer" case.
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "nmp_app_chirp_register must succeed for the full-composition gate (status={status})"
    );
    assert!(!handle.is_null(), "register returned null handle on Ok");

    // Under `--features marmot` the iOS shell additionally registers the
    // `MarmotActionModule` against the kernel action registry at sign-in
    // (`nmp_marmot::ffi::register_with_keys` → `register_action(MarmotActionModule)`).
    // The heavyweight Marmot SERVICE (MLS SQLite DB + keyring) is NOT needed to
    // exercise the byte-doorway invariant — only the `ActionModule` is, and it
    // is the same value the production path registers. Registering it directly
    // keeps the gate faithful to the wire-reachable module set without forcing a
    // real signer/store/keyring into a unit test.
    #[cfg(feature = "marmot")]
    {
        // SAFETY: `app` is valid and not aliased here (the register call above
        // dropped its borrow; the handle holds only a copy of the raw pointer).
        unsafe { &mut *app }
            .register_action(nmp_marmot::projection::action::MarmotActionModule);
    }

    (app, handle)
}

/// Sorted expected untyped set for the composition actually built (feature
/// dependent).
fn expected_untyped() -> Vec<String> {
    let mut expected: Vec<String> =
        MIGRATION_PENDING_UNTYPED.iter().map(|s| (*s).to_string()).collect();
    #[cfg(feature = "marmot")]
    expected.push(MARMOT_PENDING_UNTYPED.to_string());
    expected.sort();
    expected
}

/// THE production gate: after the FULL Chirp composition is wired, the untyped
/// (JSON-doorway-only) action-module set is EXACTLY the frozen migration
/// allowlist — no more (no new JSON-only module slipped into the composition),
/// no fewer (a migrated module must be struck from the allowlist). Everything
/// else is typed (ADR-0064 / #1756 — the byte doorway is typed-only).
#[test]
fn full_chirp_composition_untyped_modules_match_the_migration_allowlist() {
    let (app, handle) = build_full_chirp_composition();

    // SAFETY: `app` is a valid non-null pointer with no live aliases.
    let untyped = unsafe { &mut *app }.untyped_action_namespaces(); // already sorted
    let expected = expected_untyped();

    assert_eq!(
        untyped, expected,
        "the untyped (JSON-doorway-only) action-module set of the FULL Chirp \
         composition must equal the frozen ADR-0064 migration allowlist. A \
         namespace present here but NOT in the allowlist is a NEW JSON-only \
         module in the production composition (forbidden — the byte doorway is \
         typed-only, #1756). A namespace in the allowlist but absent here \
         finished its typed migration — strike it from \
         `MIGRATION_PENDING_UNTYPED` (or `MARMOT_PENDING_UNTYPED`) so the \
         ratchet shrinks toward empty (Cut B = empty across the full \
         composition)."
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// A deliberately JSON-only module — `serde_json::Value` action, NO
/// `decode_payload` override. Reachable through the byte doorway only if the
/// typed-only invariant regresses; the gate must flag it.
struct JsonOnlyAppModule;
impl nmp_core::substrate::ActionModule for JsonOnlyAppModule {
    const NAMESPACE: &'static str = "test.json_only_full_composition_gate"; // doctrine-allow: D9 — test-only namespace inside a #[cfg(test)] test; never on the wire
    type Action = serde_json::Value;
    // `decode_payload` left defaulted (`None`) — the forbidden JSON-only shim.

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// LOAD-BEARING negative: register a JSON-only module ON TOP OF the full Chirp
/// composition and prove the probe FLAGS its namespace (and adds EXACTLY one
/// entry). If `untyped_action_namespaces()` (or the underlying
/// `is_typed_capable` probe) ever stopped distinguishing typed from JSON-only
/// modules, this would go green-when-it-should-be-red and the positive gate
/// above would be vacuous. Together they prove the gate fires iff a JSON-only
/// module is reachable through the byte doorway of the real composition.
#[test]
fn gate_flags_a_json_only_module_added_to_the_full_composition() {
    let (app, handle) = build_full_chirp_composition();

    // SAFETY: `app` is valid and not aliased here.
    let app_mut = unsafe { &mut *app };
    let before = app_mut.untyped_action_namespaces();
    assert_eq!(
        before,
        expected_untyped(),
        "precondition: the full composition's untyped set is the allowlist"
    );
    assert!(
        !before.contains(&"test.json_only_full_composition_gate".to_string()),
        "precondition: the test JSON-only namespace is not yet registered"
    );

    // Introduce the forbidden JSON-only shim into the real composition.
    app_mut.register_action(JsonOnlyAppModule);

    let after = app_mut.untyped_action_namespaces();
    assert!(
        after.contains(&"test.json_only_full_composition_gate".to_string()),
        "the typed-only gate must flag the JSON-only module's namespace \
         (proving it is load-bearing, not vacuous); got: {after:?}"
    );
    assert_eq!(
        after.len(),
        before.len() + 1,
        "registering one JSON-only module must add EXACTLY one untyped namespace"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
