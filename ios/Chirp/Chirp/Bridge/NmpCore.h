#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdbool.h>
#include <stdint.h>

// Chirp uses the raw C bridge over the NMP kernel actor. This header MUST stay
// in sync with the non-test-gated `#[no_mangle] extern "C" fn nmp_app_*`
// symbols exported from `crates/nmp-ffi/src/`. The M14 UniFFI codegen path
// will supersede this; until then it's hand-maintained and verified by the CI gate
// `ci/check-ffi-header-drift.sh`.

void *nmp_app_new(void);
void nmp_app_free(void *app);
typedef enum NmpConfigStatus {
    NmpConfigStatus_Ok             = 0,
    NmpConfigStatus_NullApp        = 1,
    NmpConfigStatus_AlreadyStarted = 2,
    NmpConfigStatus_Unavailable    = 3,
} NmpConfigStatus;
// Borrowed FlatBuffers `nmp.transport.UpdateFrame` bytes. The pointer is valid
// only for the callback duration; Swift copies before decoding.
typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
// Persistent storage directory for the LMDB EventStore backend. Must be
// called before `nmp_app_start`; a NULL or empty `path` clears it. Inert
// unless nmp-core is built with the `lmdb-backend` feature. Returns
// NmpConfigStatus_AlreadyStarted if called after nmp_app_start.
uint32_t nmp_app_set_storage_path(void *app, const char *path);
void nmp_app_start(void *app, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_configure(void *app, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);
void nmp_app_reset(void *app);
// V-68 / V-112 (ADR-0042): nmp_app_open_author, nmp_app_open_thread deleted.
// Use nmp_app_chirp_open_author_feed / nmp_app_chirp_open_thread_feed below.
//
// M2 (ADR-0042) — generic feed-subscription surface. Replaces the deleted
// open_firehose_tag verb. Hashtag feeds now use the Chirp-owned tag-feed seam:
// primary kind `[1]` is declared app-side, repost wrapper acquisition is
// derived by NIP-18, and the compiled `#t` filter is opened at Global scope.
// `filter_json` is a verbatim NIP-01 REQ filter. Declared feeds pass primary
// kinds only through their typed seam; protocol adapters derive repost wrappers.
// `consumer_id` refcounts owners across call sites passing the same filter;
// `scope` is 0 = ActiveAccount (re-route on switch), 1 = Global
// (account-agnostic, e.g. a hashtag feed).
void nmp_app_open_interest(void *app, const char *filter_json,
                           const char *consumer_id, uint32_t scope);
void nmp_app_close_interest(void *app, const char *filter_json,
                            const char *consumer_id, uint32_t scope);
// F-TTL — `force` (treated as `force != 0`) controls the lazy re-verification
// gate for the cached kind:0 profile. Pass `1` when the user explicitly opened
// this author's profile screen or pulled to refresh; pass `0` for background /
// `.onAppear` list-row claims. Replaces the removed `nmp_app_refresh_replaceable`
// symbol (force-refresh is now an argument; no new C-ABI symbol).
//
// `liveness` (treated as the discrete values 0 / 1) declares the consumer's
// desired subscription shape:
//   * 0 = CacheOk — serve from cache; a OneShot kind:0 fetch fills a miss;
//     NO live subscription. Use for feed avatars / inline list contexts.
//   * 1 = Live — register a Tailing kind:0 interest so reactive profile-edit
//     updates flow in. Use for the profile screen.
// Mixed claims on one pubkey resolve Tailing-wins in the kernel, deduped to a
// single REQ; the shell only passes its intent.
void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id,
                           int force, int liveness);
