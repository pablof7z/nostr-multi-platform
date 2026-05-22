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
//
// The per-verb `nmp_app_react` / `nmp_app_follow` / `nmp_app_unfollow`
// symbols were deleted: the three social verbs are D0 app nouns and now
// route through the generic `nmp_app_dispatch_action` path under the
// `chirp.react` / `chirp.follow` / `chirp.unfollow` namespaces, which
// `nmp-app-chirp` registers at `nmp_app_chirp_register` time.
void nmp_app_signin_nsec(void *app, const char *secret);
void nmp_app_signin_bunker(void *app, const char *uri);
void nmp_app_create_new_account(void *app, const char *profile_json, const char *relays_json, bool mls);
void nmp_app_switch_active(void *app, const char *identity_id);
void nmp_app_remove_account(void *app, const char *identity_id);
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);
void nmp_app_open_timeline(void *app);

// ── Publish lifecycle (control plane only) ───────────────────────────────
//
// PR-F (one door per capability) DELETED the bespoke event-producing
// publish FFI:
//   * `nmp_app_publish_signed_event` [event_json]
//   * `nmp_app_publish_signed_event_to` [event_json, relays_json]
//   * `nmp_app_publish_unsigned_event` [unsigned_json]
//
// Every user / app-authored publish now goes through the single
// `nmp_app_dispatch_action` door under the `"nmp.publish"` namespace
// (see the action seam below). What stays here is the *control plane* —
// retry / cancel address an already-queued publish handle, never produce
// events, and have no `dispatch_action` equivalent.
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
// (PR-F: the `nmp_app_publish_unsigned_event` symbol was deleted — every
// user / app-authored publish now reaches the kernel through
// `nmp_app_dispatch_action` under the `"nmp.publish"` namespace instead.
// The action JSON carries the same `UnsignedEvent` shape the deleted
// symbol used to take, plus the registry-minted `correlation_id` in the
// dispatch return value so a host can correlate the eventual
// `last_error_toast` / `action_results` outcome.)
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
//
// Host action-namespace registration (ADR-0027) is Rust-only: a host calls
// `NmpApp::register_action::<M>()` with a typed `ActionModule` impl whose
// `M::start` validates and `M::execute` enqueues an `ActorCommand`. The
// previous C-ABI dual seam (`nmp_app_register_action_executor`,
// `nmp_app_register_action_module`) was deleted — `M::Action` and
// `ActorCommand` have no stable C representation, so any non-Rust host that
// wants a custom action namespace stages it through a Rust shim crate it
// controls. The Rust composition root in `apps/chirp/nmp-app-chirp` wires
// `chirp.react`/`chirp.follow`/`chirp.unfollow` and the NIP-29/NIP-17
// `ActionModule` impls this way.
//
// `nmp_app_register_snapshot_projection` is the OUTPUT-side counterpart to
// the action registration seam.  `KernelSnapshot` is a sealed social wire
// schema; a non-social app (marketplace, todo list, …) cannot extend it.
// This seam lets a host register a `projector` callback invoked on every
// snapshot tick whose returned JSON string is appended to the snapshot under
// a host-chosen `key` (e.g. `"market.listings"`, `"todo.items"`) inside a
// `projections` object — WITHOUT editing nmp-core's typed social fields.
// The `projector` returns a NUL-terminated JSON string, or NULL to contribute
// an empty object; an un-parseable return becomes JSON `null` (D6).  A null
// `app`/`key`/`projector` is a silent no-op (D6).  D8 — the projector runs on
// the actor thread inside the snapshot tick; it MUST be cheap and
// non-blocking (no I/O, no waits), or every subsequent snapshot stalls.
// A shell that predates this field never sees the `projections` key (it is
// omitted from the JSON when empty — backwards compatible).
//
// `nmp_app_register_action_result_observer` is the PUSH-side counterpart to
// the snapshot-projection (pull) output seam.  After `nmp_app_dispatch_action`
// accepts an action and its executor returns success, the registered
// `observer` callback is invoked with a NUL-terminated JSON C string
// `{"correlation_id":"<hex>","result_json":<value>}`.  This is an "action
// accepted and enqueued" signal — NOT a completion carrier: for `nmp.publish`
// the actor still has to verify+publish after this fires, and built-in
// executors are fire-and-forget so `result_json` is `null`.  An action that
// needs to return a value writes it into a snapshot projection (the pull
// model).  The JSON pointer is owned by nmp-core and valid only for the
// duration of the callback — copy any needed bytes before returning; do NOT
// free or retain it.  Unlike the action-executor/module seams this takes only
// the app handle (the observer lives behind a shared slot), so it may be
// registered before OR after `nmp_app_start`; a second registration replaces
// the first.  A null `app` or null `observer` is a silent no-op (D6).

