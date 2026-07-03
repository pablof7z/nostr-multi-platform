package org.nmp.registry

data class NostrGroupChatReactionWire(
    val emoji: String,
    val count: Int,
)

data class NostrGroupChatMessageWire(
    val id: String,
    val authorPubkey: String,
    val content: String,
    val createdAtLabel: String,
    val replyPreview: String? = null,
    val reactions: List<NostrGroupChatReactionWire> = emptyList(),
    val isOutgoing: Boolean = false,
)

data class NostrGroupChatParticipantWire(
    val pubkey: String,
    val roleLabel: String? = null,
    val statusLabel: String? = null,
)