void nmp_app_release_profile(void *app, const char *pubkey, const char *consumer_id);
// Claim an embedded event by `nostr:` URI (T180 / ADR-0034). Refcounted per
// `consumer_id`; the kernel fetches the event over the OneshotApi (single-
// writer interest registration — D4) when not yet in the store, and surfaces
// it in `snapshot.projections.claimed_events` keyed by `primary_id` (event-id
// hex for `nevent`/`note`; `"kind:pubkey:d"` for `naddr`). FFI-clean (D6):
// null/invalid arguments are silent no-ops, never panics. D8: forwards to the
// actor; no polling, no sync wait.
// F-TTL — `force` (treated as `force != 0`) controls the lazy re-verification
// gate; it only has an effect for `naddr` (addressable / replaceable) URIs and
// is a silent no-op for immutable `nevent`/`note` URIs. Pass `1` when the user
// explicitly navigated to / opened this article/event or pulled to refresh;
// pass `0` for background claims.
void nmp_app_claim_event(void *app, const char *uri, const char *consumer_id, int force);
void nmp_app_release_event(void *app, const char *uri, const char *consumer_id);
// V-68 / V-112 (ADR-0042): nmp_app_close_author, nmp_app_close_thread deleted.
// Use nmp_app_chirp_close_author_feed / nmp_app_chirp_close_thread_feed below.
//
// Legacy compatibility shim for the active-follows feed declaration.
// Current Rust app/defaults code uses NmpApp::declare_active_follows_feed.
// `kinds_json` is a JSON array of primary unsigned 32-bit event kinds, e.g.
// `"[1]"`; protocol adapters derive wrapper acquisition (D0).
// An empty array `"[]"` is a legitimate clear (same effect as close).
// A malformed or non-array value surfaces a diagnostic toast (D6).
// D8: fire-and-forget; the actor processes the command asynchronously.
void nmp_app_open_contact_feed(void *app, const char *kinds_json);
// Legacy compatibility shim for clearing the active-follows feed declaration.
// Withdraws all follow-feed M2 interests from the lifecycle registry;
// `drain_lifecycle_tick` emits CLOSE frames for any live REQs on the next idle
// tick. D6: a null `app` is a silent no-op.
void nmp_app_close_contact_feed(void *app);

// Live "following count" read for the host profile header — the number of
// distinct hex-valid `p` tags in the active account's latest kind:3, read
// synchronously from the kernel's published store (read-your-writes, ADR-0057).
// Returns >= 0 when a kind:3 exists (0 for an explicit empty list), or -1 when
// there is no active account / no kind:3 yet / a lock is poisoned. Hosts render
// -1 as 0; the value is kept distinct so callers can tell "no list yet" apart.
int64_t nmp_app_active_following_count(void *app);

// T66a — identity / publish / multi-account / relay-edit. None return a
// value; outcomes (incl. validation failures) arrive via the snapshot's
// last_error_toast / accounts / publish_queue fields (D6).
//
// The per-verb `nmp_app_react` / `nmp_app_follow` / `nmp_app_unfollow`
// symbols were deleted: the three social verbs are D0 app nouns and now
// route through the generic `nmp_app_dispatch_action` path under the
// `nmp.nip25.react` / `nmp.follow` / `nmp.unfollow` namespaces, which
// `nmp-app-template` registers from `nmp_app_chirp_register`.
// make_active=1: sign in and set as the active account (normal sign-in).
// make_active=0: register a visible secondary signer without activating it.
// Hidden app-managed keys use nmp_app_register_agent_nsec.
void nmp_app_signin_nsec(void *app, const char *secret, uint8_t make_active);
// Register a persisted app-managed local signer. It signs only when named by
// pubkey and never appears in account projections or becomes active.
void nmp_app_register_agent_nsec(void *app, const char *secret);
void nmp_app_signin_bunker(void *app, const char *uri, uint8_t make_active);
// ADR-0048 Stage 2 — NIP-55 external signer (Android-only at runtime; the
// symbols exist behind nmp-ffi's `external-signer` feature, which the iOS
// build does not enable — declared here so the header stays the single
// canonical mirror of the Rust `nmp_app_*` surface).
// Begin a NIP-55 sign-in routed to `signer_package` (NULL = OS resolver).
void nmp_app_signin_nip55(void *app, const char *signer_package);
// Report a raw ExternalSignerResponse JSON back to the NIP-55 driver (D7).
void nmp_app_deliver_external_signer_response(void *app, const char *response_json);
// Sign an unsigned event with the named account's signer and park the result
// in the snapshot's signed_events projection.  Returns a correlation_id string
// that the caller uses to retrieve the signed event JSON.  Free with
// nmp_free_string.  Pass an empty string for account_pubkey_hex to use
// the active account.
char *nmp_app_sign_event_for_return(void *app, const char *account_pubkey_hex, const char *unsigned_json);
void nmp_app_create_new_account(void *app, const char *profile_json, const char *relays_json, bool mls, uint8_t make_active);
// Chirp-owned create-account wrapper (#1493). Same arguments as
// nmp_app_create_new_account, but the fresh account auto-follows Chirp's
// product seed set (nmp-chirp-config::chirp_default_follows) — the seed pubkeys
// stay in Rust, never in this shell. Chirp callers use THIS symbol; the generic
// one auto-follows nobody. Returns false on a NULL app or undecodable JSON.
bool nmp_app_chirp_create_new_account(void *app, const char *profile_json, const char *relays_json, bool mls, uint8_t make_active);
void nmp_app_switch_active(void *app, const char *identity_id);
void nmp_app_remove_account(void *app, const char *identity_id);
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);
// Chirp relay-bootstrap seeding. Policy lives in Rust (nmp-chirp-config), not
// in Swift (D7 / thin-shell): the relay default set has ONE source of truth.
// `nmp_app_chirp_seed_default_relays` adds the Chirp reference set; returns
// false only when `app` is NULL. `nmp_app_chirp_seed_relays_from_json` parses
// the NMP_TEST_RELAYS override (a [["url","role"],…] JSON array) and seeds each
// entry; returns false on a NULL app, malformed JSON, or an empty array — the
// caller falls back to the default seed on false. iOS analogue of the Android
// nmp-android-ffi relay-seeding glue.
bool nmp_app_chirp_seed_default_relays(void *app);
bool nmp_app_chirp_seed_relays_from_json(void *app, const char *json);
// V-68 Stage 2 (ADR-0042 amendment 2026-06-12): nmp_app_open_timeline REMOVED.
// Use the Chirp home-feed wrappers below instead.
void nmp_app_chirp_open_home_feed(void *app);
void nmp_app_chirp_close_home_feed(void *app);