typedef char *(*NmpCapabilityCallback)(void *context, const char *request_json);
void nmp_app_set_capability_callback(void *app, void *context, NmpCapabilityCallback callback);
char *nmp_app_dispatch_capability(void *app, const char *request_json);
char *nmp_app_dispatch_action(void *app, const char *namespace, const char *action_json);
typedef void (*NmpActionResultObserver)(const char *result_json);
void nmp_app_register_action_result_observer(void *app, NmpActionResultObserver observer);
// PR-G: ack a `correlation_id` in the `action_stages` snapshot mirror so the
// kernel drops its stage history. The host calls this AFTER it has reacted
// to the terminal stage (`Accepted` / `Failed`) — the entry persists across
// every snapshot tick until acked, so a dropped tick cannot strand the
// progress indicator. A null `app`, a null/empty `correlation_id`, or an
// unknown id is a silent no-op (D6). Dispatch is non-blocking: this only
// enqueues an actor command (D8).
void nmp_app_ack_action_stage(void *app, const char *correlation_id);
typedef const char *(*NmpSnapshotProjector)(void);
void nmp_app_register_snapshot_projection(void *app, const char *key, NmpSnapshotProjector projector);
void nmp_app_free_string(char *ptr);
// PR-F deleted `nmp_app_publish_unsigned_event` — use
// `nmp_app_dispatch_action(app, "nmp.publish", action_json)` instead.
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
// relay_url may be NULL. When NULL, Rust chooses the first configured
// write-capable relay from the app kernel, falling back to its default.
// callback_scheme may be NULL. When non-null, Rust appends
// `&callback=<percent-encoded callback_scheme>` to the URI so the signer
// app deep-links back to the host on approval. Hosts MUST NOT compose this
// suffix themselves — protocol-owned strings stay in Rust.
char *nmp_app_nostrconnect_uri(void *app, const char *relay_url, const char *callback_scheme);
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
void nmp_app_chirp_register_group_chat(void *app, const char *group_id_json);
void nmp_app_chirp_register_dm_inbox(void *app);
char *nmp_app_chirp_snapshot(void *handle);
void nmp_app_chirp_snapshot_free(char *ptr);
void nmp_app_chirp_unregister(void *handle);

// ── NIP-29 group-chat read projection ────────────────────────────────────
//
// Wires a single NIP-29 group's chat-message read model into the kernel.
// Pure consumption — the read side of a group-chat screen.
//
//   • `group_id_json` is a JSON object naming the target group:
//       {"host_relay_url":"wss://groups.example.com","local_id":"room"}
//   • Returns void — registers no handle and exports no companion
//     `unregister`. The group's chat messages surface on every kernel
//     snapshot tick under the `projections` key `"nip29.group_chat"`,
//     shaped `{ "messages": [ { id, pubkey, content, created_at, kind } ] }`
//     ordered newest-first.
//   • Single-screen scope: calling it twice overwrites the snapshot key
//     with the newer projection and leaves the older event observer
//     registered for the life of `app` (a small, bounded leak). A
//     multi-group host would need a handle-returning variant.
//   • Fire-and-forget (D6): a null `app`, null / invalid-UTF-8
//     `group_id_json`, or a JSON shape that does not deserialize to a
//     `GroupId` all degrade to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for the
//     duration of this call.
void nmp_app_chirp_register_group_chat(void *app, const char *group_id_json);

