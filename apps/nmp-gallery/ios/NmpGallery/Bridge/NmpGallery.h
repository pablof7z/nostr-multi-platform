#ifndef NMP_GALLERY_H
#define NMP_GALLERY_H

#include <stdbool.h>
#include <stdint.h>

// NmpGallery is a developer-facing component gallery for the NMP registry.
// It links one aggregate Rust archive — `libnmp_app_gallery.a` — that bundles
// the NMP kernel symbols plus a gallery-tailored projection. The subset of the
// NMP C-ABI declared below is exactly what the gallery shell needs; matching
// declarations live in `ios/Chirp/Chirp/Bridge/NmpCore.h` (kept hand-in-sync by
// `ci/check-ffi-header-drift.sh`).

// ── Kernel lifecycle ─────────────────────────────────────────────────────

void *nmp_app_new(void);
void nmp_app_free(void *app);

// Borrowed FlatBuffers `nmp.transport.UpdateFrame` bytes. The pointer is valid
// only for the callback duration; Swift copies before decoding.
typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);

// Persistent storage directory for the LMDB EventStore backend. Must be called
// before `nmp_app_start`; a NULL or empty `path` clears it. Inert unless
// nmp-core is built with the `lmdb-backend` feature.
void nmp_app_set_storage_path(void *app, const char *path);

void nmp_app_start(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);

// ── Profile claim / release (real relay data) ────────────────────────────

// Claim a profile for `pubkey`. The kernel keeps a refcounted interest open
// across all consumers (`consumer_id` is the bookkeeping key for matched
// release calls). Claimed profiles are projected under
// `projections.claimed_profiles[pubkey]` in the regular update snapshot.
void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id);
void nmp_app_release_profile(void *app, const char *pubkey, const char *consumer_id);

// Open an author view on `pubkey`. The kernel fetches kind:10002 + kind:0
// from discovery relays and surfaces the resolved `ProfileCard` under
// `projections.author_view.profile` in the push-callback snapshot. Refcounted
// — paired with `nmp_app_close_author`.
void nmp_app_open_author(void *app, const char *pubkey);
void nmp_app_close_author(void *app, const char *pubkey);

// ── Event claim / release (kind-dispatch embed) ──────────────────────────

// Claim an embedded event by `nostr:` URI (T180 / ADR-0034). Refcounted per
// `consumer_id`; the kernel fetches the event over the OneshotApi
// (single-writer interest registration — D4) when not yet in the store, and
// surfaces it in `snapshot.projections.claimed_events` keyed by `primary_id`
// (event-id hex for `nevent`/`note`; `"kind:pubkey:d"` for `naddr`).
// FFI-clean (D6): null/invalid arguments are silent no-ops, never panics.
// D8: forwards to the actor; no polling, no sync wait.
void nmp_app_claim_event(void *app, const char *uri, const char *consumer_id);
void nmp_app_release_event(void *app, const char *uri, const char *consumer_id);

// ── Relay management ─────────────────────────────────────────────────────

// Add a relay row (operator-supplied), canonicalizing the URL and dialing a
// real socket. The kernel uses the resulting `app_relays` set for routing
// when there is no logged-in user and threads it through the planner so
// kind:0 / kind:10002 lookups can reach a peer. `role` accepts `"read"`,
// `"write"`, or `"both"` (NULL → `"both"`). Mirrors the corresponding entry
// in Chirp's `NmpCore.h`; kept hand-in-sync by
// `ci/check-ffi-header-drift.sh`.
void nmp_app_add_relay(void *app, const char *url, const char *role);
void nmp_app_remove_relay(void *app, const char *url);

// ── Generic action dispatch (phase 2 / write surface) ────────────────────

// Single namespace-keyed entry point for the M6 `ActionModule` family. The
// gallery uses it (phase 2) for the showcase "publish a note" page. Returns a
// heap-allocated JSON envelope (`{"correlation_id":"<32-hex>"}` or
// `{"error":"…"}`) the caller MUST free via `nmp_app_free_string`.
char *nmp_app_dispatch_action(void *app, const char *namespace, const char *action_json);

// ── Showcase sign-in (phase 2) ───────────────────────────────────────────

// Sign in with a raw nsec / hex secret. Fire-and-forget (D6): outcome arrives
// through the snapshot's `accounts` / `last_error_toast` fields.
void nmp_app_signin_nsec(void *app, const char *secret);

// ── Gallery projection (per-app FFI) ─────────────────────────────────────
//
// `libnmp_app_gallery.a` is the gallery-specific aggregate archive. Doctrine
// D0 keeps the gallery's bespoke projection outside `nmp-core` while still
// letting the iOS shell link a single Rust archive.
//
// Profile-data flow (CRITICAL): all kernel state arrives via the push
// callback registered with `nmp_app_set_update_callback`; the FlatBuffers
// update frame the kernel passes to that callback carries the full snapshot.
// Identical to Chirp's update-channel pattern. There is no pull-side snapshot
// accessor — kernel liveness is observed through `nmp_app_is_alive`.
//
// Flow:
// 1. Call `nmp_app_gallery_register(app)` once after `nmp_app_new()` succeeds
//    and BEFORE `nmp_app_start`. Silent no-op on a NULL app (D6).
// 2. Register the push callback via `nmp_app_set_update_callback`.
//    FlatBuffers update frames arrive on every emit tick.
//
// Fire-and-forget: every entry point degrades silently on null pointers,
// poisoned mutexes, or serialization failure (D6).
void nmp_app_gallery_register(void *app);
const char *nmp_app_gallery_showcase_references_json(void);

// ── Heap-string release ──────────────────────────────────────────────────

void nmp_app_free_string(char *ptr);

#endif