// H4 — NMP-provided NIP-19 identity encoder. Turns a 64-char hex pubkey into a
// bech32 display identifier so app shells stop hand-rolling bech32.  Prefers
// `nprofile1…` (pubkey + relays) when the kernel already holds the pubkey's
// kind:10002 relay hints; otherwise returns a bare `npub1…`.  Never fetches —
// it is a synchronous read of cached kind:10002 state.  Returns a heap string
// the caller MUST free via nmp_free_string.  D6: a null/invalid input or
// any encode failure degrades to a copy of the raw input, never NULL.
char *nmp_app_encode_profile(void *app, const char *pubkey_hex);

// Stateless NIP-21 / bare NIP-19 decode helper. Accepts `nostr:` URIs and bare
// bech32 profile/event/address entities, returning bounded JSON:
//   {"ok":true,"target":"profile"|"event"|"address",...}
// or an error object such as {"ok":false,"error":"nsec-forbidden"}.
// The returned string is never NULL and MUST be freed via nmp_free_string.
char *nmp_nip21_decode_uri(const char *input);

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

// #1607: nmp_app_wallet_{connect,disconnect,pay_invoice} deleted.
// iOS callers use nmp_app_dispatch_action with the wallet action namespaces:
//   nmp_app_dispatch_action(app, "nmp.wallet.connect",    "{\"Connect\":{\"uri\":\"…\"}}")
//   nmp_app_dispatch_action(app, "nmp.wallet.disconnect", "\"Disconnect\"")
//   nmp_app_dispatch_action(app, "nmp.wallet.pay_invoice","{\"PayInvoice\":{\"bolt11\":\"…\",\"amount_msats\":null}}")
// The bolt11 double-tap guard now lives in WalletPayInvoiceModule (nmp-nip47).

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

// Actor-liveness probe (D7 pull-side sibling of the push-side panic frame
// the update channel emits on actor-thread death). Returns `1` when the
// kernel's actor thread is still running, `0` when it has terminated —
// panic, clean Shutdown, or "never started" all collapse to `0`. A null
// `app` is `0` (no kernel to be alive). Pairs with the `{"t":"panic",...}`
// update frame the channel emits on death: the panic frame is the push
// signal Swift sees on `nmp_app_set_update_callback`; this probe is the
// pull sibling, queryable on `applicationWillEnterForeground` so a host
// that was backgrounded across the panic frame's arrival (and never saw
// it) still learns the kernel is gone. The host treats every non-`1`
// response as "kernel dead — surface a fatal error". Observability only;
// the kernel is not influenced by this call.
uint8_t nmp_app_is_alive(void *app);

