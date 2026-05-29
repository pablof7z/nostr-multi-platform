package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Decoded shape of the kernel JSON snapshot — Android peer of iOS
 * `KernelUpdate` (see `ios/Chirp/.../KernelBridge.swift`). Every field is
 * nullable / defaulted so an older or trimmed kernel build still decodes
 * (D1: best-effort, fail-closed). Property names are camelCase; JSON is
 * snake_case via `JsonNamingStrategy.SnakeCase`.
 *
 * NO derived state lives here — this is a verbatim mirror (D8).
 */
@Serializable
data class KernelUpdate(
    val rev: Long = 0,
    val running: Boolean = false,
    val relayUrl: String = "",
    @SerialName("items") val legacyItems: List<TimelineItem> = emptyList(),
    val modularTimeline: ChirpOpFeedSnapshot = ChirpOpFeedSnapshot(),
    val metrics: KernelMetricsLite? = null,
    val relayStatuses: List<RelayStatus> = emptyList(),
    val lastErrorToast: String? = null,
    val projections: SnapshotProjections? = null,
) {
    val items: List<TimelineItem>
        get() = projections?.timeline ?: legacyItems

    val activeAccount: String
        get() = projections?.activeAccount.orEmpty()
}

@Serializable
data class SnapshotProjections(
    @SerialName("active_account") val activeAccount: String? = null,
    val accounts: List<AccountSummary> = emptyList(),
    val timeline: List<TimelineItem> = emptyList(),
    @SerialName("nmp.nip17.dm_inbox") val dmInbox: DmInboxSnapshot? = null,
    @SerialName("wallet_status") val walletStatus: String? = null,
    @SerialName("wallet_balance") val walletBalance: String? = null,
)

@Serializable
data class AccountSummary(
    val id: String = "",
    val npubShort: String = "",
    val displayName: String = "",
    val status: String = "",
    val signerLabel: String = "",
)

@Serializable
data class TimelineItem(
    val id: String = "",
    // aim.md §2 — backend ships raw hex pubkey + Unix seconds; the
    // Compose layer is the presentation surface and formats author
    // display / relative-time labels at render time.
    val authorPubkey: String = "",
    val content: String = "",
    val contentPreview: String = "",
    val createdAt: Long = 0,
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

@Serializable
data class DmMessage(
    val id: String = "",
    val senderPubkey: String = "",
    val content: String = "",
    val createdAt: Long = 0,
    val replyTo: String? = null,
    val isOutgoing: Boolean = false,
    val sourceRelays: List<String>? = null,
)

@Serializable
data class DmConversation(
    val peerPubkey: String = "",
    val messages: List<DmMessage> = emptyList(),
)

@Serializable
data class DmInboxSnapshot(
    val conversations: List<DmConversation> = emptyList(),
    val remoteSignerUnsupported: Boolean = false,
)
