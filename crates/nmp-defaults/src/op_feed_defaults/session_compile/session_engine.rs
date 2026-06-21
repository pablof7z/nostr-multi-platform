//! The generalized session-engine builder for non-default feed scopes (#1740
//! step 3).
//!
//! Every [`nmp_feed::FeedScope`] that is NOT `ActiveUserFollows` (which keeps
//! the framework-default home wiring) compiles through here. The builder is a
//! SESSION WRAPPER over the existing OP-feed mechanics — the same generic engine
//! [`nmp_nip01::op_feed::register_op_feed`] the home feed uses — parameterized
//! on:
//!
//! * a COMPILED, EVENT-AWARE admission predicate (the engine's
//!   [`nmp_feed::RootAdmission`], built INSIDE the framework from resolved
//!   pubkey-set DATA / `#t` tag terms — no app closure crosses the seam) that
//!   gates which roots ENTER the feed (#1740 step 3); and
//! * a set of INTERNAL acquisition interests (NIP-01 filter JSON), registered
//!   via [`nmp_core::ActorCommand::OpenInterest`] under the session's projection
//!   key as `consumer_id` and withdrawn symmetrically on close.
//!
//! The session registers under the caller's UNIQUE [`nmp_feed::ProjectionKey`]
//! (not the home `OP_FEED_SNAPSHOT_KEY`), so many scope sessions coexist. Close
//! tears everything down in reverse order: withdraw each interest, remove the
//! controller + projection, revoke the ingest observer + any resolver observers.
//!
//! Doctrine map:
//! - D0: this names no app product — it consumes a compiled predicate + filter
//!   JSON. The scope→predicate semantics live in `resolve.rs`.
//! - D4: reuses `register_op_feed` + `op_feed_observer` + `OpenInterest`; no
//!   second feed engine, no re-derived filter on close.
//! - D8: each session's interests are withdrawn on close (symmetric teardown).

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{empty_suppression_lookup, KernelEvent};
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_feed::{
    ClosureInterestShape, FeedAdvance, FeedApply, FeedController, FeedReset, FeedSessionBuild,
    PullFeedController, RootAdmission,
};
use nmp_ffi::{FeedOpenError, NmpApp};
use nmp_planner::InterestShape;

use super::resolve::ResolvedScope;

