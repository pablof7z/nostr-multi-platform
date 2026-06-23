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

// Persistent storage directory for the LMDB EventStore backend. Must be called
// before `nmp_app_start`; a NULL or empty `path` clears it. Inert unless
// nmp-core is built with the `lmdb-backend` feature. Returns
// NmpConfigStatus_AlreadyStarted if called after nmp_app_start.
uint32_t nmp_app_set_storage_path(void *app, const char *path);

void nmp_app_start(void *app, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);

// ── Reference resolution (ADR-0063 #1671) ────────────────────────────────

// Unified, origin-blind reference-resolution entry points. The gallery resolves
// visible profiles through these (superseding the deleted
// nmp_app_claim_profile / nmp_app_release_profile surface). The resolved kind:0
// flows back through the kernel's `refs.profile` row-delta projection (merged
// host-side into the GalleryRefProfileStore; see below).
//
// `namespace` — 0 = profile (kind:0).
// `key` — lowercase 64-hex pubkey.
// `consumer_id` — opaque refcount owner key (e.g. SwiftUI view identity).
// `shape` — 0 = profile.ref (avatar / name), 1 = profile.card (full card).
// `liveness` — 0 = CacheOk (background / feed row), non-zero = Live (open screen).
// D6: null/invalid args and unknown int codes are silent no-ops, never panics.
// D8: fire-and-forget; the actor processes commands asynchronously.
void nmp_app_resolve_ref(void *app, int namespace, const char *key,
                         const char *consumer_id, int shape, int liveness);
void nmp_app_release_ref(void *app, int namespace, const char *key,
                         const char *consumer_id);

// ── Event claim / release (kind-dispatch embed) ──────────────────────────

// #1726: nmp_app_claim_event / nmp_app_release_event DELETED.
// Callers decode the nostr: URI via nmp_nip21_decode_uri and then route to
// nmp_app_resolve_ref(namespace=1/*event*/, key=event-id-hex, shape=2/*embed*/,
// liveness=0/*CacheOk*/) and nmp_app_release_ref(namespace=1, key, consumer_id).

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

// ── Generic action dispatch (phase 2 / ADR-0064 Cut-B #1756) ────────────

// Typed byte doorway for gallery writes. Rust encodes `body_json` into the
// typed `ActionPayload` FlatBuffers bytes for `namespace` and dispatches
// through `nmp_app_dispatch_action_bytes`. No JSON crosses the FFI to the
// kernel. Returns a heap-allocated JSON envelope (`{"correlation_id":"<id>"}` or
// `{"error":"…"}`) the caller MUST free via `nmp_free_string`.
char *nmp_app_gallery_dispatch_action_bytes(void *app, const char *namespace, const char *body_json);

// ── Showcase sign-in (phase 2) ───────────────────────────────────────────

// Sign in with a raw nsec / hex secret. Fire-and-forget (D6): outcome arrives
// through the snapshot's `accounts` / `last_error_toast` fields.
void nmp_app_signin_nsec(void *app, const char *secret, uint8_t make_active);

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

// ── refs.profile host mirror (ADR-0063 #1671) ────────────────────────────
//
// Opaque host-owned mirror of the kernel's `refs.profile` row-delta projection.
// The shell allocates ONE per kernel session and threads it into every
// `nmp_app_gallery_snapshot_json_from_update_frame` call so per-key profile
// deltas accumulate across frames (the sidecar carries only changed/cleared
// rows — a single frame cannot be decoded in isolation). Sole app-side profile
// store (D4). Release with `nmp_app_gallery_ref_profile_store_free`.
typedef struct GalleryRefProfileStore GalleryRefProfileStore;
struct GalleryRefProfileStore *nmp_app_gallery_ref_profile_store_new(void);
void nmp_app_gallery_ref_profile_store_free(struct GalleryRefProfileStore *store);

// Decode borrowed FlatBuffers `nmp.transport.UpdateFrame` bytes into the
// Gallery snapshot JSON shape, merging the frame's `refs.profile` row-delta
// batch into `store` first (the rendered `refs.profile` JSON map is sourced
// from that store). `store` MUST persist across calls for one kernel session.
// Returns a heap string that MUST be released via `nmp_free_string`; returns
// NULL for a NULL store, malformed frames, or decode failures.
char *nmp_app_gallery_snapshot_json_from_update_frame(struct GalleryRefProfileStore *store,
                                                      const uint8_t *bytes, uintptr_t len);

// ── Stateless NIP-21 / NIP-19 decode ────────────────────────────────────
//
// Decode a `nostr:` URI or bare bech32 entity. Returns bounded JSON:
//   {"ok":true,"target":"event","event_id":"<hex>", ...}
// or {"ok":false,"error":"..."}.
// The returned string is never NULL and MUST be freed via nmp_free_string.
// Used by the gallery event-claim path to extract the event key from a URI
// before calling nmp_app_resolve_ref (see the deleted nmp_app_claim_event note).
char *nmp_nip21_decode_uri(const char *input);

// ── Heap-string release ──────────────────────────────────────────────────

// Release a `*mut c_char` returned by any NMP FFI function. Passing NULL is a
// no-op (D6). This is the ONLY symbol the caller must invoke to free NMP-heap
// strings — `nmp_app_free_string` and `nmp_broker_free_string` are removed.
void nmp_free_string(char *ptr);

#endif
