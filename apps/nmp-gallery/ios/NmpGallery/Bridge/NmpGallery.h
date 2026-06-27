#ifndef NMP_GALLERY_H
#define NMP_GALLERY_H

#include <stdbool.h>
#include <stdint.h>

// NmpGallery is a developer-facing component gallery for the NMP registry.
// It links one aggregate Rust archive — `libnmp_app_gallery.a` — that bundles
// the NMP kernel symbols plus a gallery-tailored projection. The subset of the
// NMP C-ABI declared below is exactly what the gallery shell needs; matching
// declarations live in `apps/chirp/ios/Chirp/Bridge/NmpCore.h` (kept hand-in-sync by
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

// Typed reference-resolution entry points. The gallery resolves visible
// profiles and event embeds through these, not through raw
// namespace/shape/liveness integers. Profiles flow back through `refs.profile`;
// event embeds flow back through the resolved event-ref/embed projections.
// D6: null/invalid args are silent no-ops, never panics.
// D8: fire-and-forget; the actor processes commands asynchronously.
void nmp_app_resolve_profile_ref(void *app, const char *key,
                                 const char *consumer_id);
void nmp_app_release_profile_ref(void *app, const char *key,
                                 const char *consumer_id);
void nmp_app_resolve_event_embed_with_metadata(void *app, const char *key,
                                               const char *consumer_id,
                                               const char *metadata_json);
void nmp_app_resolve_event_embed_live_with_metadata(void *app, const char *key,
                                                    const char *consumer_id,
                                                    const char *metadata_json);
void nmp_app_release_event_ref(void *app, const char *key,
                               const char *consumer_id);

// ── Event-ref resolve / release (kind-dispatch embed) ────────────────────

// Event URI front doors are removed. Callers decode the nostr: URI via
// nmp_nip21_decode_uri and then route to the typed event-embed adapters above.

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

// ── Bridge-private action dispatch (phase 2 / ADR-0064 Cut-B #1756) ──────

// Compatibility doorway for gallery bridge internals. App code should expose
// typed write methods rather than `(namespace, body_json)` dispatch. Rust
// encodes `body_json` into typed `ActionPayload` FlatBuffers bytes for
// `namespace` and dispatches through `nmp_app_dispatch_action_bytes`. Returns a
// heap-allocated JSON envelope (`{"correlation_id":"<id>"}` or
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

// ── refs.* host mirrors (ADR-0063 #1671) ─────────────────────────────────
//
// Opaque host-owned mirrors of the kernel's `refs.profile` / `refs.event`
// row-delta projections. The shell allocates ONE per kernel session and
// threads it into every `nmp_app_gallery_snapshot_json_from_update_frame` call
// so per-key ref deltas accumulate across frames (the sidecars carry only
// changed/cleared rows — a single frame cannot be decoded in isolation). Sole
// app-side ref stores (D4). Release with `nmp_app_gallery_ref_stores_free`.
typedef struct GalleryRefStores GalleryRefStores;
struct GalleryRefStores *nmp_app_gallery_ref_stores_new(void);
void nmp_app_gallery_ref_stores_free(struct GalleryRefStores *stores);

// Decode borrowed FlatBuffers `nmp.transport.UpdateFrame` bytes into the
// Gallery snapshot JSON shape, merging the frame's `refs.profile` /
// `refs.event` row-delta batches into `stores` first (`refs.profile` is
// rendered from the profile store; `refs.event.envelopes` is derived from the
// event store).
// `stores` MUST persist across calls for one kernel session.
// Returns a heap string that MUST be released via `nmp_free_string`; returns
// NULL for NULL stores, malformed frames, or decode failures.
char *nmp_app_gallery_snapshot_json_from_update_frame(struct GalleryRefStores *stores,
                                                      const uint8_t *bytes, uintptr_t len);

// ── Stateless NIP-21 / NIP-19 decode ────────────────────────────────────
//
// Decode a `nostr:` URI or bare bech32 entity. Returns bounded JSON:
//   {"ok":true,"target":"event","event_id":"<hex>", ...}
// or {"ok":false,"error":"..."}.
// The returned string is never NULL and MUST be freed via nmp_free_string.
// Used by app-owned URI adapters to extract the event key before calling
// nmp_app_resolve_ref.
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

// ── Heap-string release ──────────────────────────────────────────────────

// Release a `*mut c_char` returned by any NMP FFI function. Passing NULL is a
// no-op (D6). This is the ONLY symbol the caller must invoke to free NMP-heap
// strings — `nmp_app_free_string` and `nmp_broker_free_string` are removed.
void nmp_free_string(char *ptr);

#endif