// ── NIP-29 group-discovery read projection ───────────────────────────────
//
// Wires a single host relay's NIP-29 group catalog (kinds 39000/39001/39002)
// into the kernel. Pure consumption — the read side of a group-discovery /
// join screen.
//
//   • `host_relay_url` is the relay to discover groups on (`wss://…`).
//     This projection is per-relay scoped; two relays with the same
//     `local_id` are two different groups (NIP-29 identity is the pair).
//   • Returns void — registers no handle and exports no companion
//     `unregister`. Discovered groups surface on every kernel snapshot
//     tick under the `projections` key `"nip29.discovered_groups"`,
//     shaped `{ "host_relay_url": "wss://…", "groups": [
//     { group_id, host_relay_url, name?, picture?, about?, member_count,
//       admin_count, public, open } ] }` ordered alphabetically by
//     `group_id`.
//   • The companion publish side is the `nmp.nip29.discover` action — its
//     executor pushes the kind:39000/39001/39002 LogicalInterest so the
//     kernel opens a REQ. This FFI symbol registers the *read* side; both
//     halves are needed for events to surface (registration alone is
//     inert).
//   • Single-screen scope: calling it twice overwrites the snapshot key
//     and leaks the older event observer for the life of `app` (a small,
//     bounded leak). A multi-relay discovery host would need a
//     handle-returning variant.
//   • Fire-and-forget (D6): a null `app`, null / invalid-UTF-8
//     `host_relay_url`, or an empty string all degrade to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for the
//     duration of this call.
void nmp_app_chirp_register_group_discovery(void *app, const char *host_relay_url);

// ── NIP-17 private direct-message inbox read projection ───────────────────
//
// Wires the NIP-17 DM inbox read model into the kernel — the receive side of
// private direct messages. Unlike the NIP-29 group chat there is no group id:
// the inbox is global (every conversation the local account participates in).
//
//   • Takes no viewer pubkey. Rust derives the active account from the local
//     NIP-17 key slot and owns the kind:1059 `#p` gift-wrap interest
//     lifecycle itself.
//   • Returns void — registers no handle, no companion `unregister`. The
//     decrypted conversations surface on every kernel snapshot tick under
//     the `projections` key `"nip17.dm_inbox"`, shaped
//     `{ "conversations": [ { peer_pubkey, messages: [...] } ] }`.
//   • `nmp_app_chirp_register` wires this eagerly. A second call replaces the
//     observer and projection keys idempotently.
//   • Fire-and-forget (D6): a null `app` degrades to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for the
//     duration of this call.
void nmp_app_chirp_register_dm_inbox(void *app);

// ── NIP-02 follow list read projection ───────────────────────────────────
//
// Wires the active account's NIP-02 kind:3 follow list into the kernel as a
// formatted snapshot. The kernel's standing account_profile_interest already
// fetches kind:3 — no separate interest push is needed.
//
//   • `active_pubkey_or_null` is the active account's hex pubkey. The
//     projection's active-pubkey slot is set so the snapshot returns the
//     correct account's follows. NULL is permitted (startup before sign-in);
//     the caller MUST re-invoke after sign-in / account switch.
//   • Returns void — registers no handle. The follow list surfaces under
//     the `projections` key `"chirp.follow_list"`, shaped
//     `{ "follows": [ { pubkey, npub, short_npub, avatar_initials,
//       avatar_color } ] }`.
//   • Fire-and-forget (D6): a null `app` degrades to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for this call.
void nmp_app_chirp_register_follow_list(void *app, const char *active_pubkey_or_null);

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
