---
title: Chirp Projection Cache and Rev Tracking
slug: chirp-projection-cache
topic: app-projection
summary: App-owned projection keys (non-manifest keys like `chirp.timeline.home`) are absent from the kernel's builtin rev manifest, so without a fix they emit `projecti
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Projection Cache and Rev Tracking

## App-Owned Projection Keys

App-owned projection keys (non-manifest keys like `chirp.timeline.home`) are absent from the kernel's builtin rev manifest, so without a fix they emit `projection_rev = 0` on every tick, causing rev-aware host caches (`ProjectionCache.generated.swift` and `ProjectionCache.kt`) to skip every `Changed` row after the first. The op-feed projection_rev is a per-key monotonic u64 that advances when content changes, used by rev-aware host caches to skip re-decoding unchanged payloads. The projection_rev fix (NMP#2952) derives a content-driven rev for app-owned keys via a per-key counter that increments when the payload fingerprint changes, so the rev advances if and only if content changed. App-owned projection keys must emit a content-advancing projection_rev so rev-aware host caches decode updated frames instead of skipping them as stale. The kernel has zero write-chokepoint visibility into host-registered projections (opaque payload bytes from `run_typed_projections`), so content fingerprinting is the only available signal for app-owned key rev advancement. Cleared rows keep `projection_rev = 0` on the wire, and the host cache removes the entry on `Cleared`, so a subsequent `Changed` row is admitted because the cache entry is nil. The `app_owned_revs` tracker never prunes Cleared entries to preserve monotonicity, so a re-opened key resumes above its last-emitted rev. The `ProjectionRevTracker` is created once at `kernel_new` and `app_owned_revs` is never reset, including across `reset_last_emitted`. Rung 3 omission is decided by manifest presence, never by `projection_rev`; keys absent from the manifest default to `Changed` and are always kept, never omitted. This fix corrects iOS, Android, and web simultaneously with no per-platform host-side code change required.

The op-feed per-key projection_rev never advances (stuck at 0) for app-owned keys absent from the kernel's builtin rev manifest. This is the root cause of the device empty-feed (NMP#2944): app-owned projection keys emit `projection_rev = 0` on every tick, and the generated host `ProjectionCache` skips `Changed` rows when `incomingRev <= cached.rev`, freezing the feed at the first empty frame. It is also a latent trap for any host that adopts Rung-3 per-key skip, filed as NMP#2943.

<!-- citations: [^dcc80-10a76] [^dcc80-af989] [^dcc80-2451] [^dcc80-15b6e] [^dcc80-2447] [^dcc80-1f337] [^dcc80-b95e3] [^dcc80-864d4] [^dcc80-42518] [^dcc80-9f550] -->
## Host Cache Behavior

The generated iOS and Android `ProjectionCache` host caches skip a `Changed` row when `incomingRev <= cached.rev`, and remove the entry on `Cleared`. A frozen `projection_rev = 0` on app-owned keys causes the host to skip every card-bearing frame after the first empty frame is committed, so app-owned keys must emit an advancing `projection_rev` to avoid the host permanently skipping card-bearing frames.

The Chirp iOS and Android platform decoders' op-feed typed-projection extraction is covered by host live-path tests that feed frame bytes through `ProjectionMergeCache.merge` → `TypedHomeFeedDecoder.decode`, testing both a stuck-rev freeze and an advancing-rev render.

<!-- citations: [^dcc80-7433b] [^dcc80-9e7c9] [^dcc80-2447] [^dcc80-e80be] [^dcc80-cc514] [^dcc80-a2247] -->
## Row Re-Derivation

Each row re-derives its counts by scanning the same pushed frame for its own claimed `projection_key`. This is proven end-to-end with a real actor, real ingest, and real pushed-frame decode. <!-- [^dcc80-2920] --> <!-- [^dcc80-4f268] -->
