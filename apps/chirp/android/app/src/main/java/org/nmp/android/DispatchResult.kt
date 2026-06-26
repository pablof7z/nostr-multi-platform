package org.nmp.android

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Android peer of Swift `DispatchResult`.
 *
 * The kernel returns exactly one JSON envelope from `nmp_app_dispatch_action`:
 * accepted actions carry `correlation_id`; synchronous rejects carry `error`.
 * Terminal outcomes still arrive later through the snapshot projections.
 */
sealed class DispatchResult {
    abstract val correlationId: String?

    data class Accepted(override val correlationId: String) : DispatchResult()
    data class Failure(val message: String) : DispatchResult() {
        override val correlationId: String? = null
    }

    companion object {
        /**
         * Convert a UniFFI [org.nmp.android.uniffi.DispatchAck] returned by
         * [AppHandle][org.nmp.android.uniffi.AppHandle] dispatch methods into a
         * typed [DispatchResult] (M14-0 / issue #2129).
         *
         * The mapping is 1-to-1: a non-null [correlationId][org.nmp.android.uniffi.DispatchAck.correlationId]
         * wins; otherwise the [error][org.nmp.android.uniffi.DispatchAck.error] field is used.
         */
        fun fromAck(ack: org.nmp.android.uniffi.DispatchAck): DispatchResult {
            val correlationId = ack.correlationId
            if (!correlationId.isNullOrEmpty()) return Accepted(correlationId)
            val error = ack.error ?: "dispatch returned ack with no correlation_id and no error"
            return Failure(error)
        }

        fun parse(envelope: String): DispatchResult {
            val obj: JsonObject = try {
                Json.parseToJsonElement(envelope).jsonObject
            } catch (_: Exception) {
                return Failure("dispatch envelope was not a JSON object (bytes=${envelope.length})")
            }
            val correlationId = obj["correlation_id"]?.jsonPrimitive?.contentOrNull
            if (!correlationId.isNullOrEmpty()) {
                return Accepted(correlationId)
            }
            val error = obj["error"]?.jsonPrimitive?.contentOrNull
            if (error != null) {
                return Failure(error)
            }
            return Failure("dispatch envelope missing both correlation_id and error (bytes=${envelope.length})")
        }
    }
}
