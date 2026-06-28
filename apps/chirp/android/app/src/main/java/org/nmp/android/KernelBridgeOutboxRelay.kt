package org.nmp.android

import java.util.UUID
import org.nmp.android.model.RelayStatus

/**
 * Sibling extension surface for [KernelBridge] covering the publish-outbox
 * control plane (#1291 GAP 4) and NIP-65 / NIP-17 relay-list publishing
 * (#1291 GAP 5). Split out of [KernelBridge] to keep that file under the
 * 500-LOC ceiling (AGENTS.md File Size).
 *
 * Thin-shell rule: no business logic here. The relay-list publishers are typed
 * Kotlin methods; the namespace/body transport is private to this bridge file.
 * The outbox control plane wraps the `nmp_app_retry_publish` (by handle) /
 * `nmp_app_cancel_action` (by operation correlation_id; S7/#1754 replaced
 * `nmp_app_cancel_publish`) C-ABI symbols. Rust owns all policy (which relays
 * receive the kind:10002, retry backoff, etc.).
 */

/**
 * Retry a failed publish by its correlation id (outbox UI). Mirrors iOS
 * `KernelHandle.retryPublish(handle:)`. D6: a null/dead handle is a no-op.
 */
fun KernelBridge.retryPublish(correlationId: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeRetryPublish(handle, correlationId)
}

/**
 * Cancel an in-flight publish by its correlation id (outbox UI). Mirrors iOS
 * `KernelHandle.cancelPublish(handle:)`. D6: a null/dead handle is a no-op.
 */
fun KernelBridge.cancelPublish(correlationId: String) {
    val handle = rawHandle()
    if (handle != 0L) nativeCancelPublish(handle, correlationId)
}

/**
 * Publish the NIP-65 (kind:10002) relay-list metadata event via the typed
 * FlatBuffers byte builder (M14-1 / #2145). Mirrors iOS
 * `KernelHandle.publishRelayList(relays:)` — forwards each relay's verbatim
 * `url` + kernel-authored `role` string; Rust normalizes composite roles and
 * skips indexer-only rows when building the kind:10002 tags.
 */
fun KernelBridge.publishRelayList(relays: List<RelayStatus>): DispatchResult {
    val id = UUID.randomUUID().toString()
    val entries = relays.map { Pair(it.relayUrl, it.role) }
    return dispatchBytes(GeneratedActionBuilders.publishRelayList(id, entries))
}

/**
 * Publish the NIP-17 DM relay-list (kind:10050) via the typed FlatBuffers byte
 * builder (M14-1 / #2145). Mirrors iOS `KernelHandle.publishDmRelayList(relays:)`
 * — a flat `wss://` URL array.
 */
fun KernelBridge.publishDmRelayList(relays: List<String>): DispatchResult {
    val id = UUID.randomUUID().toString()
    return dispatchBytes(GeneratedActionBuilders.publishDmRelayList(id, relays))
}