// ── T151 — capability socket, generic publish, URI routing ───────────────
//
// `nmp_app_set_capability_callback` registers the native handler that the
// kernel calls (synchronously) whenever it needs a platform capability (e.g.
// iOS Keychain via PD-019/T96).  The callback receives the
// `CapabilityRequest` JSON and MUST return a freshly heap-allocated
// `CapabilityEnvelope` JSON string; that string MUST then be released by the
// caller via `nmp_free_string`.  Passing NULL for `callback` unregisters
// the handler; a request received while unregistered yields an error
// envelope (D6), never a crash.
//
// `nmp_app_dispatch_capability` routes a `CapabilityRequest` JSON through
// the registered handler and returns the resulting `CapabilityEnvelope`
// JSON.  The returned pointer is heap-allocated by Rust and MUST be freed
// by the caller via `nmp_free_string`.  Never returns NULL for a
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
// on rejection, and MUST be freed via `nmp_free_string`.  D6: never NULL
// for a non-NULL app.  SCOPE — this validates the action, assigns a
// correlation id, AND executes it: after `ActionRegistry::start` validates
// the action and mints the id, the dispatch path drives `M::execute` which
// enqueues the appropriate `ActorCommand` (the actor thread re-verifies any
// signed envelope, then routes through the publish engine / protocol-command
// loop). A returned `{"correlation_id":"…"}` therefore means the action was
// *accepted and enqueued for execution*; per-relay outcomes still surface
// asynchronously through the snapshot path / `action_results`. The durable
// action ledger is a separate M6 follow-up.
//
// Host action-namespace registration (ADR-0027) is Rust-only: a host calls
// `NmpApp::register_action::<M>()` with a typed `ActionModule` impl whose
// `M::start` validates and `M::execute` enqueues an `ActorCommand`. The
// previous C-ABI dual seam (`nmp_app_register_action_executor`,
// `nmp_app_register_action_module`) was deleted — `M::Action` and
// `ActorCommand` have no stable C representation, so any non-Rust host that
// wants a custom action namespace stages it through a Rust shim crate it
// controls. The `nmp-app-template` composition root wires common Nostr actions
// (`nmp.publish`, NIP-02, NIP-17, NIP-57, NIP-65); `nmp-app-chirp` adds
// Chirp's NIP-29/Marmot app surfaces on top.
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
void nmp_app_load_older_feed(void *app, const char *feed_key);
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
// ADR-0053 — host-declared projection subscriptions. The OUTPUT-side sibling of
// the relay push_interest lattice: a host declares, ONCE at app init, the static
// set of Tier-2 kernel-owned built-in projection keys it consumes (the union of
// every projection key any of the app's screens reads, known at build time).
// `keys` is an array of `len` NUL-terminated UTF-8 C strings. The kernel then
// serializes a kernel-owned built-in into each snapshot only if its key is
// declared. An empty / zero-len declaration leaves the kernel emitting every
// built-in (no narrowing — the relay-filter semantic); a non-empty declaration
// narrows the built-ins to its members, skipping the producer work (notably the
// `relay_diagnostics` roll-up) for everything else. Additive (multiple calls
// union). Tier-1 host projections registered via
// Tier-1 host typed projections are NOT gated by this — registration
// already declares their consumption. Call before `nmp_app_start`. A null `app`,
// a null `keys`, or `len == 0` is a silent no-op; individual null entries are
// skipped (D6).
void nmp_app_declare_consumed_projections(void *app, const char *const *keys, uintptr_t len);

// ADR-0053 / Workstream-E4 — declare the explicit "I consume every Tier-2
// built-in projection" intent (the ONE non-footgun way to receive the full
// set). A full client calls this instead of leaving the consumption intent
// undeclared (which `nmp_app_start` treats as a loud forgotten-wiring bug, not a
// silent firehose). Idempotent; call before `nmp_app_start`. A null `app` is a
// silent no-op (D6).
void nmp_app_consume_all_builtin_projections(void *app);