/// Build a registered feed session for a resolved non-default scope and return
/// its teardown recipe.
///
/// `key` is the session's unique projection key (from `params.projection`).
/// `resolved` carries the compiled admission predicate, the acquisition
/// interests, the live acquisition shape, and any resolver observer ids that
/// must be revoked on close.
pub(super) fn build_scope_session(
    app: &NmpApp,
    key: &str,
    resolved: ResolvedScope,
) -> Result<FeedSessionBuild, FeedOpenError> {
    let viewer = super::super::read_active(&app.active_account_handle()).ok_or(
        FeedOpenError::ScopeNotSupportedYet {
            scope: "scope-no-active-account",
        },
    )?;

    let ResolvedScope {
        admission,
        interests,
        live_shape,
        extra_acquisition,
        reset_hooks,
        resolver_observer_ids,
    } = resolved;

    // ── 1. Engine over the COMPILED, EVENT-AWARE admission predicate ──────
    //
    // The compiled predicate IS the engine's ROOT-admission gate (#1740 step 3):
    // a root whose author/tags the perspective does not admit never enters the
    // feed — the perspective filters the rendered feed itself, not merely reply
    // attribution. It is built inside the framework (from resolved DATA or a
    // live framework projection) — nothing app-supplied crosses the seam. A
    // permissive follow-attribution predicate is NOT needed here (a session's
    // attribution still flows through the engine's `follow` gate, which the home
    // path sets; sessions reuse the same observer wiring). We pass the compiled
    // perspective as BOTH so a session admits roots AND attributes replies from
    // in-scope authors only.
    let root_admission: RootAdmission = admission;
    let follow_predicate: nmp_feed::FollowPredicate = {
        let root_admission = root_admission.clone();
        Arc::new(move |pk: &str| {
            // Reply attribution: gate on the author alone (build a minimal
            // author-only event view). For author-scope perspectives this is the
            // exact membership test; tag-scope perspectives never qualify a reply
            // as attribution (a reply carrying no scope tag is correctly dropped).
            let probe = nmp_core::substrate::KernelEvent {
                id: String::new(),
                author: pk.to_string(),
                kind: 0,
                created_at: 0,
                tags: Vec::new(),
                content: String::new(),
                relay_provenance: Vec::new(),
            };
            root_admission(&probe)
        })
    };
    let event_store = app.event_store_handle();
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |id: &nmp_core::substrate::EventId| {
        nmp_core::slots::event_by_id_from_store(&event_store, id)
    });
    let event_lookup_for_observer = event_lookup.clone();
    let engine = nmp_nip01::op_feed::register_op_feed_with_admission(
        viewer,
        follow_predicate,
        root_admission,
        event_lookup,
    );

    // ── 2. Ingest observer ───────────────────────────────────────────────
    let observer = nmp_nip01::op_feed::op_feed_observer(
        engine.clone(),
        event_lookup_for_observer,
        empty_suppression_lookup(),
    );
    let observer_for_registry: Arc<dyn KernelEventObserver> = observer.clone();
    let engine_observer_id = app.register_event_observer(observer_for_registry);

    // ── 3. Pull controller over the live acquisition shape ───────────────
    let provider: Arc<dyn nmp_feed::FeedInterestShape + Send + Sync> = {
        let live_shape = live_shape.clone();
        Arc::new(ClosureInterestShape::new(move || live_shape()))
    };
    let pull = app.feed_pull_fn();
    let apply: FeedApply = {
        let observer = Arc::clone(&observer);
        let engine = Arc::clone(&engine);
        Arc::new(move |event: &KernelEvent| {
            let before = visible_payload(&engine);
            observer.on_kernel_event(event);
            visible_payload(&engine) != before
        })
    };
    let advance: FeedAdvance = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            engine.grow_visible_window();
        })
    };
    let reset: FeedReset = {
        let engine = Arc::clone(&engine);
        Arc::new(move || {
            let had_rows = !engine.snapshot_current_window().cards.is_empty();
            engine.reset_for_perspective_change();
            had_rows
        })
    };
    let controller: Arc<dyn FeedController> =
        PullFeedController::new_with_perspective(provider, pull, apply, None, Some(reset), advance);
    app.register_feed(key.to_string(), controller.clone());

    // ── 3b. Typed NOFS sidecar under the session key ─────────────────────
    //
    // Mirrors the home feed's typed projection so a `NOFS`-aware host renders
    // the session's window from the typed payload (generic `Value` fallback for
    // others). Sessions emit always (no incremental-apply omit bookkeeping — a
    // session feed is short-lived; the home path owns the omit optimization).
    let engine_for_typed = Arc::clone(&engine);
    let typed_key = key.to_string();
    app.register_typed_snapshot_projection(key.to_string(), move || {
        let snapshot = engine_for_typed.snapshot_current_window();
        Some(nmp_core::TypedProjectionData {
            key: typed_key.clone(),
            schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.to_string(),
            schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip01::op_feed::OP_FEED_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot),
            ..Default::default()
        })
    });

    // ── 4+5. Open interests + reactive re-sync ───────────────────────────
    //
    // A session opens INTERNAL acquisition interests via `OpenInterest` under its
    // projection key as `consumer_id`. The seed/list interests are fixed; the
    // MEMBER-timeline interest tracks a set that is empty until the source event
    // (kind:30000 list / kind:3 contacts) lands. So acquisition must re-sync when
    // the resolved set changes — exactly the home feed's `sync_follow_feed_interests`
    // asymmetry, mirrored here.
    //
    // `opened` is the live log of every `(filter_json, scope)` this session has
    // OpenInterest'd (the fixed ones now, the live member shape later). Teardown
    // drains it so EVERY opened interest is withdrawn — no leak even for the
    // dynamically-resolved member interest (D8).
    let sender = app.command_sender();
    let opened: Arc<Mutex<Vec<(String, u32)>>> = Arc::new(Mutex::new(Vec::new()));
    let open_interest = {
        let sender = sender.clone();
        let key = key.to_string();
        let opened = Arc::clone(&opened);
        move |filter_json: String, scope: u32| {
            // Idempotent at the kernel (refcount by shape+consumer+scope); we
            // log each distinct filter once so teardown closes it exactly once.
            if let Ok(mut log) = opened.lock() {
                if log.iter().any(|(f, s)| *f == filter_json && *s == scope) {
                    return;
                }
                log.push((filter_json.clone(), scope));
            }
            let _ = sender.send(ActorCommand::OpenInterest {
                filter_json,
                consumer_id: key.clone(),
                scope,
            });
        }
    };

    // Open the fixed interests resolved at build time.
    for (filter_json, scope) in &interests {
        open_interest(filter_json.clone(), *scope);
    }
    // Open the current dynamic acquisition shapes if already populated.
    sync_member_interest(&extra_acquisition, &open_interest);

    // Wire each underlying-set change to (a) re-sync acquisition for the new
    // members, then (b) reset the window so it regrows under the new perspective.
    for hook in reset_hooks {
        let controller_for_reset = controller.clone();
        let extra = extra_acquisition.clone();
        let open_interest = open_interest.clone();
        let reset_trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            sync_member_interest(&extra, &open_interest);
            let _ = controller_for_reset.reset();
        });
        hook(reset_trigger);
    }

    // ── 6. Teardown recipe (registration order = reverse of execution) ───
    //
    // Execution order on close (the registry reverses the Vec):
    //   1. unregister the controller            (registry removal, runs first)
    //   2. revoke the engine ingest observer
    //   3. revoke each resolver observer
    //   4. remove the projection
    //   5. close each acquisition interest      (WITHDRAW actor-owned state)
    //   6. mark-changed                          (the notification, runs last)
    let teardown_handle = app.feed_teardown();
    let mut teardown: Vec<nmp_feed::TeardownAction> = Vec::new();
    teardown.push(teardown_handle.mark_changed()); // exec #6 (last)
    // exec #5 — close EVERY interest this session opened, draining the live log
    // so dynamically-resolved member interests are withdrawn too (no leak, D8).
    {
        let sender_for_close = sender.clone();
        let key_for_close = key.to_string();
        let opened_for_close = Arc::clone(&opened);
        teardown.push(Box::new(move || {
            let drained = opened_for_close
                .lock()
                .map(|log| log.clone())
                .unwrap_or_default();
            for (filter_json, scope) in drained {
                let _ = sender_for_close.send(ActorCommand::CloseInterest {
                    filter_json,
                    consumer_id: key_for_close.clone(),
                    scope,
                });
            }
        }));
    }
    teardown.push(teardown_handle.remove_projection(key.to_string())); // exec #4
    for id in &resolver_observer_ids {
        teardown.push(teardown_handle.revoke_observer(*id));
    } // exec #3
    teardown.push(teardown_handle.revoke_observer(engine_observer_id)); // exec #2
    teardown.push(teardown_handle.unregister_feed(key.to_string())); // exec #1 (first)

    Ok(FeedSessionBuild {
        projection_key: nmp_feed::ProjectionKey(key.to_string()),
        teardown,
    })
}

