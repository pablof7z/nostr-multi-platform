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

typedef void (*NmpUpdateCallback)(void *context, const char *json);
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
// release calls). The gallery uses one consumer id — `"gallery"`.
void nmp_app_claim_profile(void *app, const char *pubkey, const char *consumer_id);
void nmp_app_release_profile(void *app, const char *pubkey, const char *consumer_id);

// ── Generic action dispatch (phase 2 / write surface) ────────────────────

// Single namespace-keyed entry point for the M6 `ActionModule` family. The
// gallery uses it (phase 2) for the demo "publish a note" page. Returns a
// heap-allocated JSON envelope (`{"correlation_id":"<32-hex>"}` or
// `{"error":"…"}`) the caller MUST free via `nmp_app_free_string`.
char *nmp_app_dispatch_action(void *app, const char *namespace, const char *action_json);

// ── Demo sign-in (phase 2) ───────────────────────────────────────────────

// Sign in with a raw nsec / hex secret. Fire-and-forget (D6): outcome arrives
// through the snapshot's `accounts` / `last_error_toast` fields.
void nmp_app_signin_nsec(void *app, const char *secret);

// ── Gallery projection (per-app FFI) ─────────────────────────────────────
//
// `libnmp_app_gallery.a` is the gallery-specific aggregate archive. Doctrine
// D0 keeps the gallery's bespoke projection outside `nmp-core` while still
// letting the iOS shell link a single Rust archive.
//
// Profile-data flow (CRITICAL): profile data does NOT travel through
// `nmp_app_gallery_snapshot`. Profile data arrives via the push callback
// registered with `nmp_app_set_update_callback`; the JSON the kernel passes
// to that callback carries the full snapshot including `profiles: {…}`.
// Identical to Chirp's update-channel pattern.
//
// `nmp_app_gallery_snapshot` returns a minimal status envelope only:
//   { "schema": <u32>, "alive": <bool>, "projections": {} }
// The gallery uses it for diagnostics / alive-checks, not for component data.
//
// Flow:
// 1. Call `nmp_app_gallery_register(app)` once after `nmp_app_new()` succeeds.
//    Returns an opaque handle, or NULL on any failure (D6).
// 2. Register the push callback via `nmp_app_set_update_callback`. Profile
//    JSON arrives on every emit tick.
// 3. `nmp_app_gallery_snapshot(handle)` is for status only; the shell owns
//    the returned pointer until it calls `nmp_app_gallery_snapshot_free(ptr)`.
//
// Fire-and-forget: every entry point degrades silently on null pointers,
// poisoned mutexes, or serialization failure (D6).
void *nmp_app_gallery_register(void *app);
char *nmp_app_gallery_snapshot(void *handle);
void nmp_app_gallery_snapshot_free(char *ptr);

// ── Heap-string release ──────────────────────────────────────────────────

void nmp_app_free_string(char *ptr);

#endif