// ADR-0055 Rung 3 — declare that this host's runtime owns the NMP cache-merge
// layer (D3-3) so the kernel may omit `Unchanged` projections from the frame.
// Single-writer, call before `nmp_app_start`. After this call the next snapshot
// is a full baseline (all live Tier-2 projections as Changed).
//
// Return codes (R3-S1b / issue #1390):
//   0  — success
//   1  — AlreadyStarted: called after nmp_app_start (a repeat declare BEFORE
//          start is idempotent and returns 0)
//   2  — RegistryUnavailable: internal snapshot registry is not yet ready
//  -1  — null `app` pointer (D6 silent guard)
int nmp_app_declare_incremental_apply(void *app);

// ── V-51 phase 2 — routing-trace snapshot accessor ───────────────────────
//
// Return a heap-owned NUL-terminated JSON snapshot of the kernel's recent
// routing decisions (the bounded ring-buffer projection
// `RoutingTraceProjection`). The caller MUST release the returned pointer
// via `nmp_free_string`.
//
// Payload shape (stable, schema-versioned — schema_version=1):
//
//   {
//     "schema_version": 1,
//     "capacity": 64,
//     "publishes":     [ { at_ms, kind, author, event_id_short,
//                          explicit_targets_set,
//                          urls: [ {url, lanes: [...]} ] } ],
//     "subscriptions": [ { at_ms, interest_id, kinds, authors_count,
//                          explicit_targets_set,
//                          urls: [...] } ]
//   }
//
// Each `lanes[]` entry is a `{ "kind": "Nip65", "direction": "Write" }`-
// style object whose discriminant matches the chirp-repl pretty-printer's
// grammar (`Nip65/Write`, `ClassRouted/<class>/<via>`, etc.) — the JSON
// and the human-readable form never drift.
//
// D6: never returns NULL for a non-NULL app — a kernel that hasn't yet
// constructed its projection, a poisoned slot, or a serialisation failure
// all collapse to a well-formed empty-rings payload
// (`{"schema_version":1,"capacity":0,"publishes":[],"subscriptions":[]}`).
// A NULL `app` is also handled — returns the same empty-rings payload.
char *nmp_app_recent_routing_decisions(void *app);

// ADR-0049 Part 2 — composition report (the explain-the-composition surface,
// NMP's analog of Spring Boot's ConditionEvaluationReport). Returns a heap-owned
// NUL-terminated JSON snapshot of the composition ledger: every host-init
// registration decision (action modules, ingest parsers, snapshot projections,
// the last-writer-wins wiring slots) and its disposition.
//
// Payload shape (stable, schema-versioned):
//   { "schema_version": 1, "count": N, "records": [
//       { "seam": "action_registry", "key": "nmp.nip02.follow",
//         "provider": "nmp_nip02::FollowModule", "disposition": "Installed" },
//       { "seam": "action_registry", "key": "nmp.publish",
//         "provider": "app::MyPublish", "disposition": "ReplacedPrevious",
//         "replaced": "nmp_core::publish::PublishModule" } ] }
//
// `disposition` is one of "Installed", "ReplacedPrevious", "YieldedToExisting",
// "DroppedLateWiring". `replaced` is present only for the replaced/yielded cases.
//
// The caller MUST release the returned pointer via nmp_free_string.
// D6: never returns NULL for a non-NULL app — a serialisation failure collapses
// to a well-formed empty document (`{"schema_version":1,"count":0,"records":[]}`).
// A NULL `app` is also handled — returns the same empty document.
char *nmp_app_composition_report(void *app);

// Release a Rust-heap C string returned by ANY NMP FFI function. Null-safe.
// This is the ONLY correct freer — the host's free(3) must NOT be used.
void nmp_free_string(char *ptr);
// PR-F deleted `nmp_app_publish_unsigned_event` — use
// `nmp_app_dispatch_action(app, "nmp.publish", action_json)` instead.
void nmp_app_open_uri(void *app, const char *uri);

