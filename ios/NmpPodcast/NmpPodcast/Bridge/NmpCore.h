#ifndef NMP_CORE_H
#define NMP_CORE_H

#include <stdint.h>

// T156 — NmpPodcast bridging header. Mirrors ios/Chirp/Chirp/Bridge/NmpCore.h
// but trimmed to only the symbols the podcast app currently exercises. New
// kernel symbols (capability socket, NIP-46 broker, wallet, etc.) come in as
// the podcast app grows feature surface, kept in lockstep with the FFI surface
// audit at docs/ffi-surface.md.

// --- nmp-core kernel handle ---
void *nmp_app_new(void);
void nmp_app_free(void *app);

typedef void (*NmpUpdateCallback)(void *context, const char *json);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
void nmp_app_start(void *app, unsigned int events_per_second, unsigned int visible_limit, unsigned int emit_hz);
void nmp_app_stop(void *app);

// scenePhase → kernel bridge (mirrors Chirp).
void nmp_app_lifecycle_foreground(void *app);
void nmp_app_lifecycle_background(void *app);

// --- nmp-app-podcast per-app FFI ---
//
// `libnmp_app_podcast.a` is a separate Rust static library. D0 forbids
// `nmp-core` from gaining podcast nouns, so the podcast composition layer
// ships its own static archive — same packaging rule as nmp-signer-broker /
// nmp-app-chirp.
//
// Flow:
// 1. Call `nmp_app_podcast_register(app)` once after `nmp_app_new()` succeeds.
//    Returns an opaque handle (or NULL on null `app`).
// 2. Whenever the shell needs the current library, call
//    `nmp_app_podcast_snapshot(handle)` to get a JSON
//    `{ "podcasts": [{ "id", "title", "author", "artwork_url", "episode_count" }] }`.
//    The shell owns the returned pointer until it calls
//    `nmp_app_podcast_snapshot_free(ptr)`.
// 3. Dispatch user intents via `nmp_app_podcast_subscribe(handle, feed_url,
//    title_or_null, author_or_null)` and `nmp_app_podcast_unsubscribe(handle,
//    podcast_id)`. Both are fire-and-forget (D6).
// 4. On teardown, call `nmp_app_podcast_unregister(handle)` BEFORE
//    `nmp_app_free(app)`.
//
// Every entry point degrades silently on null pointers, invalid UTF-8,
// malformed URLs / ULIDs, or serialization failures (D6).
void *nmp_app_podcast_register(void *app);
char *nmp_app_podcast_snapshot(void *handle);
void nmp_app_podcast_snapshot_free(char *ptr);
void nmp_app_podcast_subscribe(void *handle, const char *feed_url,
                                const char *title_or_null, const char *author_or_null);
void nmp_app_podcast_unsubscribe(void *handle, const char *podcast_id);
void nmp_app_podcast_unregister(void *handle);

#endif
