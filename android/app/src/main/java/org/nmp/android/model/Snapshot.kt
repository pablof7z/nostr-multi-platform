package org.nmp.android.model

import kotlinx.serialization.Serializable

/**
 * Decoded shape of the kernel JSON snapshot — a strict subset of the iOS
 * `KernelUpdate` (see `ios/Chirp/.../KernelBridge.swift`). Every field is
 * nullable / defaulted so an older or trimmed kernel build still decodes and
 * the model keeps its prior value (D1: best-effort, fail-closed). Property
 * names are camelCase; JSON is snake_case via `JsonNamingStrategy.SnakeCase`.
 *
 * NO derived state lives here — this is a verbatim mirror (D8).
 */
@Serializable
data class KernelUpdate(
    val rev: Long = 0,
    val running: Boolean = false,
    val relayUrl: String = "",
    val testNpub: String = "",
    val items: List<TimelineItem> = emptyList(),
    val modularTimeline: ChirpTimelineSnapshot = ChirpTimelineSnapshot(),
    val metrics: KernelMetricsLite? = null,
    val relayStatuses: List<RelayStatus> = emptyList(),
)

@Serializable
data class TimelineItem(
    val id: String = "",
    val authorDisplay: String = "",
    val authorAvatarInitials: String = "",
    val authorAvatarColor: String = "",
    val content: String = "",
    val contentPreview: String = "",
    val createdAtDisplay: String = "",
    val relayCount: Long = 0,
)

@Serializable
data class KernelMetricsLite(
    val storedEvents: Long = 0,
    val visibleItems: Long = 0,
    val eventsRx: Long = 0,
    val updateSequence: Long = 0,
)

@Serializable
data class RelayStatus(
    val role: String = "",
    val relayUrl: String = "",
    val connection: String = "",
    val auth: String = "",
    val activeWireSubscriptions: Int = 0,
    val reconnectCount: Long = 0,
)