// ── NIP-46 signer broker (Stage 4) ───────────────────────────────────────
//
// The reusable signer broker is app-neutral; the NmpApp/actor adapter lives
// in nmp-ffi and is linked through the aggregate `libnmp_app_chirp.a` archive.
// That keeps process-global Rust state, including the bunker hook, single-copy.
//
// Call `nmp_signer_broker_init(app)` exactly once, right after `nmp_app_new()`,
// before `nmp_app_start()`. Returns NmpConfigStatus_AlreadyStarted when called
// too late.
// It registers a `bunker://` handler that drives the NIP-46 connect /
// get_public_key dance on a worker thread; subsequent
// `nmp_app_signin_bunker(app, uri)` calls flow through the broker.
//
// `nmp_app_cancel_bunker_handshake(app)` aborts any in-flight handshake.
// Idempotent / safe when nothing is in flight.
uint32_t nmp_signer_broker_init(void *app);
void nmp_app_cancel_bunker_handshake(void *app);
// Generate a nostrconnect:// URI for the QR-code NIP-46 sign-in flow.
// The returned string must be freed via nmp_free_string.
// Returns NULL if the broker is not yet initialised or no write relay is
// configured (D3: relay selection is Rust-owned — the caller supplies only
// the optional platform callback scheme, never the relay URL).
// callback_scheme may be NULL. When non-null, Rust appends
// `&callback=<percent-encoded callback_scheme>` to the URI so the signer
// app deep-links back to the host on approval. Hosts MUST NOT compose this
// suffix themselves — protocol-owned strings stay in Rust.
char *nmp_app_nostrconnect_uri(void *app, const char *callback_scheme);

// ── T146: nmp-app-chirp per-app FFI ──────────────────────────────────────
//
// `libnmp_app_chirp.a` is the Chirp Rust aggregate archive: doctrine D0
// keeps protocol/app glue outside nmp-core while still letting the iOS
// shell link one Rust archive.
//
// Flow:
// 1. Call `nmp_app_chirp_register(app, viewer_pubkey, &handle)` once after
//    `nmp_app_new()` succeeds. Returns NmpRegisterStatus (0 = Ok). On Ok,
//    `handle` is written with a non-null opaque pointer.
//    `viewer_pubkey` may be NULL (treated as "no viewer set").
//    A non-null viewer_pubkey MUST be a 64-char case-insensitive hex string.
// 2. Read the standard `projections["nmp.feed.home"]` value from the normal
//    NMP update stream. It carries
//    `{ "blocks": [...], "cards": [...], "page": {...}, "metrics": {...} }`.
// 3. When the rendered tail becomes visible, call generic
//    `nmp_app_load_older_feed(app, "nmp.feed.home")`. Rust owns the cursor,
//    page size, and cap policy.
// 4. On teardown, call `nmp_app_chirp_unregister(handle)` BEFORE
//    `nmp_app_free(app)`.
//
// V-73 (D6 fix): a non-null viewer_pubkey that is not a valid 64-char hex
// pubkey returns NmpRegisterStatus_InvalidViewerPubkey (2) and leaves
// *handle_out as NULL. Callers must check the status before using the handle.
//
// D6 null handle_out guard: if handle_out itself is NULL, the function returns
// NmpRegisterStatus_NullApp (1) without writing through the pointer or leaking
// any allocation. Passing a null handle_out is a programmer-error contract
// violation (same as passing a null app).

// Status codes returned by `nmp_app_chirp_register`.
// Discriminants are stable — do not renumber.
typedef enum : uint32_t {
    NmpRegisterStatus_Ok                  = 0,
    NmpRegisterStatus_NullApp             = 1,
    NmpRegisterStatus_InvalidViewerPubkey = 2,
} NmpRegisterStatus;

uint32_t nmp_app_chirp_register(void *app,
                                const char *viewer_pubkey_or_null,
                                void **handle_out);
void nmp_app_chirp_register_group_chat(void *app, const char *group_id_json);
void nmp_app_chirp_register_dm_inbox(void *app);
// ADR-0053 — declare Chirp's built-in projection consumption. Chirp's screens
// (incl. the diagnostics view) read every kernel-owned built-in, so this routes
// to `consume_all_builtin_projections` (the codegen-derived built-in key set —
// the single source of truth, no hand-maintained list). Call once at app
// construction, before `nmp_app_start`. A null `app` is a silent no-op (D6).
void nmp_app_chirp_declare_consumed_projections(void *app);
// Build a Rust-authored Chirp action dispatch spec from typed user intent JSON.
// Returns {"namespace":"...","body_json":"..."} or {"error":"..."}; free with
// nmp_free_string.
char *nmp_app_chirp_action_spec(const char *intent_json);
void nmp_app_chirp_unregister(void *handle);

