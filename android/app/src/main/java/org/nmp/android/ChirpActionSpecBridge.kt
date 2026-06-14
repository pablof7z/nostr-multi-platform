package org.nmp.android

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

@OptIn(ExperimentalSerializationApi::class)
internal val chirpActionJson = Json {
    encodeDefaults = false
    explicitNulls = false
    ignoreUnknownKeys = true
}

@Serializable
internal data class ChirpActionIntent(
    @SerialName("type") val type: String,
    val content: String? = null,
    @SerialName("reply_to_event_id") val replyToEventId: String? = null,
    @SerialName("event_id") val eventId: String? = null,
    @SerialName("author_pubkey") val authorPubkey: String? = null,
    val reaction: String? = null,
    val pubkey: String? = null,
    @SerialName("recipient_pubkey") val recipientPubkey: String? = null,
    @SerialName("amount_msats") val amountMsats: Long? = null,
    @SerialName("target_event_id") val targetEventId: String? = null,
    val lnurl: String? = null,
    val comment: String? = null,
)

@Serializable
internal data class ChirpActionSpec(
    val namespace: String = "",
    @SerialName("body_json") val bodyJson: String = "",
    val error: String? = null,
)