fn visible_payload(engine: &nmp_nip01::OpFeedEngine) -> Vec<u8> {
    let snapshot = engine.snapshot_current_window();
    nmp_nip01::op_feed::encode_op_feed_snapshot(&snapshot)
}

/// Open an `OpenInterest` for every CURRENT dynamic acquisition shape (the
/// member timeline, and for WoT the seed's direct follows' kind:3 whose contact
/// lists feed the ranking). Called at build and on every underlying-set change so
/// newly-resolved authors are actually acquired from relays — not merely
/// admitted. `open_interest` is idempotent + logged, so repeated calls as the set
/// grows add only the new authors and never double-open. The render/pull shape
/// (`live_shape`) is NOT opened here — its acquisition is carried by
/// `extra_acquisition`; `live_shape` only feeds the pull-pager store scan.
fn sync_member_interest(extra: &ExtraAcquisition, open_interest: &impl Fn(String, u32)) {
    for shape in extra() {
        open_interest(filter_json_for_shape(&shape), 0);
    }
}

/// Serialize an [`InterestShape`]'s acquisition fields to a NIP-01 filter JSON
/// the kernel's `OpenInterest` re-parses. Authors + kinds + `#t`/generic tags
/// (the fields the session scopes use). `nmp-core`'s canonical `filter_json_for`
/// is crate-private, so this mirrors the subset the perspective compiler emits.
fn filter_json_for_shape(shape: &InterestShape) -> String {
    let mut obj = serde_json::Map::new();
    if !shape.authors.is_empty() {
        let authors: Vec<&String> = shape.authors.iter().collect();
        obj.insert("authors".into(), serde_json::json!(authors));
    }
    if !shape.kinds.is_empty() {
        let kinds: Vec<&u32> = shape.kinds.iter().collect();
        obj.insert("kinds".into(), serde_json::json!(kinds));
    }
    for (key, vals) in &shape.tags {
        let vals: Vec<&String> = vals.iter().collect();
        obj.insert(format!("#{}", key.as_str()), serde_json::json!(vals));
    }
    serde_json::Value::Object(obj).to_string()
}

// Re-export so `mod.rs` can name the live-shape type alias used by `resolve.rs`.
/// The single render/pull acquisition shape (the member timeline), re-read live.
pub(super) type LiveShape = Arc<dyn Fn() -> Option<InterestShape> + Send + Sync>;
/// Extra acquisition shapes a scope must subscribe to BEYOND the render shape
/// (e.g. WoT's seed-follows kind:3, needed to build the second-degree ranking).
pub(super) type ExtraAcquisition = Arc<dyn Fn() -> Vec<InterestShape> + Send + Sync>;
