//! Integration test for [`nmp_defaults::register_defaults`].
//!
//! Spins up a real [`NmpApp`] via `nmp_app_new`, calls `register_defaults`,
//! and asserts that every canonical action namespace is reachable through
//! the standard FFI dispatch seam (`nmp_app_dispatch_action`). A registered
//! namespace round-trips a `correlation_id`; an unregistered namespace
//! comes back with an `error` field. That asymmetry is the lightest proof
//! the template actually wires what it claims to wire.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free, nmp_app_new, nmp_free_string};

/// All action namespaces [`nmp_defaults::register_defaults`] is
/// contracted to register.
const EXPECTED_NAMESPACES: &[&str] = &[
    // NIP-02 — substrate-level social graph (follow / unfollow / react).
    "nmp.follow",
    "nmp.unfollow",
    "nmp.nip25.react",
    // NIP-17 — DM send + DM-relay-list publish.
    "nmp.nip17.send",
    "nmp.nip17.publish_relay_list",
    // NIP-57 — lightning zap.
    "nmp.nip57.zap",
    // NIP-65 — relay-list publish (absorbed into nmp-router).
    "nmp.nip65.publish_relay_list",
];

#[test]
fn register_defaults_wires_every_canonical_namespace() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    for ns in EXPECTED_NAMESPACES {
        let result = dispatch(app, ns, "{}");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("dispatch returned non-JSON");

        // A registered namespace either accepts (correlation_id) or
        // rejects on input-shape validation (error). The single failure
        // mode that proves NON-registration is "unknown namespace" — the
        // registry returns an error whose message contains the namespace
        // and the phrase "unknown". So: anything OTHER than
        // unknown-namespace counts as "registered".
        if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
            assert!(
                !err.to_ascii_lowercase().contains("unknown"),
                "namespace `{ns}` was not registered by `register_defaults` \
                 (dispatch error: {err})"
            );
        }
        // If we got a correlation_id, registration is unambiguously proven.
    }

    // Confirm a genuinely-unregistered namespace surfaces the
    // unknown-namespace error — proves our above test is not vacuous.
    let bogus = dispatch(app, "nmp.template.never.registered", "{}");
    let parsed: serde_json::Value = serde_json::from_str(&bogus).expect("bogus reply not JSON");
    let err = parsed
        .get("error")
        .and_then(|e| e.as_str())
        .expect("unregistered namespace must surface an error");
    assert!(
        err.to_ascii_lowercase().contains("unknown"),
        "control case: expected unknown-namespace error, got: {err}"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_is_repeatable_for_routing_and_runtime_slots() {
    // Composition root may legitimately re-run `register_defaults` (e.g.
    // a host that rebuilds its `NmpApp` factory). Action namespaces are
    // de-duplicated by the registry; routing-substrate / coverage-hook
    // slots are last-writer-wins; ingest parsers register additively (a
    // duplicate parser is harmless — the kernel calls all parsers for a
    // kind). The proof: a second call does not panic.
    let app = nmp_app_new();
    // SAFETY: same as above.
    nmp_defaults::register_defaults(unsafe { &mut *app });
    nmp_defaults::register_defaults(unsafe { &mut *app });
    nmp_app_free(app);
}

#[test]
fn register_defaults_wires_wot_bootstrap_projection() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    nmp_defaults::register_defaults(unsafe { &mut *app });

    // The generic JSON lane is deleted (rule A6). Check via the typed registry.
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    assert!(
        typed_keys.contains(&"nmp.wot.bootstrap".to_string()),
        "WOT bootstrap typed projection was not registered"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_longform_is_typed_only_not_in_json_map() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    // The generic JSON lane is fully deleted (rule A6). The NIP-23 longform
    // projection was already typed-only before this PR; that contract now holds
    // trivially for all projections. Verify the typed projection IS registered.
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    assert!(
        typed_keys.contains(&"nmp.nip23.articles".to_string()),
        "longform projection must be registered in the typed projection registry"
    );

    nmp_app_free(app);
}

/// The NIP-57 zap-subscription reconciler no longer registers a snapshot
/// projection: it was re-homed onto the generic per-tick observer seam
/// (`AppHost::register_snapshot_tick_observer`) because it only diffs the active
/// pubkey and enqueues `PushInterest` / `WithdrawInterest` — it produced no
/// projection data. After the re-home (and with the entire JSON lane deleted per
/// rule A6 / PR #1525), `"nmp.nip57.zap_subscription"` must NOT appear in the
/// typed projection registry at all.
#[test]
fn register_defaults_zap_subscription_is_no_longer_a_projection_key() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    // The generic JSON lane is deleted (rule A6). Check the typed registry.
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    assert!(
        !typed_keys.contains(&"nmp.nip57.zap_subscription".to_string()),
        "zap_subscription must NOT appear in the typed projections registry — it is a \
         per-tick observer now, not a projection"
    );

    nmp_app_free(app);
}

