# T-podcast-gap-3 — host HTTP-fetch capability for feed download

> **Surfaced by:** T-podcast-android-3
> **Filed:** 2026-05-18
> **Status:** Open — blocks automatic feed refresh on subscribe
> **Depends on:** Nothing blocking — can be implemented independently

## Symptom

`nmp-app-podcast` can parse RSS/Atom feed bytes injected by the host
platform, but the crate itself does NOT perform HTTP fetching. The kernel
has no generic HTTP-fetch capability, so there is no path for the Rust
side to autonomously download a feed on subscribe.

As a result:
- `nmp_app_podcast_subscribe` stores the feed URL and title/author metadata
  but leaves the episode list empty.
- The host must separately fetch the bytes and call
  `nmp_app_podcast_ingest_bytes(handle, feed_url, bytes_ptr, bytes_len)`
  to populate episodes.

## Evidence

```bash
# nmp-app-podcast has no HTTP dep; fetch capability doesn't exist in nmp-core
$ grep -RE 'reqwest|ureq|HttpCapability|http_fetch' crates/nmp-core/src/
(no matches)
$ grep -RE 'reqwest|ureq' apps/podcast/
(no matches)
```

## What exists today (T-podcast-android-3)

The subscribe path stores metadata. The new `nmp_app_podcast_ingest_bytes`
FFI symbol accepts bytes from the host and runs `podcast-feeds::parser::parse_feed`
— all parsing is fully implemented. The shape is ready; the gap is the
"who fetches" question.

## Impact

**Android:** `PodcastKernelBridge.subscribe()` calls
`nmp_app_podcast_subscribe` end-to-end, but the Android side does NOT yet
call `nmp_app_podcast_ingest_bytes` after subscribing. Episodes are empty
after subscribe. The user sees `0 episodes` until an ingest call is made.

**iOS:** same — depends on the iOS shell calling `nmp_app_podcast_ingest_bytes`.

## Resolution options

**Option A — Host-side fetch (preferred, matches capability architecture)**

1. Android: after `subscribe()` succeeds, launch a Kotlin coroutine that
   fetches the feed via OkHttp and calls the new JNI method
   `nativeIngestBytes(handle, feedUrl, bytes)`.
2. iOS: after `subscribe()` succeeds, launch a `URLSession` task that fetches
   the feed and calls `nmp_app_podcast_ingest_bytes`.
3. The `nmp_app_podcast_ingest_bytes` symbol returns a JSON status the host
   can surface as a toast on failure (parse error or network error).

**Option B — Generic kernel HTTP capability**

Add an `HttpFetchCapability` to the kernel (generic, no podcast nouns) that
the host registers at startup. The podcast layer uses the capability via the
existing `nmp_app_dispatch_capability` FFI socket. This is the architecturally
cleanest path but adds ~1 day of kernel work before any podcast code benefits.

## Recommended path

**Option A** is the right immediate step — it's what the architecture already
implies (host provides transport, Rust provides parsing/state), and unblocks
the episode list UI without touching nmp-core. Option B can follow later as
a general capability for any use-case that needs HTTP.

## Wiring ready

The parse side is fully implemented. Once the Android/iOS host calls
`nativeIngestBytes` (Android) or `nmp_app_podcast_ingest_bytes` (iOS) with
real feed bytes, episodes appear in the snapshot immediately.

## Cross-references

- `apps/podcast/nmp-app-podcast/src/state.rs` — `ingest_feed_bytes` seam
- `apps/podcast/nmp-app-podcast/src/ffi.rs` — `nmp_app_podcast_ingest_bytes`
- `apps/podcast/podcast-feeds/src/parser.rs` — `parse_feed` implementation
- Parent: T-podcast-android-3 (this iteration)
- Related: T-podcast-gap-1 (kernel DomainModule), T-podcast-gap-2 (FFI surface)
