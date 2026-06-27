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
// M2 (ADR-0042) — low-level static interest surface. `filter_json` is a
// verbatim NIP-01 REQ filter after feed/source policy has already been compiled
// elsewhere. Declared app feeds use nmp_app_open_feed; dynamic sources such as
// active follows are Rust-owned ReducedSource sessions, not native-computed
// author lists.
// `consumer_id` refcounts owners across call sites passing the same filter;
// `scope` is 0 = ActiveAccount (re-route on switch), 1 = Global
// (account-agnostic, e.g. a hashtag feed).
void nmp_app_open_interest(void *app, const char *filter_json,
                           const char *consumer_id, uint32_t scope);
void nmp_app_close_interest(void *app, const char *filter_json,
                            const char *consumer_id, uint32_t scope);
// Higher-order NIP-50 search (nmp-nip50). `request_json` is the serde JSON of a
// SearchRequest, e.g. {"query":"nostr","scope":"Users","targets":"UserPreferred",
// "max_hits":50} (scope also accepts "LongForm" / {"Kinds":[...]}; targets also
// "AppDefault" / {"Explicit":["wss://..."]}). `session_id` keys the session for
// close + the typed N50S snapshot projection under `nmp.nip50.search.<session_id>`.
void nmp_app_search_open(void *app, const char *request_json,
                         const char *session_id);
// Close a search session opened via `nmp_app_search_open`. Idempotent.
void nmp_app_search_close(void *app, const char *session_id);
// Copy the current typed N50S search-results buffer for `session_id` into
// `out_buf` (capacity `cap` bytes). Returns the buffer's byte length (the
// required size), or 0 when the session is unknown / has no data. If the return
// value exceeds `cap`, nothing was copied — retry with a larger buffer (standard
// two-call size-probe). The bytes match the snapshot frame's N50S sidecar.
int nmp_app_search_snapshot(void *app, const char *session_id,
                            uint8_t *out_buf, uintptr_t cap);
// ADR-0063 Lane D — typed reference-resolution entry points. Hosts call these
// adapters instead of spelling raw namespace/shape/liveness integers.
// D6: null/invalid args are silent no-ops, never panics.
// D8: fire-and-forget; the actor processes commands asynchronously.
void nmp_app_resolve_ref(void *app, int namespace, const char *key,
                         const char *consumer_id, int shape, int liveness);
void nmp_app_resolve_ref_with_metadata(void *app, int namespace,
                                       const char *key,
                                       const char *consumer_id, int shape,
                                       int liveness,
                                       const char *metadata_json);
void nmp_app_release_ref(void *app, int namespace, const char *key,
                         const char *consumer_id);
void nmp_app_resolve_profile_ref(void *app, const char *key,
                                 const char *consumer_id);
void nmp_app_resolve_profile_card_live(void *app, const char *key,
                                       const char *consumer_id);
void nmp_app_release_profile_ref(void *app, const char *key,
                                 const char *consumer_id);
void nmp_app_resolve_event_embed(void *app, const char *key,
                                 const char *consumer_id);
void nmp_app_resolve_event_embed_live(void *app, const char *key,
                                      const char *consumer_id);
void nmp_app_resolve_event_embed_with_metadata(void *app, const char *key,
                                               const char *consumer_id,
                                               const char *metadata_json);
void nmp_app_resolve_event_embed_live_with_metadata(void *app, const char *key,
                                                    const char *consumer_id,
                                                    const char *metadata_json);
void nmp_app_release_event_ref(void *app, const char *key,
                               const char *consumer_id);