// ───────────────────────────────────────────────────────────────────────
// Tier split (`register_substrate`) + config struct (`register_defaults_with`)
// ───────────────────────────────────────────────────────────────────────

/// The action namespaces that belong to the SUBSTRATE tier — the routing
/// crate's own relay-list publish action. `register_substrate` alone must wire
/// this (routing is broken without the kind:10002 publish path).
const SUBSTRATE_NAMESPACES: &[&str] = &["nmp.nip65.publish_relay_list"];

/// The action namespaces that belong to the SOCIAL tier — `register_substrate`
/// alone must NOT wire any of these (they are preferences, not correctness).
const SOCIAL_NAMESPACES: &[&str] = &[
    "nmp.follow",
    "nmp.unfollow",
    "nmp.nip25.react",
    "nmp.nip25.unreact",
    "nmp.nip17.send",
    "nmp.nip17.publish_relay_list",
    "nmp.nip57.zap",
];

/// `register_substrate` alone yields a **routable but social-free** composition:
/// the substrate relay-list action dispatches, but NONE of the social action
/// bundles do. This is the `MinimalPlugins`-analog floor a non-social external
/// consumer (podcast-player, hl) stands on without swallowing the social bundle.
#[test]
fn register_substrate_is_routable_but_social_free() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // Substrate tier ONLY — note the default coverage gate, matching what
    // `register_defaults` passes.
    nmp_defaults::register_substrate(
        unsafe { &mut *app },
        nmp_coverage_gate::CoverageGate::default(),
    );

    // Substrate action(s) ARE wired (anything other than unknown-namespace).
    for ns in SUBSTRATE_NAMESPACES {
        assert!(
            is_registered(app, ns),
            "substrate namespace `{ns}` must be wired by `register_substrate`"
        );
    }

    // Social actions are NOT wired — the discriminating half of the proof.
    for ns in SOCIAL_NAMESPACES {
        assert!(
            !is_registered(app, ns),
            "social namespace `{ns}` must NOT be wired by `register_substrate` alone \
             (substrate tier is social-free)"
        );
    }

    // Social runtimes (WOT / DM) are likewise absent from the JSON registry.
    assert!(
        read_projection(app, "nmp.wot.bootstrap").is_none(),
        "WOT bootstrap must NOT be wired by `register_substrate` alone"
    );
    assert!(
        read_projection(app, "nmp.nip17.dm_relay_list").is_none(),
        "DM runtime must NOT be wired by `register_substrate` alone"
    );

    nmp_app_free(app);
}

/// `register_defaults_with(default)` ≡ `register_defaults`: the same action
/// namespaces register, and the same social runtimes appear. This pins the
/// "zero behaviour change for `register_defaults()`" contract.
#[test]
fn register_defaults_with_default_equals_register_defaults() {
    // Reference app via the legacy entry point.
    let app_ref = nmp_app_new();
    nmp_defaults::register_defaults(unsafe { &mut *app_ref });

    // Candidate app via the new config entry point with the default config.
    let app_cfg = nmp_app_new();
    nmp_defaults::register_defaults_with(
        unsafe { &mut *app_cfg },
        nmp_defaults::NmpDefaults::default(),
    );

    // Every canonical action namespace registers in BOTH (and the bogus one in
    // neither). Substrate + social.
    for ns in EXPECTED_NAMESPACES {
        assert_eq!(
            is_registered(app_ref, ns),
            is_registered(app_cfg, ns),
            "namespace `{ns}` registration must match between register_defaults and \
             register_defaults_with(default)"
        );
        assert!(
            is_registered(app_cfg, ns),
            "namespace `{ns}` must be registered by register_defaults_with(default)"
        );
    }

    // Social runtime projections present in both.
    for key in ["nmp.wot.bootstrap", "nmp.nip17.dm_relay_list"] {
        assert_eq!(
            read_projection(app_ref, key).is_some(),
            read_projection(app_cfg, key).is_some(),
            "projection `{key}` presence must match between the two entry points"
        );
        assert!(
            read_projection(app_cfg, key).is_some(),
            "projection `{key}` must be present under register_defaults_with(default)"
        );
    }

    nmp_app_free(app_ref);
    nmp_app_free(app_cfg);
}