// ── M2 per-open flat author / thread feeds (ADR-0042 §5.1, V-112) ─────────
//
// Replace the deleted `author_view` / `thread_view` snapshot projections (and
// the deleted `nmp_app_open_author` / `nmp_app_open_thread` symbols). Each open
// registers a flat `FlatFeed` under a per-consumer snapshot key AND pushes the
// kernel interest that admits primary kind:1 notes plus NIP-18-derived repost
// wrappers into storage; each close tears both down. Chirp declares primary
// kind `[1]`; wrapper acquisition is derived below that app-facing declaration
// (D0).
//
//   • `nmp_app_chirp_open_author_feed(app, pubkey_hex)` — registers
//     `"nmp.feed.author.<pubkey_hex>"`, read by ProfileView. The feed emits the
//     SAME `RootFeedSnapshot` (`{ "cards": [{ card, attribution }] }`) shape the
//     home feed emits (attribution always empty), so the existing
//     `nmp.feed.home` reader decodes it with no new schema.
//   • `nmp_app_chirp_open_thread_feed(app, event_id_hex)` — registers
//     `"nmp.feed.thread.<event_id_hex>"`, read by ThreadScreen: the root by id
//     plus every admitted primary note or derived repost wrapper referencing it
//     via `#e`.
//   • The matching `close_*` symbols drop the feed controller, its snapshot
//     projection, and its ingest observer, and detach the kernel interest.
//     Idempotent — closing an unopened feed is a harmless no-op.
//   • Fire-and-forget (D6): a null `app` or null / invalid-UTF-8 id is a silent
//     no-op. `app` MUST outlive the feed; call the matching `close_*` (or rely
//     on the `nmp_app_free` actor join) before freeing it.
void nmp_app_chirp_open_author_feed(void *app, const char *pubkey_hex);
void nmp_app_chirp_close_author_feed(void *app, const char *pubkey_hex);
void nmp_app_chirp_open_thread_feed(void *app, const char *event_id_hex);
void nmp_app_chirp_close_thread_feed(void *app, const char *event_id_hex);

// ── NIP-29 group-chat read projection ────────────────────────────────────
//
// Wires a single NIP-29 group's chat-message read model into the kernel.
// Pure consumption — the read side of a group-chat screen.
//
//   • `group_id_json` is a JSON object naming the target group:
//       {"host_relay_url":"wss://groups.example.com","local_id":"room"}
//   • Returns void — registers no handle and exports no companion
//     `unregister`. The group's chat messages surface on every kernel
//     snapshot tick under the `projections` key `"nmp.nip29.group_chat"`,
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

// ── NIP-29 group-discovery open/close lifecycle ──────────────────────────
//
// Open a group-discovery session for a single host relay. The session owns
// a `DiscoveredGroupsProjection` for kinds 39000/39001/39002 — the read side
// of a discover/join screen. Tear it down with
// `nmp_app_chirp_close_group_discovery` when the screen is dismissed; the
// companion publish side is the `nmp.nip29.discover` dispatch action.
//
// `nmp_app_chirp_open_group_discovery`:
//   • `host_relay_url` is the relay to discover groups on (`wss://…`).
//   • Returns an opaque `void *` handle on success, NULL on failure (D6:
//     null `app`, null/invalid-UTF-8/empty `host_relay_url`, or internal
//     registration failure all return NULL).
//   • Discovered groups surface under the `projections` key
//     `"nmp.nip29.discovered_groups"` on every snapshot tick until the
//     session is closed.
//   • `app` MUST outlive the handle. Call
//     `nmp_app_chirp_close_group_discovery` before `nmp_app_free`.
//
// `nmp_app_chirp_close_group_discovery`:
//   • Unregisters the event observer and removes the
//     `"nmp.nip29.discovered_groups"` snapshot projection so no stale
//     group catalog is emitted after the screen is dismissed.
//   • Reclaims the handle; the pointer MUST NOT be used after this call.
//   • D6: a null `handle` is a silent no-op.
void *nmp_app_chirp_open_group_discovery(void *app, const char *host_relay_url);
void nmp_app_chirp_close_group_discovery(void *handle);

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
//     the `projections` key `"nmp.nip17.dm_inbox"`, shaped
//     `{ "conversations": [ { peer_pubkey, messages: [...] } ] }`.
//   • `nmp_app_chirp_register` inherits this from `nmp-app-template` eagerly.
//     This symbol remains a compatibility entry point for hosts that have not
//     moved to the template registration path.
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
//     the `projections` key `"nmp.follow_list"`, shaped
//     `{ "follows": [ { pubkey, npub, short_npub, avatar_initials,
//       avatar_color } ] }`.
//   • Fire-and-forget (D6): a null `app` degrades to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for this call.
void nmp_app_chirp_register_follow_list(void *app, const char *active_pubkey_or_null);