// #1740 step 7 — the ONE public app-facing feed doorway. (The raw
// nmp_app_open_contact_feed / nmp_app_close_contact_feed active-follows shims
// are RETIRED in step 8 — pass FeedParams whose acquisition scope is
// FeedScope::ActiveUserFollows to nmp_app_open_feed for the home feed.)
//
// nmp_app_open_feed: open ONE feed session from a JSON-encoded FeedParams (the
// app's PRIMARY content kinds + a typed FeedScope acquisition + admission /
// ranking / window + projection key). Wrapper/delete acquisition is derived
// below the boundary. Returns a heap-owned C string the caller MUST free via
// nmp_free_string:
//   success → {"projection_key":"<key>","session_id":<u64>}  (the close token)
//   failure → {"error":"<token>"}  (null_app | bad_params | invalid_primary_kinds
//             | scope_unsupported | registry_unavailable)
// D6 — never NULL for a non-null app; never panics across the ABI.
char *nmp_app_open_feed(void *app, const char *params_json);
// nmp_app_close_feed: tear down a feed session opened by nmp_app_open_feed,
// addressed by its HANDLE (the verbatim {"projection_key":…,"session_id":…}
// envelope the open returned — never a re-derived filter). Idempotent; a null
// app / malformed handle / already-closed session is a harmless no-op (D6).
void nmp_app_close_feed(void *app, const char *handle_json);

// #1726: nmp_app_active_following_count DELETED (sync sentinel read).
// Follow count is in the nmp.follow_list typed projection: read follows.len()
// from projections["nmp.follow_list"].follows on the next snapshot frame.

// T66a — identity / publish / multi-account / relay-edit. None return a
// value; outcomes (incl. validation failures) arrive via the snapshot's
// last_error_toast / accounts / publish_queue fields (D6).
//
// The per-verb `nmp_app_react` / `nmp_app_follow` / `nmp_app_unfollow`
// symbols were deleted: social write verbs are D0 app nouns and are now
// dispatched via `nmp_app_dispatch_action_bytes` using bytes produced by
// generated action builders (Swift `dispatchBytes` / Kotlin `ActionBuilders`)
// under the `nmp.nip25.react` / `nmp.follow` / `nmp.unfollow` namespaces.
// `nmp-app-template` registers these actions via `nmp_app_chirp_register`.
// Do NOT reintroduce the old intent/action-spec path; use generated builders.
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
void nmp_external_signer_init(void *app);
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
void nmp_app_chirp_open_tag_feed(void *app, const char *tag);
void nmp_app_chirp_close_tag_feed(void *app, const char *tag);

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

// Stateless content tokenizer. Returns {"ok":true,"tree":ContentTreeWire}
// using the same wire arena as the registry renderers, or
// {"ok":false,"error":"..."}. `mode`: 0 plain, 1 markdown, 2 auto by `kind`.
// `tags_json` may be NULL or a JSON [[string]] event-tag array for emoji tags.
// The returned string is never NULL and MUST be freed via nmp_free_string.
char *nmp_content_tokenize_text(const char *content,
                                const char *tags_json,
                                int mode,
                                uint32_t kind);

// ── Input-intent resolver (#1804) ────────────────────────────────────────────
// One untyped input string (one-box / paste / search field) is classified into
// exactly one of: a NIP-19/21 direct ref, a relay URL, a NIP-05-shaped
// identifier, a registered recognizer's target, or a free-text NIP-50 search —
// or refused (secret-like / unparseable / unregistered-scope / disallowed-scope).
//
// `nmp_app_intent_classify` is STATELESS / sync / side-effect-free: it reads the
// app's registered recognizer snapshot and runs the pure classifier. No kernel
// mutation, no network. `request_json` is an InputIntentRequest:
//   {"input":"jb55@jb55.com",
//    "scopes":[{"namespace":"nip50","name":"profiles"}],
//    "text_targets":"UserPreferred"}
// Returns {"ok":true,"classification":{...}} (a Candidates list or a single
// Rejection), or {"ok":false,"error":"…"} on a bad argument. A SecretLike
// rejection NEVER echoes the input. The returned string is never NULL and MUST
// be freed via nmp_free_string.
char *nmp_app_intent_classify(void *app, const char *request_json);

// `nmp_app_intent_dispatch` classifies `request_json`, then routes the TOP
// candidate to its seam: DirectRef → open-uri; TextQuery → a search session
// keyed by `session_id`; Nip05 → the NIP-05 reverse-lookup worker (HTTP →
// profile resolve-ref). RelayUrl / Registered have no in-FFI seam and are
// returned to the host to route. Returns {"ok":true,"dispatched":<candidate>}
// or {"ok":true,"rejection":<rejection>}. The returned string is never NULL and
// MUST be freed via nmp_free_string.
char *nmp_app_intent_dispatch(void *app, const char *request_json,
                              const char *session_id);

