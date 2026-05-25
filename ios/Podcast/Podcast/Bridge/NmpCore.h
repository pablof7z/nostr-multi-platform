#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdbool.h>
#include <stdint.h>

// Podcast uses the raw C bridge over the NMP kernel actor. This header MUST stay
// in sync with the non-test-gated `#[no_mangle] extern "C" fn nmp_app_*`
// symbols exported from `crates/nmp-ffi/src/` and `apps/podcast/nmp-app-podcast/`.
// The M14 UniFFI codegen path will supersede this; until then it's hand-maintained
// and verified by the CI gate `ci/check-ffi-header-drift.sh`.

void *nmp_app_new(void);
void nmp_app_free(void *app);
typedef void (*NmpUpdateCallback)(void *context, const char *json);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
// Persistent storage directory for the LMDB EventStore backend. Must be
// called before `nmp_app_start`; a NULL or empty `path` clears it. Inert
// unless nmp-core is built with the `lmdb-backend` feature.
void nmp_app_set_storage_path(void *app, const char *path);
void nmp_app_start(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_configure(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);
void nmp_app_reset(void *app);
void nmp_app_open_author(void *app, const char *pubkey);
void nmp_app_open_thread(void *app, const char *event_id);
void nmp_app_open_firehose_tag(void *app, const char *tag);
void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id);
void nmp_app_release_profile(void *app, const char *pubkey, const char *consumer_id);
void nmp_app_close_author(void *app, const char *pubkey);
void nmp_app_close_thread(void *app, const char *event_id);

// T66a — identity / publish / multi-account / relay-edit. None return a
// value; outcomes (incl. validation failures) arrive via the snapshot's
// last_error_toast / accounts / publish_queue fields (D6).
void nmp_app_signin_nsec(void *app, const char *secret);
void nmp_app_signin_bunker(void *app, const char *uri);
void nmp_app_create_new_account(void *app, const char *profile_json, const char *relays_json, bool mls);
void nmp_app_switch_active(void *app, const char *identity_id);
void nmp_app_remove_account(void *app, const char *identity_id);
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);
void nmp_app_open_timeline(void *app);

// ── Publish lifecycle (control plane only) ───────────────────────────────
void nmp_app_retry_publish(void *app, const char *handle);
void nmp_app_cancel_publish(void *app, const char *handle);

// ── T146 — kernel event observer ─────────────────────────────────────────
typedef void (*NmpEventObserverCallback)(void *context, const char *event_json);
uint64_t nmp_app_register_event_observer(void *app, void *context, NmpEventObserverCallback callback);
void nmp_app_unregister_event_observer(void *app, uint64_t id);

// ── Raw signed-event tap ─────────────────────────────────────────────────
typedef void (*NmpRawEventObserverCallback)(void *context, const char *event_json);
uint64_t nmp_app_register_raw_event_observer(void *app, void *context, NmpRawEventObserverCallback callback, const char *kinds_json);
void nmp_app_unregister_raw_event_observer(void *app, uint64_t id);

// NIP-47 Nostr Wallet Connect. All fire-and-forget (D6); outcomes arrive via
// the snapshot's `wallet_status` and `last_error_toast` fields.
void nmp_app_wallet_connect(void *app, const char *uri);
void nmp_app_wallet_disconnect(void *app);
void nmp_app_wallet_pay_invoice(void *app, const char *bolt11, const char *amount_msats_or_null);

// T118 / G3 — iOS scenePhase → kernel lifecycle bridge.
// Fire-and-forget (D6): a null app, an already-stopped actor, or a closed
// channel are silent no-ops.
void nmp_app_lifecycle_foreground(void *app);
void nmp_app_lifecycle_background(void *app);

// Optional callback fired on a meaningful phase transition.
typedef void (*NmpLifecycleCallback)(void *context, uint32_t phase);
void nmp_app_set_lifecycle_callback(void *app, void *context, NmpLifecycleCallback callback);

// Actor-liveness probe (D7 pull-side sibling of the push-side panic frame).
// Returns `1` when the kernel's actor thread is still running, `0` when it
// has terminated. A null `app` is `0`.
uint8_t nmp_app_is_alive(void *app);

// ── T151 — capability socket, generic publish, URI routing ───────────────
typedef char *(*NmpCapabilityCallback)(void *context, const char *request_json);
void nmp_app_set_capability_callback(void *app, void *context, NmpCapabilityCallback callback);
char *nmp_app_dispatch_capability(void *app, const char *request_json);
char *nmp_app_dispatch_action(void *app, const char *namespace, const char *action_json);
typedef void (*NmpActionResultObserver)(const char *result_json);
void nmp_app_register_action_result_observer(void *app, NmpActionResultObserver observer);
// PR-G: ack a `correlation_id` in the `action_stages` snapshot mirror.
void nmp_app_ack_action_stage(void *app, const char *correlation_id);
typedef const char *(*NmpSnapshotProjector)(void);
void nmp_app_register_snapshot_projection(void *app, const char *key, NmpSnapshotProjector projector);

// ── V-51 phase 2 — routing-trace snapshot accessor ───────────────────────
char *nmp_app_recent_routing_decisions(void *app);

void nmp_app_free_string(char *ptr);
void nmp_app_open_uri(void *app, const char *uri);

// ── nmp-app-podcast per-app FFI ──────────────────────────────────────────
//
// `libnmp_app_podcast.a` is the Podcast Rust aggregate archive.
//
// Flow:
// 1. Call `nmp_app_podcast_register(app, viewer_pubkey_or_null)` once after
//    `nmp_app_new()` succeeds. Returns an opaque handle (or NULL on failure).
//    `viewer_pubkey` may be NULL.
// 2. On each render tick, call `nmp_app_podcast_snapshot(handle)` to get a
//    nul-terminated JSON string. The shell owns the pointer until it calls
//    `nmp_app_podcast_snapshot_free(ptr)`.
// 3. On teardown, call `nmp_app_podcast_unregister(handle)` BEFORE
//    `nmp_app_free(app)`.
//
// Fire-and-forget: every entry point degrades silently on null pointers,
// poisoned mutexes, or serialization failure (D6).
void *nmp_app_podcast_register(void *app, const char *viewer_pubkey_or_null);
char *nmp_app_podcast_snapshot(void *handle);
void nmp_app_podcast_snapshot_free(char *ptr);
void nmp_app_podcast_unregister(void *handle);

#endif
