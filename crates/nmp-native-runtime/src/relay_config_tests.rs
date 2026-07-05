//! Unit + end-to-end tests for the native-runtime relay-config sidecar
//! (`.nmp-relay-config.json`) — load/save round-trip, the `persist_configured_relays`
//! write-through seam (#3059), and nostrconnect relay selection.

use super::*;

fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.push(format!(
        "nmp-relay-config-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn save_then_load_round_trips() {
    let dir = unique_temp_dir("roundtrip");
    let relays = vec![
        (
            "wss://primary-relay.example".to_string(),
            "both,indexer".to_string(),
        ),
        (
            "wss://indexer-relay.example".to_string(),
            "indexer".to_string(),
        ),
    ];
    save(&dir, &relays);
    let loaded = load(&dir).expect("sidecar loads after save");
    assert_eq!(loaded, relays);
}

#[test]
fn load_missing_file_returns_none() {
    let dir = unique_temp_dir("missing");
    // No save() call — the sidecar does not exist.
    assert!(load(&dir).is_none(), "missing sidecar must yield None");
}

#[test]
fn load_empty_array_returns_none() {
    let dir = unique_temp_dir("empty");
    // Persist an explicitly empty list, then confirm load treats it as
    // "nothing configured" (None) so the builder falls back to defaults.
    save(&dir, &[]);
    assert!(
        load(&dir).is_none(),
        "an empty sidecar array must be treated as None, not Some(vec![])"
    );
}

#[test]
fn load_malformed_json_returns_none() {
    let dir = unique_temp_dir("malformed");
    let path = dir.join(RELAY_CONFIG_FILENAME);
    std::fs::write(&path, b"{ this is not valid json").expect("write malformed");
    assert!(load(&dir).is_none(), "unparseable sidecar must yield None");
}

#[test]
fn nostrconnect_configured_selection_uses_router_policy_and_core_roles() {
    let rows = [
        ("read-relay", "read"),
        ("write-relay", "write"),
        ("both-relay", "both"),
    ];

    assert_eq!(
        configured_nostrconnect_relay_url(rows),
        Some("write-relay".to_string())
    );
}

#[test]
fn nostrconnect_configured_selection_accepts_composite_role() {
    let rows = [
        ("indexer-relay", "indexer"),
        ("composite-relay", "both,indexer"),
    ];

    assert_eq!(
        configured_nostrconnect_relay_url(rows),
        Some("composite-relay".to_string())
    );
}

#[test]
fn persist_configured_relays_writes_sidecar_when_storage_dir_present() {
    let dir = unique_temp_dir("persist-some");
    let relays = vec![("wss://nos.lol".to_string(), "read".to_string())];
    persist_configured_relays(Some(&dir), &relays);
    assert_eq!(
        load(&dir).expect("sidecar must exist after persist_configured_relays"),
        relays
    );
}

#[test]
fn persist_configured_relays_is_noop_without_storage_dir() {
    // In-memory apps (`.in_memory()` / no `.storage_path(...)`) have no
    // sidecar location; persist_configured_relays must not fabricate one.
    persist_configured_relays(None, &[("wss://nos.lol".to_string(), "read".to_string())]);
    // Nothing to assert against a real path — the contract under test is
    // simply "does not panic and touches no filesystem state". A dir that
    // was never created cannot be loaded from.
}

#[test]
fn persist_configured_relays_overwrites_stale_first_run_defaults() {
    // Reproduces #3059: the sidecar initially holds only the builder's
    // first-run defaults (no nos.lol). A later runtime relay addition
    // (add_relay dispatch / kind:10002 sync) must overwrite the sidecar
    // with the FULL set, so a subsequent cold-start `load()` sees
    // nos.lol too instead of silently losing it.
    let dir = unique_temp_dir("persist-overwrite");
    let first_run_defaults = vec![("wss://relay.primal.net".to_string(), "both".to_string())];
    save(&dir, &first_run_defaults);
    assert_eq!(load(&dir).unwrap(), first_run_defaults);

    let full_set_after_runtime_edit = vec![
        ("wss://relay.primal.net".to_string(), "both".to_string()),
        ("wss://nos.lol".to_string(), "read".to_string()),
    ];
    persist_configured_relays(Some(&dir), &full_set_after_runtime_edit);

    let reloaded = load(&dir).expect("sidecar must reload after persist");
    assert_eq!(
        reloaded, full_set_after_runtime_edit,
        "cold relaunch must see nos.lol, not just the stale first-run default"
    );
}

#[test]
fn add_relay_dispatch_persists_to_sidecar_end_to_end() {
    // End-to-end reproduction of #3059: a real `NmpApp`, started with a
    // storage-backed sidecar and the builder's first-run defaults (no
    // nos.lol yet), then `add_relay` dispatched mid-session — the same
    // path a Settings-screen edit, or Marmot key-package discovery
    // adding a relay, would take. Before the fix nothing ever re-wrote
    // the sidecar after first start, so a subsequent cold relaunch would
    // silently reload only the stale first-run default and drop
    // nos.lol. This test proves the sidecar now mirrors the live set.
    let dir = unique_temp_dir("e2e-add-relay");
    let dir_str = dir.to_string_lossy().into_owned();

    let app_ptr = crate::NmpAppBuilder::new()
        .storage_path(dir_str.clone())
        .declare_consumed_projections(["profile"])
        .with_relays([("wss://relay.primal.net".to_string(), "both".to_string())])
        .start(crate::RunConfig {
            visible_limit: 16,
            emit_hz: 2,
        });
    assert!(!app_ptr.is_null(), "builder returned a null app pointer");
    // SAFETY: non-null pointer returned by `start`; this test owns it and
    // frees it exactly once below.
    let app = unsafe { &*app_ptr };

    // `wait_barrier_for_test` blocks until the actor has dispatched every
    // command enqueued before it — including `Start`'s own initial
    // `configured_relays` application — so the dispatch below is the
    // FIRST genuine change this test observes.
    assert!(
        app.wait_barrier_for_test(std::time::Duration::from_secs(5)),
        "actor must finish applying Start's initial relays before this test proceeds"
    );

    app.add_relay("wss://nos.lol".to_string(), "read".to_string());
    assert!(
        app.wait_barrier_for_test(std::time::Duration::from_secs(5)),
        "actor must dispatch add_relay before this test asserts on its effects"
    );

    // The actor-side barrier above only proves the KERNEL applied
    // add_relay; the sidecar write happens on the separate
    // update-listener thread once it drains the resulting update frame.
    // Poll with a generous bound instead of a fixed sleep — the listener
    // thread is normally sub-millisecond behind the actor, so this
    // resolves almost immediately and only times out if the persistence
    // wiring is genuinely broken.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let persisted = loop {
        if let Some(rows) = load(&dir) {
            if rows.iter().any(|(url, _)| url == "wss://nos.lol") {
                break rows;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "sidecar never observed nos.lol after add_relay + barrier"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(
        persisted.iter().any(|(url, _)| url == "wss://nos.lol"),
        "add_relay must persist the FULL relay set to the sidecar so a \
         cold relaunch does not drop it; got {persisted:?}"
    );
    assert!(
        persisted
            .iter()
            .any(|(url, _)| url == "wss://relay.primal.net"),
        "the original first-run relay must survive alongside the new one; \
         got {persisted:?}"
    );

    app.stop_runtime();
    // SAFETY: sole owner of `app_ptr`; dropped exactly once.
    unsafe { drop(Box::from_raw(app_ptr)) };
}

#[test]
fn raw_new_app_start_runtime_honors_persisted_relays_over_prestart_reseed() {
    // Reproduces chirp#168: `ChirpApp` (every UniFFI facade) constructs via
    // the raw `new_app()` + `start_runtime` path, NOT the legacy
    // `NmpAppBuilder<RelaysDeclared>::start()` path that loads the on-disk
    // sidecar. Chirp's `seedChirpRelays` unconditionally re-`add_relay`s its
    // two onboarding defaults before every `start()` call (mirrored below).
    // Before the fix, `start_runtime` never consulted the sidecar at all, so
    // a relay set the user configured last session (persisted to the
    // sidecar by the #3059/#3061 change-observer) was silently discarded —
    // the re-seeded defaults became the ONLY relays the kernel ever saw.
    let dir = unique_temp_dir("chirp-168-raw-path");
    let dir_str = dir.to_string_lossy().into_owned();

    // Simulate a previous session's Settings -> Relays edit: the user
    // removed both onboarding defaults and kept only a custom relay. The
    // #3059/#3061 observer would have written exactly this to the sidecar.
    let persisted_custom_set = vec![("wss://custom.example".to_string(), "both".to_string())];
    save(&dir, &persisted_custom_set);

    let app = crate::new_app();
    assert_eq!(
        app.set_storage_path(Some(dir_str.clone())),
        NmpConfigStatus::Ok
    );
    app.consume_all_builtin_projections();

    // Mirrors `ChirpApp::seed_default_relays()` / Chirp's `RelaySeeding.swift`
    // — called unconditionally, pre-start, on every launch.
    app.add_relay(
        "wss://relay.primal.net".to_string(),
        "both,indexer".to_string(),
    );
    app.add_relay("wss://purplepag.es".to_string(), "indexer".to_string());

    app.start_runtime(16, 2);
    assert!(
        app.wait_barrier_for_test(std::time::Duration::from_secs(5)),
        "actor must finish applying Start before this test asserts on its effects"
    );

    let urls: Vec<String> = app
        .configured_relays_handle()
        .lock()
        .map(|rows| {
            rows.as_slice()
                .iter()
                .map(|row| row.url().to_string())
                .collect()
        })
        .unwrap_or_default();

    assert_eq!(
        urls,
        vec!["wss://custom.example".to_string()],
        "the persisted custom relay set must win over the pre-start reseed \
         of Chirp's onboarding defaults; got {urls:?}"
    );

    app.stop_runtime();
}

#[test]
fn raw_new_app_start_runtime_keeps_prestart_reseed_on_genuine_first_run() {
    // Companion to the test above: with NO sidecar on disk (a genuine first
    // run), `start_runtime`'s sidecar-fallback lookup must be a no-op —
    // whatever the host pre-seeded via `add_relay` before start stands, and
    // the change-observer persists it as the sidecar's first-run baseline.
    let dir = unique_temp_dir("chirp-168-first-run");
    let dir_str = dir.to_string_lossy().into_owned();
    // No save() call — the sidecar does not exist yet.

    let app = crate::new_app();
    assert_eq!(
        app.set_storage_path(Some(dir_str.clone())),
        NmpConfigStatus::Ok
    );
    app.consume_all_builtin_projections();
    app.add_relay(
        "wss://relay.primal.net".to_string(),
        "both,indexer".to_string(),
    );
    app.add_relay("wss://purplepag.es".to_string(), "indexer".to_string());

    app.start_runtime(16, 2);
    assert!(
        app.wait_barrier_for_test(std::time::Duration::from_secs(5)),
        "actor must finish applying Start before this test asserts on its effects"
    );

    let urls: Vec<String> = app
        .configured_relays_handle()
        .lock()
        .map(|rows| {
            rows.as_slice()
                .iter()
                .map(|row| row.url().to_string())
                .collect()
        })
        .unwrap_or_default();

    assert_eq!(
        urls,
        vec![
            "wss://relay.primal.net".to_string(),
            "wss://purplepag.es".to_string(),
        ],
        "with no sidecar yet, the pre-start reseed must stand unmodified; got {urls:?}"
    );

    app.stop_runtime();
}

#[test]
fn nostrconnect_relay_url_falls_back_to_registered_bootstrap() {
    let app = crate::new_app();
    assert_eq!(
        app.set_nostrconnect_bootstrap_relay("bootstrap-relay".to_string()),
        NmpConfigStatus::Ok
    );

    assert_eq!(
        app.nostrconnect_relay_url(),
        Some("bootstrap-relay".to_string())
    );
}