// ── Publish lifecycle (control plane only) ───────────────────────────────
//
// PR-F (one door per capability) DELETED the bespoke event-producing
// publish FFI:
//   * `nmp_app_publish_signed_event` [event_json]
//   * `nmp_app_publish_signed_event_to` [event_json, relays_json]
//   * `nmp_app_publish_unsigned_event` [unsigned_json]
//
// Every user / app-authored publish now goes through the typed byte doorway
// (`nmp_app_chirp_dispatch_action_bytes` for the `"nmp.publish"` namespace;
// see the action seam below). What stays here is the *control plane* —
// retry addresses an already-queued publish handle; cancel (S7/#1754,
// replacing the deleted `nmp_app_cancel_publish` handle symbol) addresses the
// operation `correlation_id` — the kernel reverse-resolves the publish handle
// from a durable handle↔correlation index and records the user-initiated
// `cancelled` terminal under the ORIGINAL correlation_id (PD-036). Neither
// produces events nor has a `dispatch_action` equivalent.
void nmp_app_retry_publish(void *app, const char *handle);
void nmp_app_cancel_action(void *app, const char *correlation_id);

// #1607: nmp_app_wallet_{connect,disconnect,pay_invoice} deleted.
// iOS app code uses typed KernelHandle wallet methods. Those methods keep the
// ADR-0064 namespace/body compatibility route private to the bridge layer while
// the bolt11 double-tap guard lives in WalletPayInvoiceModule (nmp-nip47).

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
// user / app-authored publish now reaches the kernel through typed bridge
// methods or generated bytes over the ADR-0064 byte doorway. The JSON
// `nmp_app_dispatch_action` doorway was also deleted at ADR-0064 / Cut-B
// (#1756).)
//
// `nmp_app_open_uri` opens whatever a `nostr:` URI (or bare NIP-19 entity)
// points at.  Fire-and-forget (D6): null/invalid input is a silent no-op.
//
// `nmp_app_dispatch_action_bytes` is the raw byte entry point for the
// `ActionModule` family (M6) after ADR-0064 / Cut-B (#1756). The caller passes
// a typed `DispatchEnvelope` built by generated code or bridge-private helper
// methods. Returns the heap-allocated `{"correlation_id":"<id>"}` or
// `{"error":"…"}` envelope; MUST be freed via `nmp_free_string`. D6: never
// NULL for a non-NULL app.
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
// the snapshot-projection (pull) output seam.  After `nmp_app_dispatch_action_bytes`
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
// ADR-0064 / Cut-B (#1756) — the typed-FlatBuffers BYTE doorway (sole
// remaining dispatch entry point; the JSON `nmp_app_dispatch_action` doorway
// was deleted). The caller passes the bytes of an open `DispatchEnvelope`
// (correlation_id + action_namespace + schema_version + opaque per-crate
// payload). Social writes build this envelope via the generated
// `GeneratedActionBuilders` byte builders; the Chirp helper
// `nmp_app_chirp_dispatch_action_bytes` (below) builds it in Rust from a
// `(namespace, body_json)` pair for the direct-dispatch sites. Returns the same
// heap-allocated `{"correlation_id":"<id>"}` (accepted+enqueued) or
// `{"error":"…"}` JSON shape, which MUST be freed via `nmp_free_string`.
// Fail-closed (D6): a null `app`, a null `ptr`, an oversize / malformed /
// wrong-identifier / wrong-schema-version / namespace-less envelope, or an
// unknown namespace all return `{"error":…}` — never NULL for a non-NULL app.
char *nmp_app_dispatch_action_bytes(void *app, const uint8_t *ptr, uintptr_t len);
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
// the relay interest-install lattice: a host declares, ONCE at app init, the static
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

// ── #1726 — unified diagnostic pull accessor ──────────────────────────────
//
// `nmp_app_debug_info(app, domain)` replaces the deleted pair
// `nmp_app_recent_routing_decisions` / `nmp_app_composition_report`.
// The caller MUST release the returned pointer via `nmp_free_string`.
//
// `domain`:
//   0 — routing trace (schema_version, capacity, publishes, subscriptions)
//   1 — composition report (schema_version, count, records)
//   2 — merged: {"routing":{...},"composition":{...}}
//   other — empty JSON object `{}` (D6 silent no-op)
//
// D6: never returns NULL — a null app, unavailable projection, or
// serialization failure all collapse to a well-formed empty payload.
char *nmp_app_debug_info(void *app, int domain);

