#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdbool.h>
#include <stdint.h>

// Pulse uses the same Path-A FFI shape as NmpStress — raw C bridge over the
// kernel actor. This header MUST stay in sync with the non-test-gated
// `#[no_mangle] extern "C" fn nmp_app_*` symbols exported from
// `crates/nmp-core/src/ffi/`. The M14 UniFFI codegen path will supersede
// this; until then it's hand-maintained and verified by the CI gate
// `ci/check-ffi-header-drift.sh`.

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
void nmp_app_publish_note(void *app, const char *content, const char *reply_to_id_or_null);
void nmp_app_react(void *app, const char *target_event_id, const char *reaction);
void nmp_app_follow(void *app, const char *pubkey);
void nmp_app_unfollow(void *app, const char *pubkey);
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);
void nmp_app_open_timeline(void *app);

// ── Verbatim signed-event publish ────────────────────────────────────────
//
// `nmp_app_publish_signed_event` routes a fully-formed, externally-signed
// flat NIP-01 event `{id,pubkey,created_at,kind,tags,content,sig}` to the
// author's NIP-65 outbox. The kernel's signer is never consulted; Schnorr
// signature + event-id hash are still verified on the actor side and
// forged/garbled events are dropped (never published). Fire-and-forget
// (D6): null app / null / non-UTF-8 `event_json` are silent no-ops;
// malformed JSON surfaces via `last_error_toast`.
void nmp_app_publish_signed_event(void *app, const char *event_json);

// Explicit-relay-target sibling of `nmp_app_publish_signed_event`: routes
// the verbatim event to exactly the relays in `relays_json` (a JSON array
// of relay-URL strings) instead of the NIP-65 outbox. A null / non-UTF-8 /
// empty-array `relays_json` falls back to Auto (outbox) behaviour, byte-
// identical to `nmp_app_publish_signed_event`. Same verify / no-re-sign /
// fire-and-forget (D6) semantics; malformed input surfaces as a toast.
void nmp_app_publish_signed_event_to(void *app, const char *event_json, const char *relays_json);
void nmp_app_retry_publish(void *app, const char *handle);
void nmp_app_cancel_publish(void *app, const char *handle);

// ── T146 — kernel event observer ─────────────────────────────────────────
//
// `nmp_app_register_event_observer` registers a callback that fires on the
// actor thread once per event accepted into the kernel `EventStore`
// (insertions/replacements only). The callback receives a nul-terminated
// JSON encoding of `KernelEvent` `{id,author,kind,created_at,tags,content}`;
// the pointer is borrowed for the callback's duration only — copy any bytes
// you need. Returns a non-zero `u64` id on success, `0` on failure (null
// app, null callback, poisoned mutex). The id is required to unregister.
//
// `nmp_app_unregister_event_observer` drops a registration by id.
// Idempotent (D6): unknown ids / null app are silent no-ops.
typedef void (*NmpEventObserverCallback)(void *context, const char *event_json);
uint64_t nmp_app_register_event_observer(void *app, void *context, NmpEventObserverCallback callback);
void nmp_app_unregister_event_observer(void *app, uint64_t id);

// ── Raw signed-event tap ─────────────────────────────────────────────────
//
// `nmp_app_register_raw_event_observer` registers a callback that fires
// once per accepted inbound event whose `kind` matches `kinds_json`, with a
// nul-terminated JSON encoding of the VERBATIM flat NIP-01 signed event
// `{id,pubkey,created_at,kind,tags,content,sig}` (the `sig` is preserved
// byte-for-byte — the whole point). `kinds_json` is a JSON array of u32
// kinds (e.g. `"[445,1059]"`); a null pointer, `"[]"`, or unparseable
// input means "deliver every kind". The payload pointer is borrowed for
// the callback's duration only. Returns a non-zero `u64` id on success,
// `0` on failure (null app, null callback, poisoned mutex).
//
// `nmp_app_unregister_raw_event_observer` drops a registration by id.
// Idempotent (D6): unknown ids / null app are silent no-ops.
typedef void (*NmpRawEventObserverCallback)(void *context, const char *event_json);
uint64_t nmp_app_register_raw_event_observer(void *app, void *context, NmpRawEventObserverCallback callback, const char *kinds_json);
void nmp_app_unregister_raw_event_observer(void *app, uint64_t id);

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
//
// `nmp_app_dispatch_action` is the single namespace-keyed entry point for the
// `ActionModule` family (M6).  The caller names the action namespace (e.g.
// `"nmp.publish"`) and passes the action as JSON; the returned heap-allocated
// JSON string is `{"correlation_id":"<32-hex>"}` on accept or `{"error":"…"}`
// on rejection, and MUST be freed via `nmp_app_free_string`.  D6: never NULL
// for a non-NULL app.  SCOPE — this currently validates the action and
// assigns a correlation id ONLY; it does NOT execute it.  A correlation id
// means the action was *accepted*, not *published*; execution wiring and the
// durable action ledger are an M6 follow-up.