/// Each social toggle, when `false`, skips exactly its own block and leaves the
/// substrate floor intact.
#[test]
fn register_defaults_with_toggles_skip_their_blocks() {
    // `social: false` → no nip02 actions, no WOT runtime; substrate still routable.
    {
        let app = nmp_app_new();
        let cfg = nmp_defaults::NmpDefaults {
            social: false,
            ..Default::default()
        };
        nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
        assert!(
            !is_registered(app, "nmp.follow"),
            "social:false must skip nip02"
        );
        assert!(
            read_projection(app, "nmp.wot.bootstrap").is_none(),
            "social:false must skip the WOT runtime"
        );
        // Substrate floor intact.
        assert!(
            is_registered(app, "nmp.nip65.publish_relay_list"),
            "substrate must remain wired regardless of social toggle"
        );
        // Other toggles untouched.
        assert!(is_registered(app, "nmp.nip17.send"), "dms still on");
        assert!(is_registered(app, "nmp.nip57.zap"), "zaps still on");
        nmp_app_free(app);
    }

    // `dms: false` → no nip17 actions, no DM runtime projection.
    {
        let app = nmp_app_new();
        let cfg = nmp_defaults::NmpDefaults {
            dms: false,
            ..Default::default()
        };
        nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
        assert!(
            !is_registered(app, "nmp.nip17.send"),
            "dms:false must skip nip17"
        );
        assert!(
            read_projection(app, "nmp.nip17.dm_relay_list").is_none(),
            "dms:false must skip the DM runtime projection"
        );
        assert!(is_registered(app, "nmp.follow"), "social still on");
        nmp_app_free(app);
    }

    // `zaps: false` → no nip57 action.
    {
        let app = nmp_app_new();
        let cfg = nmp_defaults::NmpDefaults {
            zaps: false,
            ..Default::default()
        };
        nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
        assert!(
            !is_registered(app, "nmp.nip57.zap"),
            "zaps:false must skip nip57"
        );
        assert!(is_registered(app, "nmp.follow"), "social still on");
        nmp_app_free(app);
    }

    // `longform: false` is observed via the TYPED registry, not the JSON map
    // (longform is typed-only). We can at least assert the rest stay on and the
    // call doesn't panic; the typed-projection absence is asserted in-crate.
    {
        let app = nmp_app_new();
        let cfg = nmp_defaults::NmpDefaults {
            longform: false,
            ..Default::default()
        };
        nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
        assert!(is_registered(app, "nmp.follow"), "social still on");
        assert!(
            is_registered(app, "nmp.nip65.publish_relay_list"),
            "substrate still on"
        );
        nmp_app_free(app);
    }
}

/// A custom `nostrconnect_bootstrap_relay` is consumed without panic. The relay
/// value itself is not exposed through a read seam, so this guards the
/// plumbing/consume path. (#1493: the default is now `None` — NMP ships no relay
/// URL — so a leaf app supplies `Some(url)` here.)
#[test]
fn register_defaults_with_accepts_custom_bootstrap_relay() {
    let app = nmp_app_new();
    let cfg = nmp_defaults::NmpDefaults {
        nostrconnect_bootstrap_relay: Some("wss://relay.example.test".to_string()),
        ..Default::default()
    };
    nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
    // Substrate + social still wired.
    assert!(is_registered(app, "nmp.nip65.publish_relay_list"));
    assert!(is_registered(app, "nmp.follow"));
    nmp_app_free(app);
}

/// `true` when dispatching `namespace` does NOT surface an unknown-namespace
/// error — i.e. the namespace is registered (it may still reject `{}` on input
/// shape, which still proves registration).
fn is_registered(app: *mut nmp_ffi::NmpApp, namespace: &str) -> bool {
    let result = dispatch(app, namespace, "{}");
    let parsed: serde_json::Value =
        serde_json::from_str(&result).expect("dispatch returned non-JSON");
    if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
        return !err.to_ascii_lowercase().contains("unknown");
    }
    // A correlation_id (no error) is unambiguous registration.
    true
}

/// Check typed projection registry for key presence — replaces deleted JSON lane (rule A6).
fn read_projection(app: *mut nmp_ffi::NmpApp, key: &str) -> Option<String> {
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    let typed_keys = app_ref.registered_typed_projection_keys();
    if typed_keys.contains(&key.to_string()) {
        // Return a sentinel non-null string; callers using `.is_some()` / `.is_none()` still work.
        Some(String::from("{}"))
    } else {
        None
    }
}

fn dispatch(app: *mut nmp_ffi::NmpApp, namespace: &str, action_json: &str) -> String {
    let ns_c = CString::new(namespace).unwrap();
    let json_c = CString::new(action_json).unwrap();
    let raw = nmp_app_dispatch_action(app, ns_c.as_ptr(), json_c.as_ptr());
    assert!(!raw.is_null(), "dispatch returned null for `{namespace}`");
    // SAFETY: `raw` is a fresh non-null C string owned by `nmp-core`.
    let s = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(raw);
    s
}