// Release a Rust-heap C string returned by ANY NMP FFI function. Null-safe.
// This is the ONLY correct freer — the host's free(3) must NOT be used.
void nmp_free_string(char *ptr);
// PR-F deleted `nmp_app_publish_unsigned_event`; ADR-0064 / Cut-B (#1756)
// deleted `nmp_app_dispatch_action` — use
// `nmp_app_chirp_dispatch_action_bytes(app, "nmp.publish", action_json)` instead.
void nmp_app_open_uri(void *app, const char *uri);

// ── NIP-46 actor-lane runtime ─────────────────────────────────────────────
//
// NIP-46 rides the actor's shared relay lane (no separate worker thread / socket).
// The interceptor + per-app bunker hook live in nmp-ffi and are linked through
// the aggregate `libnmp_app_chirp.a` archive. State is per-app (ADR-0052 §D3 —
// no process-global), stored on the NmpApp handle.
//
// Call `nmp_signer_broker_init(app)` exactly once, right after `nmp_app_new()`,
// before `nmp_app_start()`. Returns NmpConfigStatus_Ok (0) — including on a
// second, idempotent no-op call — or NmpConfigStatus_AlreadyStarted when called
// too late.
// It registers a `bunker://` handler that drives the NIP-46 connect /
// get_public_key handshake over the actor relay lane; subsequent
// `nmp_app_signin_bunker(app, uri)` calls flow through it.
//
// `nmp_app_cancel_bunker_handshake(app)` aborts any in-flight handshake
// (clears the runtime + unregisters the persistent subscription).
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
// ADR-0053 — declare Chirp's built-in projection consumption. Chirp's screens
// (incl. the diagnostics view) read every kernel-owned built-in, so this routes
// to `consume_all_builtin_projections` (the codegen-derived built-in key set —
// the single source of truth, no hand-maintained list). Call once at app
// construction, before `nmp_app_start`. A null `app` is a silent no-op (D6).
void nmp_app_chirp_declare_consumed_projections(void *app);
// ADR-0064 / S4 (#1782), M14-1 / #2145 — bridge-private doorway for the
// direct-dispatch action families (NIP-29 group ops) where the host already
// holds a `(namespace, body_json)` pair. Social writes (notes, reactions,
// reposts, follows, zaps, DMs) no longer route here: they ride the generated
// `GeneratedActionBuilders` byte builders straight to
// `nmp_app_dispatch_action_bytes`. Rust converts the verbatim body to the
// namespace's typed payload bytes and dispatches through the byte doorway; only
// typed bytes cross to the kernel. Returns `{"correlation_id":"<id>"}`
// (accepted) or `{"error":"…"}` JSON, freed via `nmp_free_string`. Fail-closed
// (D6) on null/unknown namespace.
char *nmp_app_chirp_dispatch_action_bytes(void *app, const char *namespace, const char *body_json);
void nmp_app_chirp_unregister(void *handle);

// ── NIP-29 group-chat read projection ────────────────────────────────────
//
// Wires a single NIP-29 group's chat-message read model into the kernel.
// Pure consumption — the read side of a group-chat screen. HYDRATING (#2088):
// a screen opened AFTER the group's events were already cached catches up on
// the cached tail, then tails live.
//
//   • `request_json` wraps the target group under `"group"` and names the
//     event `"kinds"` the view consumes:
//       {"group":{"host_relay_url":"wss://groups.example.com","local_id":"room"},
//        "kinds":[9,11]}
//     `kinds` is REQUIRED: an empty array means "all kinds", a missing array
//     is rejected. Chirp's group chat passes `[9, 11]` (chat + thread root).
//   • Returns void. The group's chat messages surface on every kernel
//     snapshot tick under the `projections` key `"nmp.nip29.group_events"`,
//     shaped `{ "messages": [ { id, pubkey, content, created_at, kind } ] }`
//     ordered newest-first.
//   • Singleton scope: calling it again replaces the prior view (the prior
//     hydrating session is closed first — no leak). Because the view now holds
//     a relay interest, tear it down with `nmp_app_chirp_unregister_group_events`
//     when the screen is dismissed.
//   • Fire-and-forget (D6): a null `app`, null / invalid-UTF-8
//     `request_json`, or a JSON shape that does not deserialize to a valid
//     request (missing `group` or `kinds`) all degrade to a silent no-op.
//   • `app` MUST outlive the registration; it is borrowed only for the
//     duration of this call.
void nmp_app_chirp_register_group_events(void *app, const char *request_json);