typedef char *(*NmpCapabilityCallback)(void *context, const char *request_json);
void nmp_app_set_capability_callback(void *app, void *context, NmpCapabilityCallback callback);
char *nmp_app_dispatch_capability(void *app, const char *request_json);
char *nmp_app_dispatch_action(void *app, const char *namespace, const char *action_json);
void nmp_app_free_string(char *ptr);
void nmp_app_publish_unsigned_event(void *app, const char *unsigned_json);
void nmp_app_open_uri(void *app, const char *uri);

// ── NIP-46 signer broker (Stage 4) ───────────────────────────────────────
//
// The signer broker lives outside nmp-core (doctrine D0 forbids
// `nmp-core -> nmp-signers`) but Chirp links it through the aggregate
// `libnmp_app_chirp.a` archive. That keeps process-global Rust state,
// including the bunker hook, single-copy.
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
// Generate a nostrconnect:// URI for the QR-code NIP-46 sign-in flow.
// The returned string must be freed via nmp_broker_free_string.
// Returns NULL if the broker is not yet initialised.
// relay_url may be NULL (uses wss://r.f7z.io as default).
char *nmp_app_nostrconnect_uri(void *app, const char *relay_url);
void nmp_broker_free_string(char *ptr);

// ── T146: nmp-app-chirp per-app FFI ──────────────────────────────────────
//
// `libnmp_app_chirp.a` is the Chirp Rust aggregate archive: doctrine D0
// keeps protocol/app glue outside nmp-core while still letting the iOS
// shell link one Rust archive.
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

// ── Marmot (MLS encrypted groups) per-app FFI ────────────────────────────
//
// Six symbols exported from the same `libnmp_app_chirp.a` archive (the
// Chirp-specific composition point — D0 forbids `nmp-core -> nmp-marmot`).
// They mirror the lifetime / free / D6 conventions of the modular-timeline
// symbols above.
//
// Flow:
// 1. `nmp_app_chirp_marmot_register(app, secret_key_hex, db_dir)` once the
//    local identity secret is known. `secret_key_hex` is hex OR `nsec…`;
//    the encrypted MLS SQLite DB is created at
//    `<db_dir>/marmot-mls-state.sqlite`. Returns an opaque handle, or NULL
//    on any failure (D6).
// 2. `nmp_app_chirp_marmot_snapshot(handle)` each render tick → JSON
//    `{ groups, pending_welcomes, key_package }`.
// 3. `nmp_app_chirp_marmot_group_messages(handle, group_id_hex)` → newest
//    200 decrypted messages for one group (JSON array).
// 4. `nmp_app_chirp_marmot_dispatch(handle, action_json)` → one mutating
//    op; returns `{"ok":true,…}` / `{"ok":false,"error":"…"}`.
// 5. Free EVERY returned string via `nmp_app_chirp_marmot_string_free`.
// 6. `nmp_app_chirp_marmot_unregister(handle)` BEFORE `nmp_app_free(app)`.
//
// Fire-and-forget: every entry point degrades silently on null pointers,
// poisoned mutexes, or (de)serialization failure (D6).
void *nmp_app_chirp_marmot_register(void *app, const char *secret_key_hex, const char *db_dir);
/// Register using the actor-owned key — Swift never sees the nsec. Reads
/// the active local key from the slot the actor writes after identity
/// mutations. Returns NULL if no local account is active (D6).
void *nmp_app_chirp_marmot_register_active(void *app, const char *db_dir);
/// Rust-owned Chirp identity bootstrap: restore a persisted local secret
/// through the native keyring capability, sign in through the kernel actor,
/// and register Marmot. `test_nsec` may be NULL; when non-NULL it overrides
/// keyring recall for UI tests. Returns the Marmot handle or NULL.
void *nmp_app_chirp_identity_restore(void *app, const char *db_dir, const char *test_nsec);
/// Rust-owned nsec sign-in: persist through keyring capability, sign in, and
/// register Marmot. Returns the Marmot handle or NULL.
void *nmp_app_chirp_identity_sign_in_nsec(void *app, const char *secret, const char *db_dir);
/// Rust-owned removal policy: forget Chirp's persisted local secret and
/// remove the identity through the kernel actor.
void nmp_app_chirp_identity_remove_account(void *app, const char *identity_id);
char *nmp_app_chirp_marmot_snapshot(void *handle);
char *nmp_app_chirp_marmot_group_messages(void *handle, const char *group_id_hex);
char *nmp_app_chirp_marmot_dispatch(void *handle, const char *action_json);
void nmp_app_chirp_marmot_string_free(char *ptr);
void nmp_app_chirp_marmot_unregister(void *handle);

/// Trigger the kernel to fetch KeyPackage events (kind:30443/443) for the named
/// pubkeys from relays. `pubkeys_json` is a JSON array of pubkey strings (hex
/// or npub). Fire-and-forget; results arrive asynchronously through the Marmot
/// raw-event tap and appear in `cached_kp_pubkeys`.
void nmp_app_chirp_marmot_fetch_key_packages(void *handle, const char *pubkeys_json);

#endif
