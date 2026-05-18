#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdint.h>

// Pulse uses the same Path-A FFI shape as NmpStress — raw C bridge over the
// kernel actor. This header MUST stay in sync with the symbols exported from
// `crates/nmp-core/src/ffi.rs`. The M14 UniFFI codegen path will supersede
// this; until then it's hand-maintained.

void *nmp_app_new(void);
void nmp_app_free(void *app);
typedef void (*NmpUpdateCallback)(void *context, const char *json);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
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
void nmp_app_create_new_account(void *app);
void nmp_app_switch_active(void *app, const char *identity_id);
void nmp_app_remove_account(void *app, const char *identity_id);
void nmp_app_publish_note(void *app, const char *content, const char *reply_to_id_or_null);
void nmp_app_react(void *app, const char *target_event_id, const char *reaction);
void nmp_app_follow(void *app, const char *pubkey);
void nmp_app_unfollow(void *app, const char *pubkey);
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);
void nmp_app_open_timeline(void *app);

// NIP-47 Nostr Wallet Connect. All fire-and-forget (D6); outcomes arrive via
// the snapshot's `wallet_status` and `last_error_toast` fields.
void nmp_app_wallet_connect(void *app, const char *uri);
void nmp_app_wallet_disconnect(void *app);
void nmp_app_wallet_pay_invoice(void *app, const char *bolt11, const char *amount_msats_or_null);

// T118 / G3 — iOS scenePhase → kernel lifecycle bridge. ChirpApp observes
// `@Environment(\.scenePhase)` and reports `.active` / `.background` here;
// the kernel decides what each phase MEANS (D7) — when to fan
// `TriggerEvent::Foreground` through the NIP-77 reconciler, when to throttle
// retries, etc. `.inactive` is iOS's interstitial state during app-switch
// animations; the shell silently drops it (no FFI symbol).
//
// Fire-and-forget (D6): a null app, an already-stopped actor, or a closed
// channel are silent no-ops.
void nmp_app_lifecycle_foreground(void *app);
void nmp_app_lifecycle_background(void *app);

// Optional callback fired on a meaningful phase transition (the debounced
// `EnteredForeground` / `EnteredBackground` verdicts — rapid scenePhase
// oscillation collapses to one event). `phase` is `0` for foreground, `1`
// for background. Chirp does not currently register here (no client-side
// TriggerEngine; the in-kernel observer is what fans NIP-77 reconcile work
// internally). The symbol is exposed so a future shell-side consumer (or
// test harness) can plug in without changing the FFI shape.
typedef void (*NmpLifecycleCallback)(void *context, uint32_t phase);
void nmp_app_set_lifecycle_callback(void *app, void *context, NmpLifecycleCallback callback);

// ── T151 — capability socket, generic publish, URI routing ───────────────
//
// `nmp_app_set_capability_callback` registers the native handler that the
// kernel calls (synchronously) whenever it needs a platform capability (e.g.
// iOS Keychain via PD-019/T96).  The callback receives the
// `CapabilityRequest` JSON and MUST return a freshly heap-allocated
// `CapabilityEnvelope` JSON string; that string MUST then be released by the
// caller via `nmp_app_free_string`.  Passing NULL for `callback` unregisters
// the handler; a request received while unregistered yields an error
// envelope (D6), never a crash.
//
// `nmp_app_dispatch_capability` routes a `CapabilityRequest` JSON through
// the registered handler and returns the resulting `CapabilityEnvelope`
// JSON.  The returned pointer is heap-allocated by Rust and MUST be freed
// by the caller via `nmp_app_free_string`.  Never returns NULL for a
// non-NULL app/request_json (D6).
//
// `nmp_app_publish_unsigned_event` signs and publishes an `UnsignedEvent`
// JSON (fields: pubkey, kind, tags, content, created_at).  Fire-and-forget
// (D6); outcomes arrive via `last_error_toast` / `publish_queue`.
//
// `nmp_app_open_uri` opens whatever a `nostr:` URI (or bare NIP-19 entity)
// points at.  Fire-and-forget (D6): null/invalid input is a silent no-op.

typedef char *(*NmpCapabilityCallback)(void *context, const char *request_json);
void nmp_app_set_capability_callback(void *app, void *context, NmpCapabilityCallback callback);
char *nmp_app_dispatch_capability(void *app, const char *request_json);
void nmp_app_free_string(char *ptr);
void nmp_app_publish_unsigned_event(void *app, const char *unsigned_json);
void nmp_app_open_uri(void *app, const char *uri);

// ── NIP-46 signer broker (Stage 4) ───────────────────────────────────────
//
// `libnmp_signer_broker.a` is a separate Rust static library (doctrine D0
// forbids `nmp-core -> nmp-signers`, so the broker — which depends on both
// — must live in its own archive). The two symbols below are exported from
// that archive and MUST be reachable to the Chirp link step.
//
// Call `nmp_signer_broker_init(app)` exactly once, right after `nmp_app_new()`.
// It registers a `bunker://` handler that drives the NIP-46 connect /
// get_public_key dance on a worker thread; subsequent
// `nmp_app_signin_bunker(app, uri)` calls flow through the broker.
//
// `nmp_app_cancel_bunker_handshake(app)` aborts any in-flight handshake.
// Idempotent / safe when nothing is in flight.
void nmp_signer_broker_init(void *app);
void nmp_app_cancel_bunker_handshake(void *app);

// ── T146: nmp-app-chirp per-app FFI ──────────────────────────────────────
//
// `libnmp_app_chirp.a` is a separate Rust static library: doctrine D0
// forbids `nmp-core -> nmp-nip01 / nmp-threading`, so the Chirp-specific
// glue that composes the modular timeline projection lives in its own
// archive. The four symbols below are exported from that archive.
//
// Flow:
// 1. Call `nmp_app_chirp_register(app, viewer_pubkey)` once after
//    `nmp_app_new()` succeeds. Returns an opaque handle (or NULL on
//    failure). `viewer_pubkey` may be NULL (treated as "no viewer set").
// 2. On each render tick (or after an update arrives), call
//    `nmp_app_chirp_snapshot(handle)` to get a nul-terminated JSON string
//    `{ "blocks": [...], "cards": [...] }`. The shell owns the pointer
//    until it calls `nmp_app_chirp_snapshot_free(ptr)`.
// 3. On teardown, call `nmp_app_chirp_unregister(handle)` BEFORE
//    `nmp_app_free(app)`.
//
// Fire-and-forget: every entry point degrades silently on null pointers,
// poisoned mutexes, or serialization failure (D6).
void *nmp_app_chirp_register(void *app, const char *viewer_pubkey_or_null);
char *nmp_app_chirp_snapshot(void *handle);
void nmp_app_chirp_snapshot_free(char *ptr);
void nmp_app_chirp_unregister(void *handle);

#endif
