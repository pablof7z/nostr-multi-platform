package org.nmp.android

/**
 * Sibling extension surface for [KernelBridge] covering demand-driven reference
 * resolution: event claim/release surface + the ADR-0063 Lane D/G unified
 * resolve_ref / release_ref seam (#1671).
 *
 * ADR-0063 Lane H: the old profile-claim bridge was deleted; use resolveRef
 * with RefNamespace.Profile instead.
 *
 * Split out of [KernelBridge] to keep that file under the 500-LOC ceiling
 * (AGENTS.md File Size). Same package — no import required. Thin-shell rule:
 * no business logic here; Rust owns all resolution policy (D7).
 */

/**
 * Demand-driven embedded-event fetch claim (#984 / T180 / ADR-0034): the UI
 * is rendering an out-of-feed `EventRef` ([uri] is the verbatim
 * `nevent`/`note`/`naddr` URI) under [consumerId]; the kernel resolves the
 * event (cache-first, then relay) and ships its typed row in `refs.event`;
 * Chirp also receives the derived `refs.event.envelopes` sidecar
 * (`projections.refEventEnvelopes`). App-local wrappers named `claimEvent` /
 * `nativeClaimEvent` are URI adapters over unified `resolve_ref`, not kernel or
 * C front doors.
 *
 * Idempotent per (uri, consumerId); the matching [releaseEvent] must be
 * called when the embed leaves the composition so the kernel reclaims the
 * resolution interest.
 */
fun KernelBridge.claimEvent(uri: String, consumerId: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeClaimEvent(handle, uri, consumerId)
}

/**
 * Demand-driven embedded-event fetch release (#984): the UI no longer needs
 * [uri] under [consumerId]. When the last consumer releases, the kernel
 * drops the resolution interest. Safe to call even if no matching claim is
 * live.
 */
fun KernelBridge.releaseEvent(uri: String, consumerId: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeReleaseEvent(handle, uri, consumerId)
}

// ── ADR-0063 Lane G (#1671) — unified resolve_ref / release_ref ──────────

/**
 * ADR-0063 Lane D/G (#1671) — unified, origin-blind reference resolution. The
 * Android twin of iOS `KernelHandle.resolveRef`. Supersedes the deleted legacy
 * profile-claim surface (ADR-0063 Lane H).
 *
 * [namespace] — [RefNamespace] (profile / event).
 * [key] — lowercase 64-hex pubkey (profile) or event-id hex / `kind:pubkey:d`
 *   (event).
 * [consumerId] — opaque refcount owner key (e.g. a Compose item key).
 * [shape] — the requested [RefShape] projection subset.
 * [liveness] — [RefLiveness]: CacheOk (background) vs Live (open screen).
 *
 * Idempotent per `(namespace, key, consumerId)`; the matching [releaseRef]
 * must be called on teardown so the kernel reclaims the resolver slot.
 */
fun KernelBridge.resolveRef(
    namespace: RefNamespace,
    key: String,
    consumerId: String,
    shape: RefShape,
    liveness: RefLiveness,
) {
    val handle = rawHandle()
    if (handle != 0L) {
        nativeResolveRef(
            handle,
            namespace.code,
            key,
            consumerId,
            shape.code,
            liveness.code,
        )
    }
}

/**
 * ADR-0063 Lane D/G (#1671) — release a reference previously registered via
 * [resolveRef]. Decrements the per-consumer refcount; the resolver slot is
 * torn down when the last consumer releases. Safe to call with no live claim.
 */
fun KernelBridge.releaseRef(namespace: RefNamespace, key: String, consumerId: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeReleaseRef(handle, namespace.code, key, consumerId)
}
