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

use nmp_ffi::{NmpApp, nmp_app_free, nmp_app_new};

use super::super::{
    ChirpHandle, NmpRegisterStatus, nmp_app_chirp_register, nmp_app_chirp_unregister,
};

#[cfg(feature = "marmot")]
use super::helpers::{MarmotTestRegistration, register_marmot_for_test};

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
/// All previously-pending modules have been migrated to typed FlatBuffers
/// payloads on origin/master:
///  - `nmp.nip01.visible_note_relations`: `decode_payload` added in #1838
///    (`crates/nmp-relations/src/visible_relations.rs:68`).
///  - `nmp.nip29.discover`: `decode_payload` added in #1838
///    (`crates/nmp-nip29/src/action/discover.rs:54`).
/// The default-feature untyped set is now empty. Cut B for the full
/// composition (excluding the `marmot` feature) is REACHED.
const MIGRATION_PENDING_UNTYPED: &[&str] = &[];

struct FullChirpComposition {
    app: *mut NmpApp,
    handle: *mut ChirpHandle,
    #[cfg(feature = "marmot")]
    marmot: Option<MarmotTestRegistration>,
}

impl Drop for FullChirpComposition {
    fn drop(&mut self) {
        #[cfg(feature = "marmot")]
        drop(self.marmot.take());

        if !self.handle.is_null() {
            nmp_app_chirp_unregister(self.handle);
            self.handle = std::ptr::null_mut();
        }
        if !self.app.is_null() {
            nmp_app_free(self.app);
            self.app = std::ptr::null_mut();
        }
    }
}

/// Build the EXACT production Chirp composition on a fresh real [`NmpApp`]:
/// `nmp_app_chirp_register` (= `register_defaults` + NIP-29 actions +
/// visible-note-relations + NIP-47 wallet + zaps/group/op-feed projections),
/// and, under `--features marmot`, the `MarmotActionModule` the iOS shell
/// registers at sign-in. Returns the live `(app, handle)`; the caller must
/// `nmp_app_chirp_unregister(handle)` then `nmp_app_free(app)`.
fn build_full_chirp_composition() -> FullChirpComposition {
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

    let mut composition = FullChirpComposition {
        app,
        handle,
        #[cfg(feature = "marmot")]
        marmot: None,
    };

    // Under `--features marmot` the iOS shell additionally registers the
    // `MarmotActionModule` against the kernel action registry at sign-in
    // (`nmp_marmot::ffi::register_with_secret_hex` →
    // `register_action(MarmotActionModule::new(projection))`).
    // This gate does not drive a Marmot operation; it only needs the same
    // action module the production registration path installs. Use the
    // production Rust helper so the module construction stays in `nmp-marmot`
    // and Chirp does not regain direct MDK/test-storage dependencies.
    #[cfg(feature = "marmot")]
    {
        composition.marmot = Some(register_marmot_for_test(app, "typed-only"));
    }

    composition
}

/// Sorted expected untyped set for the composition actually built.
/// After M14-1c (#2169), `nmp.marmot` is now TYPED (`MarmotActionModule`
/// overrides `decode_payload`) so it is no longer in the untyped allowlist.
/// The full composition (including `--features marmot`) has zero untyped modules.
fn expected_untyped() -> Vec<String> {
    MIGRATION_PENDING_UNTYPED
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// THE production gate: after the FULL Chirp composition is wired, the untyped
/// (JSON-doorway-only) action-module set is EXACTLY the frozen migration
/// allowlist — no more (no new JSON-only module slipped into the composition),
/// no fewer (a migrated module must be struck from the allowlist). Everything
/// else is typed (ADR-0064 / #1756 — the byte doorway is typed-only).
#[test]
fn full_chirp_composition_untyped_modules_match_the_migration_allowlist() {
    let composition = build_full_chirp_composition();

    // SAFETY: `app` is a valid non-null pointer with no live aliases.
    let untyped = unsafe { &mut *composition.app }.untyped_action_namespaces(); // already sorted
    let expected = expected_untyped();

    assert_eq!(
        untyped, expected,
        "the untyped (JSON-doorway-only) action-module set of the FULL Chirp \
         composition must equal the frozen ADR-0064 migration allowlist. A \
         namespace present here but NOT in the allowlist is a NEW JSON-only \
         module in the production composition (forbidden — the byte doorway is \
         typed-only, #1756). A namespace in the allowlist but absent here \
         finished its typed migration — strike it from \
         `MIGRATION_PENDING_UNTYPED` so the \
         ratchet shrinks toward empty (Cut B = empty across the full \
         composition)."
    );

    drop(composition);
}

/// A deliberately JSON-only module — `serde_json::Value` action, NO
/// `decode_payload` override. Reachable through the byte doorway only if the
/// typed-only invariant regresses; the gate must flag it.
struct JsonOnlyAppModule;
impl nmp_core::substrate::ActionModule for JsonOnlyAppModule {
    const NAMESPACE: &'static str = "test.json_only_full_composition_gate"; // doctrine-allow: action_namespace — test-only namespace inside a #[cfg(test)] test; never on the wire
    type Action = serde_json::Value;
    // `decode_payload` left defaulted (`None`) — the forbidden JSON-only shim.

    fn execute(
        &self,
        _ctx: &nmp_core::substrate::ActionContext,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
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
    let composition = build_full_chirp_composition();

    // SAFETY: `app` is valid and not aliased here.
    let app_mut = unsafe { &mut *composition.app };
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
    let _ = app_mut.register_action(JsonOnlyAppModule);

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

    drop(composition);
}