// Tear down the group-chat read view opened by
// `nmp_app_chirp_register_group_events`: detaches the relay interest, revokes the
// observer, and removes the `"nmp.nip29.group_events"` snapshot projection.
// Idempotent; a null `app` is a silent no-op (D6).
void nmp_app_chirp_unregister_group_events(void *app);

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
//   • Unregisters the observed projection and removes the
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
// Remaining native-facing lifecycle symbols:
// 1. `nmp_marmot_register_active(app, db_dir, keyring_service_id)` — reads the
//    nsec from the actor's active local-key slot (no nsec exposed to Swift).
//    Registers the Marmot observer AND the two push projections.
//    `keyring_service_id` is the app-scoped keyring namespace for the Marmot
//    MLS DB encryption key (e.g. "com.example.marmot"). Returns an opaque
//    handle, or NULL on any failure (D6).
// 2. Mutating ops: typed Marmot bridge methods route the action envelope through
//    the bridge-private ADR-0064 compatibility doorway. Results arrive through
//    the next push snapshot frame.
// 3. `nmp_marmot_unregister(handle)` BEFORE `nmp_app_free(app)`.
//
// #1727: the secret-bearing `nmp_marmot_register(app, secret_key_hex, …)` C
// symbol was removed from the native ABI — no native code ever called it, and
// no `nmp_marmot_*` symbol may carry secret key material. The nsec sign-in path
// keeps its synchronous registration entirely Rust-side (see
// `nmp_app_chirp_identity_sign_in_nsec` below, which never re-exposes the
// secret to native).
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

// #1727: `nmp_marmot_fetch_key_packages(handle, pubkeys_json)` was removed —
// it had no native caller. The kernel already fetches KeyPackage events
// (kind:30443) internally whenever an invite/group action needs a peer's
// key package (the same lookup interest is pushed by the invite/group flow).

// ADR-0058 §3 (step 3b) — synchronous read-only pull-page surface.
//
// Owned heap buffer returned by `nmp_mirror_pull_page`. The page/gap/error result
// is binary (it carries raw event JSON and may contain NUL bytes), so it is not
// a C string. Release it EXACTLY once via `nmp_mirror_free_bytes` — the buffer
// belongs to the Rust allocator; mixing with the host `free(3)` is undefined
// behaviour.
//
// Renamed NmpOwnedBytes → NmpMirrorBytes (#1726) to gate raw history behind the
// nmp_mirror_* family and make the ownership discipline explicit.
typedef struct NmpMirrorBytes {
    uint8_t *ptr;
    uintptr_t len;
    uintptr_t cap;
} NmpMirrorBytes;

// Synchronously drain one page of the kernel ingest log for a registered pull
// cursor. `max_entries` is clamped to [1, 512]; cumulative raw bytes are bounded
// by min(max_total_raw_bytes, 4 MiB). A null app, unknown cursor, or unavailable
// store returns a serialized Error variant — never NULL, never a panic (D6).
// The result encoding (Page / Gap / Error) is documented in
// `crates/nmp-ffi/src/pull.rs`.
// Renamed nmp_app_pull_page → nmp_mirror_pull_page (#1726).
struct NmpMirrorBytes nmp_mirror_pull_page(const void *app,
                                           uint64_t cursor_id,
                                           uint32_t max_entries,
                                           uint32_t max_total_raw_bytes);

// Release a buffer returned by `nmp_mirror_pull_page`. Passing a NULL `ptr` is a
// no-op (D6). Renamed nmp_free_bytes → nmp_mirror_free_bytes (#1726).
void nmp_mirror_free_bytes(struct NmpMirrorBytes bytes);

#endif
