//! Integration test for [`nmp_defaults::register_defaults`].
//!
//! Spins up a real [`NmpApp`] via `nmp_app_new`, calls `register_defaults`,
//! and asserts that every canonical action namespace is registered. #1996:
//! registration presence is read directly from the action registry via the
//! `registered_action_namespaces()` test-support introspection probe — the
//! authoritative registration view — rather than dispatching an empty `{}`
//! body through the retired JSON `nmp_app_dispatch_action` doorway. A
//! registered namespace appears in that set; an unregistered one does not.
//! That asymmetry is the lightest proof the template actually wires what it
//! claims to wire.

use nmp_ffi::{nmp_app_free, nmp_app_new};

#[test]
fn register_defaults_wires_every_canonical_namespace() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    for ns in nmp_codegen::canonical_default_action_namespaces() {
        assert!(
            is_registered(app, ns),
            "namespace `{ns}` was not registered by `register_defaults`"
        );
    }

    // Confirm a genuinely-unregistered namespace is absent from the registry —
    // proves the above test is not vacuous.
    assert!(
        !is_registered(app, "nmp.template.never.registered"),
        "control case: an unregistered namespace must NOT appear in the action registry"
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
fn register_defaults_with_handles_returns_wot_runtime_when_social_is_enabled() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults::default(),
    );

    assert!(
        handles.wot.is_some(),
        "default social composition must return the installed WOT runtime handle"
    );
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    assert!(
        app_ref
            .registered_typed_projection_keys()
            .contains(&"nmp.wot.bootstrap".to_string()),
        "handle-returning entry point must preserve WOT bootstrap projection registration"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_with_handles_omits_wot_runtime_when_social_is_disabled() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults {
            social: false,
            ..Default::default()
        },
    );

    assert!(
        handles.wot.is_none(),
        "social:false must not install or return the WOT runtime handle"
    );
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    assert!(
        !app_ref
            .registered_typed_projection_keys()
            .contains(&"nmp.wot.bootstrap".to_string()),
        "social:false must not register the WOT bootstrap projection"
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
/// pubkey and enqueues `EnsureInterest` / `DropInterestOwner` — it produced no
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
    for ns in nmp_codegen::substrate_action_namespaces() {
        assert!(
            is_registered(app, ns),
            "substrate namespace `{ns}` must be wired by `register_substrate`"
        );
    }

    // Social actions are NOT wired — the discriminating half of the proof.
    for ns in nmp_codegen::social_action_namespaces()
        .into_iter()
        .chain(nmp_codegen::dm_action_namespaces())
        .chain(nmp_codegen::zap_action_namespaces())
    {
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
    for ns in nmp_codegen::canonical_default_action_namespaces() {
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

/// A custom `nostrconnect_perms` is consumed without panic (#1493 P9). NMP ships
/// no perm set by default — which event kinds an app requests is leaf-app
/// product policy — so a leaf app supplies `Some(perms)` here. This guards the
/// plumbing/consume path; the perms value surfaces only through the
/// `nostrconnect://` URI built on the FFI thread.
#[test]
fn register_defaults_with_accepts_custom_nostrconnect_perms() {
    let app = nmp_app_new();
    let cfg = nmp_defaults::NmpDefaults {
        nostrconnect_perms: Some("sign_event:1,sign_event:7".to_string()),
        ..Default::default()
    };
    nmp_defaults::register_defaults_with(unsafe { &mut *app }, cfg);
    // Substrate + social still wired.
    assert!(is_registered(app, "nmp.nip65.publish_relay_list"));
    assert!(is_registered(app, "nmp.follow"));
    nmp_app_free(app);
}

/// `true` when `namespace` is present in the app's action registry — read
/// directly from the authoritative `registered_action_namespaces()` probe
/// (#1996: replaces the retired `nmp_app_dispatch_action("{}")` registration
/// probe). Registration presence is exactly what every caller asserts, so the
/// introspection view is both more direct and more correct than inferring it
/// from a dispatch error envelope.
fn is_registered(app: *mut nmp_ffi::NmpApp, namespace: &str) -> bool {
    // SAFETY: `app` is a valid non-null pointer from `nmp_app_new`, live for
    // the duration of this read; no aliasing `&mut` is held at the call sites.
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    app_ref
        .registered_action_namespaces()
        .iter()
        .any(|ns| ns == namespace)
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