// ── Marmot (MLS encrypted groups) per-app FFI ────────────────────────────
//
// V-107 / ADR-0039: the former pull symbols `nmp_marmot_snapshot`,
// `nmp_marmot_group_messages`, and `nmp_marmot_string_free` were deleted.
// Swift now reads Marmot state reactively from the push projections
// `projections["nmp.marmot.snapshot"]` and `projections["nmp.marmot.messages"]`
// on every SnapshotFrame — no per-tick pull needed (D8: no polling).
//
// Remaining lifecycle symbols:
// 1. `nmp_marmot_register(app, secret_key_hex, db_dir, keyring_service_id)` once
//    the local identity secret is known. Registers the Marmot observer AND the
//    two push projections. `keyring_service_id` is the app-scoped keyring
//    namespace for the Marmot MLS DB encryption key (e.g. "com.example.marmot").
//    Returns an opaque handle, or NULL on any failure (D6).
// 2. `nmp_marmot_register_active(app, db_dir, keyring_service_id)` — same, but
//    reads the nsec from the actor's active local-key slot (no nsec exposed to
//    Swift).
// 3. Mutating ops: `nmp_app_dispatch_action("nmp.marmot", action_json)`.
//    Results arrive through the next push snapshot frame.
// 4. `nmp_marmot_unregister(handle)` BEFORE `nmp_app_free(app)`.
void *nmp_marmot_register(void *app, const char *secret_key_hex, const char *db_dir, const char *keyring_service_id);
/// Register using the actor-owned key — Swift never sees the nsec. Reads
/// the active local key from the slot the actor writes after identity
/// mutations. `keyring_service_id` is the app-scoped keyring namespace for
/// the Marmot MLS DB encryption key. Returns NULL if no local account is
/// active or service id is empty (D6).
void *nmp_marmot_register_active(void *app, const char *db_dir, const char *keyring_service_id);
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
void nmp_marmot_unregister(void *handle);

/// Trigger the kernel to fetch KeyPackage events (kind:30443/443) for the named
/// pubkeys from relays. `pubkeys_json` is a JSON array of pubkey strings (hex
/// or npub). Fire-and-forget; results arrive asynchronously through the Marmot
/// raw-event tap and appear in `cached_kp_pubkeys`.
void nmp_marmot_fetch_key_packages(void *handle, const char *pubkeys_json);

// ADR-0058 §3 (step 3b) — synchronous read-only pull-page surface.
//
// Owned heap buffer returned by `nmp_app_pull_page`. The page/gap/error result
// is binary (it carries raw event JSON and may contain NUL bytes), so it is not
// a C string. Release it EXACTLY once via `nmp_free_bytes` — the buffer belongs
// to the Rust allocator; mixing with the host `free(3)` is undefined behaviour.
typedef struct NmpOwnedBytes {
    uint8_t *ptr;
    uintptr_t len;
    uintptr_t cap;
} NmpOwnedBytes;

// Synchronously drain one page of the kernel ingest log for a registered pull
// cursor. `max_entries` is clamped to [1, 512]; cumulative raw bytes are bounded
// by min(max_total_raw_bytes, 4 MiB). A null app, unknown cursor, or unavailable
// store returns a serialized Error variant — never NULL, never a panic (D6).
// The result encoding (Page / Gap / Error) is documented in
// `crates/nmp-ffi/src/pull.rs`.
struct NmpOwnedBytes nmp_app_pull_page(const void *app,
                                       uint64_t cursor_id,
                                       uint32_t max_entries,
                                       uint32_t max_total_raw_bytes);

// Release a buffer returned by `nmp_app_pull_page`. Passing a NULL `ptr` is a
// no-op (D6).
void nmp_free_bytes(struct NmpOwnedBytes bytes);

#endif
